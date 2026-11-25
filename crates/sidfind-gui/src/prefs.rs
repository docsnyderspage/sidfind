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

/// A setting a scan cannot do without.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    Hvsc,
    Index,
}

impl Prefs {
    /// The HVSC folder is required for every scan, so without it the app is not set up yet.
    pub fn needs_setup(&self) -> bool {
        self.hvsc.is_none()
    }

    /// What a scan in the current mode still lacks, if anything: the HVSC folder always, and
    /// in indexed mode also the index file.
    pub fn missing(&self) -> Option<Missing> {
        if self.hvsc.is_none() {
            Some(Missing::Hvsc)
        } else if !self.on_the_fly && self.index.is_none() {
            Some(Missing::Index)
        } else {
            None
        }
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
    fn setup_is_needed_until_the_hvsc_folder_is_set() {
        assert!(Prefs::default().needs_setup());
        assert!(Prefs { index: p("/i"), ..Prefs::default() }.needs_setup(), "an index alone is not enough");
        assert!(!Prefs { hvsc: p("/h"), ..Prefs::default() }.needs_setup());
    }

    #[test]
    fn a_scan_always_needs_the_hvsc_folder_and_in_indexed_mode_the_index_too() {
        let (hvsc, index) = (p("/h"), p("/i"));
        // indexed mode (on_the_fly = false)
        assert_eq!(Prefs { index: index.clone(), ..Prefs::default() }.missing(), Some(Missing::Hvsc), "index without HVSC");
        assert_eq!(Prefs { hvsc: hvsc.clone(), ..Prefs::default() }.missing(), Some(Missing::Index));
        assert_eq!(Prefs { hvsc: hvsc.clone(), index: index.clone(), on_the_fly: false }.missing(), None);
        // on-the-fly mode needs no index
        assert_eq!(Prefs { hvsc: hvsc.clone(), index: None, on_the_fly: true }.missing(), None);
        assert_eq!(Prefs { hvsc: None, index, on_the_fly: true }.missing(), Some(Missing::Hvsc));
    }
}
