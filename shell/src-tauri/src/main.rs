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
    khadi_shell_lib::run()
}
