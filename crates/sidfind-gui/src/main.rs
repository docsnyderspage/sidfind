//! SidFind desktop GUI: native widgets (wxWidgets) over `sidfind-core`.

// no console window on Windows
#![cfg_attr(windows, windows_subsystem = "windows")]

mod job;
mod ui;

fn main() {
    let _ = wxdragon::main(|app| ui::build(&app));
}
