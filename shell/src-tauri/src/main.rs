// Prevents an extra console window on Windows in release. Khadi is Linux-only,
// but the attribute is free and the Tauri template is the shape people expect.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    khadi_shell_lib::run()
}
