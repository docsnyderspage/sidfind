//! The settings that outlive a run: HVSC folder, index file and scan mode. Stored through
//! wxWidgets' config (the registry on Windows, a preferences file elsewhere).

use std::path::PathBuf;

use wxdragon::prelude::Config;

const HVSC: &str = "hvsc";
const INDEX: &str = "index";
const MODE: &str = "on_the_fly";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prefs {
    pub hvsc: Option<PathBuf>,
    pub index: Option<PathBuf>,
    /// Scan mode: true = on-the-fly (read HVSC), false = through the index.
    pub on_the_fly: bool,
}

/// A saved value wins; an empty saved value means "cleared on purpose" (so the environment
/// default must not come back); with nothing saved the environment default applies.
fn resolve(saved: Option<String>, env: Option<PathBuf>) -> Option<PathBuf> {
    match saved {
        Some(s) if s.is_empty() => None,
        Some(s) => Some(PathBuf::from(s)),
        None => env,
    }
}

/// Without a saved choice: scan the HVSC directly only when there is no index to use.
fn default_mode(hvsc: &Option<PathBuf>, index: &Option<PathBuf>) -> bool {
    index.is_none() && hvsc.is_some()
}

impl Prefs {
    /// True when nothing was ever saved and no environment default exists: a first run.
    pub fn is_unconfigured(&self) -> bool {
        self.hvsc.is_none() && self.index.is_none()
    }

    /// Loads the saved settings (the app name must be set first), with `SIDFIND_HVSC` and
    /// `SIDFIND_INDEX` as defaults.
    pub fn load() -> Prefs {
        let env = |name| std::env::var_os(name).map(PathBuf::from);
        let config = Config::get(true);
        let saved = |key: &str| config.as_ref().filter(|c| c.has_entry(key)).map(|c| c.read_string(key, ""));
        let hvsc = resolve(saved(HVSC), env("SIDFIND_HVSC"));
        let index = resolve(saved(INDEX), env("SIDFIND_INDEX"));
        let on_the_fly = match config.as_ref().filter(|c| c.has_entry(MODE)) {
            Some(c) => c.read_long(MODE, 0) != 0,
            None => default_mode(&hvsc, &index),
        };
        Prefs { hvsc, index, on_the_fly }
    }

    pub fn save(&self) {
        let Some(config) = Config::get(true) else { return };
        let text = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        config.write_string(HVSC, &text(&self.hvsc));
        config.write_string(INDEX, &text(&self.index));
        config.write_long(MODE, i64::from(self.on_the_fly));
        config.flush(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<PathBuf> {
        Some(PathBuf::from(s))
    }

    #[test]
    fn saved_value_wins_over_the_environment() {
        assert_eq!(resolve(Some("/saved".into()), p("/env")), p("/saved"));
    }

    #[test]
    fn empty_saved_value_means_cleared_and_keeps_the_environment_out() {
        assert_eq!(resolve(Some(String::new()), p("/env")), None);
    }

    #[test]
    fn nothing_saved_falls_back_to_the_environment() {
        assert_eq!(resolve(None, p("/env")), p("/env"));
        assert_eq!(resolve(None, None), None);
    }

    #[test]
    fn default_mode_reads_hvsc_only_when_there_is_no_index() {
        assert!(default_mode(&p("/hvsc"), &None));
        assert!(!default_mode(&p("/hvsc"), &p("/idx")));
        assert!(!default_mode(&None, &None));
    }

    #[test]
    fn unconfigured_means_neither_path() {
        assert!(Prefs::default().is_unconfigured());
        assert!(!Prefs { hvsc: p("/h"), ..Prefs::default() }.is_unconfigured());
    }
}
