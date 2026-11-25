//! The identification workflow shared by the CLI and the GUI.
//!
//! Nothing here prints or exits. Results come back as values, and long operations report
//! progress, notices and cancellation through [`Control`].

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

use crate::index::{Stored, Tune};
use crate::scan::{count_kinds, fingerprints, Hit, Index, Params};
use crate::{cpu, sid, vsf};

/// Default sampling width of an index: about 170 MB for HVSC, and the widest that still
/// identified all example PRGs correctly. See `scan.rs`.
pub const DEFAULT_INDEX_SAMPLING: usize = 40;
pub const DEFAULT_WINDOW: usize = 12;

/// Message returned when the caller cancelled through [`Control::cancel`].
pub const CANCELLED: &str = "cancelled";

/// Optional hooks for long operations. The default does nothing, so callers that don't care
/// (tests, the CLI's silent paths) can pass `&Control::default()`.
#[derive(Clone, Copy, Default)]
pub struct Control<'a> {
    /// Called as `(done, total)` after each SID file has been processed. May be called from
    /// several threads, in any order.
    pub progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    /// Set to true to stop as soon as possible; the operation then fails with [`CANCELLED`].
    pub cancel: Option<&'a AtomicBool>,
    /// Human readable status lines such as "scanning 58699 SID files...".
    pub notice: Option<&'a (dyn Fn(&str) + Sync)>,
}

impl Control<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }

    fn say(&self, message: impl FnOnce() -> String) {
        if let Some(notice) = self.notice {
            notice(&message());
        }
    }
}

/// The matching options. The defaults are the command line defaults.
#[derive(Clone, Debug)]
pub struct ScanSettings {
    /// Match window in bytes. `None` means the default; an index always has its own.
    pub window: Option<usize>,
    /// Number of candidates to return per file.
    pub top: usize,
    /// Candidates scoring below this are dropped.
    pub min_score: f64,
    /// CPU cycles to emulate so a packed PRG can unpack itself (0 = don't emulate).
    pub cycles: u64,
}

impl Default for ScanSettings {
    fn default() -> Self {
        ScanSettings { window: None, top: 3, min_score: 10.0, cycles: 50_000_000 }
    }
}

/// Where the tunes to compare with come from.
#[derive(Clone, Debug)]
pub enum Source {
    /// Read every `.sid` below this HVSC root (a few seconds per run).
    Hvsc(PathBuf),
    /// A precomputed index made by [`build_index`] (well under a second).
    Index(PathBuf),
}

/// One candidate tune for a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    /// Path of the tune relative to the HVSC root, '/' separated, without a leading slash.
    pub name: String,
    /// Share of the tune's data that was found, 0.0 to 1.0.
    pub share: f64,
    pub score: f64,
}

/// The outcome for one input file.
#[derive(Clone, Debug)]
pub struct Identification {
    pub path: PathBuf,
    /// How the file was prepared, e.g. "emulated 1234 cycles, ...".
    pub emulation: String,
    /// Number of distinct windows that were checked.
    pub windows: usize,
    /// Best candidates first, at most `top`; empty when nothing matched.
    pub matches: Vec<Match>,
}

pub struct IndexSummary {
    pub tunes: usize,
    pub fingerprints: usize,
    pub bytes: u64,
}

fn find_sids(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_sids(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sid")) {
            out.push(path);
        }
    }
}

/// All `.sid` files below `hvsc`, in a stable order.
pub fn list_sids(hvsc: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    find_sids(hvsc, &mut files);
    if files.is_empty() {
        return Err(format!("no .sid files found below {}", hvsc.display()));
    }
    files.sort();
    Ok(files)
}

/// Runs `work(file number)` for every file on all cores, reporting progress and honouring
/// cancellation.
fn for_each_parallel(count: usize, ctl: &Control, work: impl Fn(usize) + Sync) -> Result<(), String> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(4, |n| n.get());
    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    if ctl.cancelled() {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= count {
                        break;
                    }
                    work(i);
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(progress) = ctl.progress {
                        progress(finished, count);
                    }
                }
            });
        }
    });
    if ctl.cancelled() { Err(CANCELLED.into()) } else { Ok(()) }
}

/// The path of a tune as printed and stored: relative to the HVSC root, '/' separated.
fn tune_name(hvsc: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(hvsc).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/").trim_start_matches('/').to_string()
}

struct Candidate {
    name: String,
    /// Number of distinct exact and masked fingerprints the tune has.
    totals: [usize; 2],
    hit: Hit,
}

/// Identifies the tunes in PRG files and VICE snapshots (told apart by their contents).
/// Fails as a whole if any file cannot be read.
pub fn identify(
    files: &[PathBuf],
    source: &Source,
    settings: &ScanSettings,
    ctl: &Control,
) -> Result<Vec<Identification>, String> {
    // Where the tunes come from decides the matching parameters: an index has its own.
    let (hvsc, stored) = match source {
        Source::Index(path) => {
            let file = fs::File::open(path).map_err(|e| format!("cannot open index {}: {e}", path.display()))?;
            let stored = Stored::read(file).map_err(|e| format!("{}: {e}; build it again with --build-index", path.display()))?;
            if settings.window.is_some_and(|w| w != stored.params.window) {
                return Err(format!("the index was built with --window {}", stored.params.window));
            }
            ctl.say(|| format!("using index {} ({} tunes)", path.display(), stored.tunes.len()));
            (None, Some(stored))
        }
        Source::Hvsc(dir) => (Some(dir), None),
    };
    let params = stored
        .as_ref()
        .map_or(Params { window: settings.window.unwrap_or(DEFAULT_WINDOW), sampling: 1 }, |s| s.params);

    let mut index = Index::new(params);
    let mut notes = Vec::new();
    for (id, path) in files.iter().enumerate() {
        let data = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let run;
        let (sources, emulation) = if vsf::is_vsf(&data) {
            // A snapshot already holds the memory of a machine that has run: nothing to unpack.
            let ram = vsf::ram(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            (vec![ram], "VICE snapshot, not emulated".to_string())
        } else {
            let (load, body) = sid::split_prg(&data).ok_or(format!("{} is too short to be a PRG", path.display()))?;
            let mut sources = vec![body];
            let mut emulation = "not emulated".to_string();
            run = (settings.cycles > 0).then(|| cpu::run_prg(load, body, settings.cycles));
            if let Some(run) = &run {
                emulation = format!("emulated {} cycles, {}", run.cycles, run.stop);
                sources.extend(run.chunks.iter().map(|c| &c[..]));
            }
            (sources, emulation)
        };
        notes.push((emulation, index.add(id as u32, &sources)));
    }

    let mut found: Vec<Vec<Candidate>> = files.iter().map(|_| Vec::new()).collect();
    if let Some(stored) = &stored {
        for (tune, hit) in index.scan_stored(stored) {
            let t = &stored.tunes[tune as usize];
            found[hit.query as usize].push(Candidate { name: t.path.clone(), totals: t.counts.map(|c| c as usize), hit });
        }
    } else if let Some(hvsc) = hvsc {
        let sids = list_sids(hvsc)?;
        ctl.say(|| format!("scanning {} SID files...", sids.len()));
        let found_shared = Mutex::new(&mut found);
        for_each_parallel(sids.len(), ctl, |i| {
            let Ok(file) = fs::read(&sids[i]) else { return };
            let Some(payload) = sid::payload(&file) else { return };
            let tune = fingerprints(payload, params);
            for hit in index.scan(&tune) {
                let query = hit.query as usize;
                let cand = Candidate { name: tune_name(hvsc, &sids[i]), totals: count_kinds(&tune), hit };
                found_shared.lock().unwrap()[query].push(cand);
            }
        })?;
    }

    Ok(files
        .iter()
        .zip(found.into_iter().zip(notes))
        .map(|(path, (cands, (emulation, windows)))| Identification {
            path: path.clone(),
            emulation,
            windows,
            matches: rank(&cands, params, settings),
        })
        .collect())
}

/// Scores the candidates for one file and returns the best, best first.
fn rank(cands: &[Candidate], params: Params, settings: &ScanSettings) -> Vec<Match> {
    // A window shared by many tunes (common player code) says little; one shared by a
    // single tune says a lot. Weigh each window by 1/k^2, k being the number of candidates
    // containing it. Squaring matters: hundreds of windows shared by a whole family of tunes
    // (one composer's player and tables) must not add up to beat a few unique ones.
    // The sum is then scaled by the share of the tune that was found: a tune that is really
    // in the PRG is mostly there, while a tune that merely shares some unique bytes is not.
    // A sampled index sees about 1/scale of the windows, hence the factor.
    let mut freq: HashMap<u64, u32> = HashMap::new();
    for c in cands {
        for &w in &c.hit.windows {
            *freq.entry(w).or_default() += 1;
        }
    }
    // The share is taken per kind of window and the better one counts: a relocated tune is
    // found through its masked windows, while its exact windows (which include the rewritten
    // addresses) rightly do not match.
    let share = |c: &Candidate| {
        let found = count_kinds(&c.hit.windows);
        (0..2).filter(|&k| c.totals[k] > 0).map(|k| found[k] as f64 / c.totals[k] as f64).fold(0.0, f64::max)
    };
    let mut scored: Vec<Match> = cands
        .iter()
        .map(|c| {
            let rarity: f64 = c.hit.windows.iter().map(|w| 1.0 / (freq[w] as f64).powi(2)).sum();
            Match { name: c.name.clone(), share: share(c), score: rarity * params.scale() * share(c) }
        })
        .collect();
    scored.retain(|m| m.score >= settings.min_score);
    scored.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
    scored.truncate(settings.top);
    scored
}

/// Builds an index of the HVSC at `hvsc` and writes it to `out`. The file is written next to
/// the target and renamed, so a failed or cancelled build never leaves a broken index.
pub fn build_index(hvsc: &Path, out: &Path, params: Params, ctl: &Control) -> Result<IndexSummary, String> {
    let files = list_sids(hvsc)?;
    ctl.say(|| format!("indexing {} SID files...", files.len()));
    let results: Mutex<Vec<Option<(String, Vec<u64>)>>> = Mutex::new(vec![None; files.len()]);
    for_each_parallel(files.len(), ctl, |i| {
        let Ok(file) = fs::read(&files[i]) else { return };
        let Some(payload) = sid::payload(&file) else { return };
        let fps = fingerprints(payload, params);
        results.lock().unwrap()[i] = Some((tune_name(hvsc, &files[i]), fps));
    })?;
    let mut tunes = Vec::new();
    let mut pairs = Vec::new();
    for (name, fps) in results.into_inner().unwrap().into_iter().flatten() {
        let no = tunes.len() as u32;
        tunes.push(Tune { path: name, counts: count_kinds(&fps).map(|c| c as u32) });
        pairs.extend(fps.into_iter().map(|fp| (no, fp)));
    }
    if tunes.len() > Stored::MAX_TUNES {
        return Err(format!("too many tunes to index ({})", tunes.len()));
    }
    let stored = Stored::new(params, tunes, pairs);

    let mut tmp = out.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let write = || -> std::io::Result<()> {
        stored.write(std::io::BufWriter::new(fs::File::create(&tmp)?))?;
        fs::rename(&tmp, out)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot write {}: {e}", out.display())
    })?;
    Ok(IndexSummary {
        tunes: stored.tunes.len(),
        fingerprints: stored.entries(),
        bytes: fs::metadata(out).map_or(0, |m| m.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                (s >> 24) as u8
            })
            .collect()
    }

    /// A minimal PSID file whose payload is `body` (header load address 0x1000, so no bytes skipped).
    fn fake_sid(body: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 0x76];
        f[..4].copy_from_slice(b"PSID");
        f[6..8].copy_from_slice(&0x76u16.to_be_bytes());
        f[8..10].copy_from_slice(&0x1000u16.to_be_bytes());
        f.extend_from_slice(body);
        f
    }

    /// A throwaway directory that is removed again when dropped.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("sidfind-test-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// An HVSC with three unrelated tunes, and a PRG that contains the second one.
    fn fixture(tag: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new(tag);
        for (i, seed) in [1u32, 2, 3].iter().enumerate() {
            let sub = dir.0.join(format!("MUSICIANS/{i}"));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join(format!("tune{i}.sid")), fake_sid(&noise(3000, *seed))).unwrap();
        }
        let mut prg = vec![0x00, 0x10];
        prg.extend(noise(3000, 2));
        let prg_path = dir.0.join("demo.prg");
        fs::write(&prg_path, prg).unwrap();
        (dir, prg_path)
    }

    fn settings() -> ScanSettings {
        ScanSettings { cycles: 0, min_score: 1.0, ..ScanSettings::default() }
    }

    fn candidate(name: &str, totals: [usize; 2], windows: &[u64]) -> Candidate {
        Candidate { name: name.into(), totals, hit: Hit { query: 0, windows: windows.to_vec() } }
    }

    const EXACT: Params = Params { window: 12, sampling: 1 };

    #[test]
    fn rank_prefers_unique_windows_and_full_share() {
        // even fingerprints are exact windows. `a` owns four windows no one else has;
        // `b` and `c` only share two windows, which are worth 1/2^2 each.
        let cands = [
            candidate("b", [10, 0], &[10, 12]),
            candidate("a", [4, 0], &[2, 4, 6, 8]),
            candidate("c", [10, 0], &[10, 12]),
        ];
        let cfg = ScanSettings { min_score: 0.0, top: 10, ..ScanSettings::default() };
        let ranked = rank(&cands, EXACT, &cfg);
        assert_eq!(ranked.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(ranked[0].share, 1.0);
        assert_eq!(ranked[0].score, 4.0);
        assert!((ranked[1].share - 0.2).abs() < 1e-12);
        assert!((ranked[1].score - 0.1).abs() < 1e-12); // 2 windows * 0.25 * share 0.2
        // equal scores are ordered by name
        assert!(ranked[1].name < ranked[2].name);
    }

    #[test]
    fn rank_applies_min_score_and_top() {
        let cands = [candidate("a", [4, 0], &[2, 4, 6, 8]), candidate("b", [4, 0], &[10, 12])];
        let strict = ScanSettings { min_score: 3.0, ..ScanSettings::default() };
        assert_eq!(rank(&cands, EXACT, &strict).len(), 1);
        let one = ScanSettings { min_score: 0.0, top: 1, ..ScanSettings::default() };
        assert_eq!(rank(&cands, EXACT, &one).len(), 1);
        assert!(rank(&[], EXACT, &one).is_empty());
    }

    #[test]
    fn identify_on_the_fly_finds_the_tune() {
        let (dir, prg) = fixture("fly");
        let found = identify(&[prg.clone()], &Source::Hvsc(dir.0.clone()), &settings(), &Control::default()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, prg);
        assert_eq!(found[0].emulation, "not emulated");
        let best = &found[0].matches[0];
        assert_eq!(best.name, "MUSICIANS/1/tune1.sid");
        assert!(best.share > 0.99, "share {}", best.share);
        assert_eq!(found[0].matches.len(), 1, "unrelated tunes must not match");
    }

    #[test]
    fn identify_through_an_index_gives_the_same_answer() {
        let (dir, prg) = fixture("idx");
        let idx = dir.0.join("hvsc.idx");
        let summary = build_index(&dir.0, &idx, EXACT, &Control::default()).unwrap();
        assert_eq!(summary.tunes, 3);
        assert!(summary.bytes > 0 && summary.fingerprints > 0);
        assert!(!dir.0.join("hvsc.idx.tmp").exists(), "temporary file must be renamed away");
        let found = identify(&[prg], &Source::Index(idx), &settings(), &Control::default()).unwrap();
        assert_eq!(found[0].matches[0].name, "MUSICIANS/1/tune1.sid");
        assert!(found[0].matches[0].share > 0.99);
    }

    #[test]
    fn identify_reports_no_match_for_unrelated_data() {
        let (dir, _) = fixture("none");
        let other = dir.0.join("other.prg");
        let mut prg = vec![0x00, 0x10];
        prg.extend(noise(3000, 99));
        fs::write(&other, prg).unwrap();
        let found = identify(&[other], &Source::Hvsc(dir.0.clone()), &settings(), &Control::default()).unwrap();
        assert!(found[0].matches.is_empty());
        assert!(found[0].windows > 0);
    }

    #[test]
    fn errors_are_reported_not_printed() {
        let (dir, prg) = fixture("err");
        let s = settings();
        let none = Control::default();
        let missing = identify(&[dir.0.join("nope.prg")], &Source::Hvsc(dir.0.clone()), &s, &none);
        assert!(missing.unwrap_err().starts_with("cannot read"));
        let short = dir.0.join("short.prg");
        fs::write(&short, [1u8]).unwrap();
        assert!(identify(&[short], &Source::Hvsc(dir.0.clone()), &s, &none).unwrap_err().contains("too short"));
        let empty = TempDir::new("empty");
        assert!(identify(&[prg.clone()], &Source::Hvsc(empty.0.clone()), &s, &none).unwrap_err().contains("no .sid files"));
        let bad_index = dir.0.join("bad.idx");
        fs::write(&bad_index, b"garbage").unwrap();
        assert!(identify(&[prg], &Source::Index(bad_index), &s, &none).unwrap_err().contains("build it again"));
    }

    #[test]
    fn index_window_must_match_the_requested_window() {
        let (dir, prg) = fixture("win");
        let idx = dir.0.join("hvsc.idx");
        build_index(&dir.0, &idx, EXACT, &Control::default()).unwrap();
        let s = ScanSettings { window: Some(8), ..settings() };
        let err = identify(&[prg], &Source::Index(idx), &s, &Control::default()).unwrap_err();
        assert!(err.contains("--window 12"), "{err}");
    }

    #[test]
    fn progress_notice_and_cancel() {
        let (dir, _) = fixture("ctl");
        let idx = dir.0.join("hvsc.idx");

        let last = AtomicUsize::new(0);
        let total = AtomicUsize::new(0);
        let notes = Mutex::new(Vec::new());
        let progress = |done: usize, of: usize| {
            last.fetch_max(done, Ordering::Relaxed);
            total.store(of, Ordering::Relaxed);
        };
        let notice = |m: &str| notes.lock().unwrap().push(m.to_string());
        let ctl = Control { progress: Some(&progress), notice: Some(&notice), cancel: None };
        build_index(&dir.0, &idx, EXACT, &ctl).unwrap();
        assert_eq!((last.load(Ordering::Relaxed), total.load(Ordering::Relaxed)), (3, 3));
        assert_eq!(notes.into_inner().unwrap(), ["indexing 3 SID files..."]);

        let cancel = AtomicBool::new(true);
        let ctl = Control { cancel: Some(&cancel), ..Control::default() };
        let out = dir.0.join("cancelled.idx");
        assert_eq!(build_index(&dir.0, &out, EXACT, &ctl).err().as_deref(), Some(CANCELLED));
        assert!(!out.exists() && !dir.0.join("cancelled.idx.tmp").exists(), "a cancelled build writes nothing");
    }
}
