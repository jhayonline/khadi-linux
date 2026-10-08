//! The terminal's other half.
//!
//! eDEX ran node-pty behind xterm.js. This is the same arrangement with the
//! same division of labour: the webview owns the screen, this owns the
//! process. A reader thread per session pumps bytes to the frontend as Tauri
//! events, because a pty is a stream and polling one over IPC would add
//! latency to every keystroke.

use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::Mutex,
};

use anyhow::{anyhow, Result};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tauri::{AppHandle, Emitter};

pub struct Session {
    writer: Box<dyn Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}

#[derive(Default)]
pub struct Ptys(pub Mutex<HashMap<String, Session>>);

/// The login shell, or a sane fallback. eDEX read $SHELL too, and a shell that
/// does not exist is better caught here than as a blank pane.
fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty() && std::path::Path::new(s).exists())
        .unwrap_or_else(|| "/bin/sh".into())
}

pub fn spawn(app: &AppHandle, state: &Ptys, id: &str, cols: u16, rows: u16) -> Result<()> {
    let sys = NativePtySystem::default();
    let pair = sys.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

    let mut cmd = CommandBuilder::new(login_shell());
    cmd.arg("-l");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.cwd(home);
    }
    // xterm.js speaks xterm-256color. Claiming anything else makes ncurses
    // programs draw the wrong thing, which is the sort of bug that gets
    // blamed on the font.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;

    let handle = app.clone();
    let out_event = format!("pty:{id}:data");
    let exit_event = format!("pty:{id}:exit");
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Lossy on purpose: a pty carries bytes, and a partial
                    // UTF-8 sequence split across two reads must not kill the
                    // session. xterm.js reassembles what it is given.
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = handle.emit(&out_event, s);
                }
            }
        }
        let _ = child.wait();
        let _ = handle.emit(&exit_event, ());
    });

    state
        .0
        .lock()
        .map_err(|_| anyhow!("pty registry poisoned"))?
        .insert(id.to_string(), Session { writer, master: pair.master });
    Ok(())
}

pub fn write(state: &Ptys, id: &str, data: &str) -> Result<()> {
    let mut map = state.0.lock().map_err(|_| anyhow!("pty registry poisoned"))?;
    let s = map.get_mut(id).ok_or_else(|| anyhow!("no pty {id}"))?;
    s.writer.write_all(data.as_bytes())?;
    s.writer.flush()?;
    Ok(())
}

pub fn resize(state: &Ptys, id: &str, cols: u16, rows: u16) -> Result<()> {
    let map = state.0.lock().map_err(|_| anyhow!("pty registry poisoned"))?;
    let s = map.get(id).ok_or_else(|| anyhow!("no pty {id}"))?;
    s.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
    Ok(())
}

pub fn kill(state: &Ptys, id: &str) -> Result<()> {
    state
        .0
        .lock()
        .map_err(|_| anyhow!("pty registry poisoned"))?
        .remove(id);
    Ok(())
}
