//! SidFind desktop GUI: native widgets (wxWidgets) over `sidfind-core`.

// no console window on Windows
#![cfg_attr(windows, windows_subsystem = "windows")]

mod job;
mod prefs;
mod results;
mod settings_window;
mod theme;
mod ui;
mod widgets;

fn main() {
    let _ = wxdragon::main(|app| {
        // the name decides where the settings and the per-user data live
        app.set_app_name("SidFind");
        // the theme can only be set reliably before the first window exists
        let theme = theme::startup_theme();
        theme::apply_at_startup(theme);
        ui::build(theme)
    });
}
