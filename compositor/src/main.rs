//! edex-comp: the Wayland compositor of the edex-rs desktop.
//!
//! It starts the edex-rs shell as a fullscreen backdrop and places every other
//! application window in the workspace area in the middle of the shell's frame.
//!
//! Based on Smithay's "smallvil" example compositor (MIT licence).

mod handlers;
mod input;
mod ipc;
mod monitors;
mod policy;
mod state;
mod udev;
mod winit;
mod displays;
mod focus;
mod xwayland;

use smithay::reexports::{
    calloop::EventLoop,
    wayland_server::Display,
};
use std::path::PathBuf;
pub use state::EdexComp;

const USAGE: &str = "\
Usage: edex-comp [OPTIONS] [-- SHELL_ARGS...]

Options:
  --shell <PATH>       Shell program to run (default: edex-rs next to this binary)
  --backend <KIND>     'winit' to run in a window, 'drm' to run on the hardware
                       (default: winit inside another desktop, otherwise drm)
  --session            Run as the login session: tell the user's background services
                       (such as the settings portal applications wait for) about it
  --no-xwayland        Do not start an X server for X11 applications
  --split              Treat the window's halves as two displays (for testing)
  --screenshot <FILE>  Save a PPM screenshot after 5 seconds and exit (for testing)
  -h, --help           Show this help

Keys (hold Super, or Ctrl+Alt):
  Space                Open the application launcher
  ,                    Open the settings
  Tab                  Switch between the terminal and open applications
  O                    Move the current application to the next display
  S                    Split the workspace between two windows, or undo that
  Left, Right          Give the keyboard to that half of a split
  F                    Full screen for the current display, or undo that
  [  ]  \\              Collapse or open the left, right and bottom panels
  Shift+R              Restart the shell, to load a newly installed build (its
                       terminal tabs close; applications stay open)
  Q                    Close the current application

Ctrl+Alt+Backspace ends the session; Ctrl+Alt+F1..F12 switch virtual terminals.";

/// The shell binary installed alongside this one, falling back to `$PATH`.
fn default_shell() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("edex-rs")))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("edex-rs"))
}

fn usage_error(message: &str) -> ! {
    eprintln!("edex-comp: {message}\n\n{USAGE}");
    std::process::exit(2);
}

/// Tells the user's D-Bus and systemd services where this session's display is.
///
/// Services that applications start on demand, the desktop portal above all, run
/// outside this process tree and see only that shared environment. Without a display
/// in it they fail to start, and every application waits out a long timeout on them.
fn announce_session(socket_name: &std::ffi::OsStr, x_display: Option<u32>) {
    let run = |program: &str, args: &[&str]| match std::process::Command::new(program).args(args).status() {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!("{program} {args:?} failed: {status}"),
        Err(e) => tracing::warn!("cannot run {program}: {e}"),
    };
    // Leftovers of another desktop's session would send those services to its display.
    run("systemctl", &["--user", "unset-environment", "DISPLAY", "GNOME_SETUP_DISPLAY"]);
    let mut variables = vec![
        "--systemd".to_string(),
        format!("WAYLAND_DISPLAY={}", socket_name.to_string_lossy()),
        "XDG_CURRENT_DESKTOP=edex-rs".to_string(),
        "XDG_SESSION_TYPE=wayland".to_string(),
    ];
    if let Some(number) = x_display {
        variables.push(format!("DISPLAY=:{number}"));
    }
    let variables: Vec<&str> = variables.iter().map(String::as_str).collect();
    run("dbus-update-activation-environment", &variables);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut shell = default_shell();
    let mut shell_args: Vec<String> = Vec::new();
    let mut screenshot = None;
    let mut backend = None;
    let mut split = false;
    let mut session = false;
    let mut xwayland = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--shell" => match args.next() {
                Some(path) => shell = PathBuf::from(path),
                None => usage_error("--shell needs a value"),
            },
            "--backend" => match args.next().as_deref() {
                Some("winit") => backend = Some(false),
                Some("drm") => backend = Some(true),
                _ => usage_error("--backend needs 'winit' or 'drm'"),
            },
            "--split" => split = true,
            "--session" => session = true,
            "--no-xwayland" => xwayland = false,
            "--screenshot" => match args.next() {
                Some(path) => screenshot = Some(PathBuf::from(path)),
                None => usage_error("--screenshot needs a value"),
            },
            "--" => {
                shell_args.extend(args.by_ref());
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => usage_error(&format!("unknown option '{other}'")),
        }
    }

    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let mut event_loop: EventLoop<'static, EdexComp> = EventLoop::try_new()?;

    let display: Display<EdexComp> = Display::new()?;
    let mut data = EdexComp::new(&mut event_loop, display);
    data.screenshot = screenshot;
    data.split = split;

    // Inside another desktop there is a display server to connect to; on a bare
    // virtual terminal there is not, and the hardware is ours to drive.
    let nested = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    if backend.unwrap_or(!nested) {
        crate::udev::init_udev(&mut event_loop, &mut data)?;
    } else {
        crate::winit::init_winit(&mut event_loop, &mut data)?;
    }

    data.reload_input();
    if xwayland {
        data.start_xwayland();
    }
    if session {
        announce_session(&data.socket_name, data.x_display);
    }

    // The shell learns about open applications over this socket.
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let ipc_path = runtime_dir.join(format!(
        "edex-comp-{}.sock",
        data.socket_name.to_string_lossy()
    ));
    match ipc::IpcServer::bind(ipc_path) {
        Ok(server) => data.ipc = Some(server),
        Err(e) => tracing::warn!("no desktop socket, the shell will not list applications: {e}"),
    }

    data.shell_spec = Some(state::ShellSpec {
        program: shell,
        args: shell_args,
    });
    data.spawn_shell()?;

    event_loop.run(None, &mut data, move |_| {})?;

    // Do not leave the shell running without a compositor.
    if let Some(mut child) = data.shell_child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    data.ipc = None;
    if let Some(reason) = data.fatal.take() {
        return Err(reason.into());
    }
    Ok(())
}
