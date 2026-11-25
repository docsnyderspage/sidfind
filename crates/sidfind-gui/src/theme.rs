//! The user's theme choice: remembered between runs and applied through wxWidgets.

use wxdragon::appearance::{Appearance, AppearanceResult};
use wxdragon::prelude::Config;

const KEY: &str = "theme";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// What happened when a theme was applied while the app is running.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    /// Visible right away.
    Now,
    /// Saved, but only takes effect the next time the app starts (Windows).
    OnRestart,
    /// The platform refused, e.g. GTK with a fixed `GTK_THEME`.
    Unsupported,
}

impl Theme {
    fn code(self) -> i64 {
        match self {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        }
    }

    fn from_code(code: i64) -> Theme {
        match code {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => Theme::System,
        }
    }

    fn appearance(self) -> Appearance {
        match self {
            Theme::System => Appearance::System,
            Theme::Light => Appearance::Light,
            Theme::Dark => Appearance::Dark,
        }
    }
}

/// The saved choice (System when nothing was saved yet). The app name must be set first.
pub fn load() -> Theme {
    Config::get(true).map_or(Theme::System, |c| Theme::from_code(c.read_long(KEY, 0)))
}

pub fn save(theme: Theme) {
    if let Some(config) = Config::get(true) {
        config.write_long(KEY, theme.code());
        config.flush(false);
    }
}

/// The theme for this run: `SIDFIND_THEME=light|dark|system` (for previews) wins over the
/// saved choice.
pub fn startup_theme() -> Theme {
    match std::env::var("SIDFIND_THEME").as_deref() {
        Ok("light") => Theme::Light,
        Ok("dark") => Theme::Dark,
        Ok("system") => Theme::System,
        _ => load(),
    }
}

/// Applies the theme before the first window exists.
pub fn apply_at_startup(theme: Theme) {
    // macOS and GTK follow the system by themselves, and on macOS an explicit "System" would
    // pin the appearance of the moment and stop following later changes. Windows only follows
    // the system when asked.
    if theme == Theme::System && !cfg!(windows) {
        return;
    }
    wxdragon::app::set_appearance(theme.appearance());
}

/// Applies the theme while the app runs.
pub fn apply_now(theme: Theme) -> Applied {
    match wxdragon::app::set_appearance(theme.appearance()) {
        AppearanceResult::Ok => Applied::Now,
        AppearanceResult::CannotChange => Applied::OnRestart,
        AppearanceResult::Failure => Applied::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_unknown_means_system() {
        for t in [Theme::System, Theme::Light, Theme::Dark] {
            assert_eq!(Theme::from_code(t.code()), t);
        }
        assert_eq!(Theme::from_code(99), Theme::System);
        assert_eq!(Theme::default(), Theme::System);
    }
}
