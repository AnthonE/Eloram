//! Eloram on the desktop: `el_client::desktop`.

// A windowed program on Windows, with no console behind it (the debug build keeps one, for logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    el_client::desktop::main()
}
