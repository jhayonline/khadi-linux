// Prevents an extra console window on Windows in release. Khadi is Linux-only,
// but the attribute is free and the Tauri template is the shape people expect.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // --selftest asks the binary whether it can read the machine, with no
    // compositor and no window. It is how the gate verifies the panels now
    // that they live behind a webview instead of in a pipe.
    if std::env::args().any(|a| a == "--selftest") {
        std::process::exit(khadi_shell_lib::selftest());
    }
    // The login screen is the same binary and the same stylesheet, with a
    // different window and a command list short enough to read in one go —
    // the desktop's includes pty_spawn, and a pre-auth shell as the `greeter`
    // user is not a thing to leave one IPC call away.
    if std::env::args().any(|a| a == "--greeter") {
        // THE GREETER SETS ITS OWN ENVIRONMENT, rather than making
        // /etc/greetd/config.toml wrap it in `sh -c`. That wrapper worked and
        // khadi-check was right to object to it: "what the config launches"
        // then reported `sh`, which is true and useless. A binary that needs
        // two variables set should set them.
        //
        // HOME: Arch gives the `greeter` user `/` as its home, root-owned and
        // read-only. The VT greeter never wrote anything; a webview wants a
        // data directory, and Tauri dies with "Failed to setup app:
        // Permission denied (os error 13)" with cage already running happily
        // behind it. XDG_RUNTIME_DIR is writable and dies with the session,
        // which is the right lifetime for a login screen's state.
        if let Ok(run) = std::env::var("XDG_RUNTIME_DIR") {
            std::env::set_var("HOME", run);
        }
        // Software compositing, only here. It is the most expensive thing that
        // was ever wrong with the desktop, so it is off there — but the
        // greeter is a static screen, so it costs nothing, and the failure it
        // prevents is a BLANK LOGIN SCREEN on a machine with no working
        // acceleration. Being locked out is worse than a clock composited on
        // the CPU.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        return khadi_shell_lib::run_greeter();
    }
    khadi_shell_lib::run()
}
