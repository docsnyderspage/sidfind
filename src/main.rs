mod cpu;
mod index;
mod opcodes;
mod scan;
mod sid;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

use index::{Stored, Tune};
use scan::{count_kinds, fingerprints, Hit, Index, Params};

const USAGE: &str = "\
sidfind - find which HVSC tune is used in a C64 .prg file

USAGE:
    sidfind [OPTIONS] <PRG>...          identify the tunes in PRG files
    sidfind --build-index <FILE> --hvsc <DIR>
                                        precompute an index of HVSC (optional, see below)

OPTIONS:
    --hvsc <DIR>     Path to the HVSC root (the folder holding DEMOS, GAMES, MUSICIANS).
                     Defaults to the SIDFIND_HVSC environment variable. Reads HVSC on every
                     run, which takes a few seconds and needs no setup.
    --index <FILE>   Use a precomputed index instead of reading HVSC (much faster). It is
                     created with --build-index. Defaults to the SIDFIND_INDEX environment
                     variable, but --hvsc on the command line takes precedence over that.
    --build-index <FILE>
                     Write an index of the HVSC given by --hvsc to FILE and exit. Takes
                     about as long as one on-the-fly run; build it again after updating
                     HVSC. An index is a sample of HVSC's fingerprints: queries run in
                     well under a second, but a PRG that shares little with a tune can be
                     ranked slightly differently than on the fly (rerun with --hvsc if a
                     result looks doubtful).
    --sampling <N>   With --build-index: how sparse the sample is (default 40, about
                     170 MB). Smaller means a larger and more accurate index: 64 is about
                     105 MB, 32 about 210 MB, 16 about 400 MB. A tune sharing at least
                     window + N - 1 bytes with the PRG is found (unless those bytes are
                     mostly zero runs or fill, which are ignored).
    --window <N>     Match window in bytes (default 12). Smaller finds more modified
                     tunes but produces more false positives. An index has its own.
    --top <N>        Number of candidates to show per PRG (default 3).
    --cycles <N>     CPU cycles to emulate so a packed PRG can unpack itself before
                     matching (default 50000000, about 50 s of C64 time; 0 = don't
                     emulate and only match the file as it is).
    --min-score <N>  Ignore candidates whose score is below N (default 10). The score is
                     the number of PRG windows found in that tune, each weighted by 1/k^2
                     where k is the number of tunes containing it (so shared player code
                     and note tables count for almost nothing), multiplied by the share
                     of the tune's data that was found.
    -h, --help       Show this help.
";

/// Default sampling width of an index: about 170 MB for HVSC, and the widest that still
/// identified all example PRGs correctly. See `scan.rs`.
const DEFAULT_INDEX_SAMPLING: usize = 40;
const DEFAULT_WINDOW: usize = 12;

struct Options {
    hvsc: Option<PathBuf>,
    index: Option<PathBuf>,
    build_index: Option<PathBuf>,
    window: Option<usize>,
    sampling: Option<usize>,
    top: usize,
    min_score: f64,
    cycles: u64,
    prgs: Vec<PathBuf>,
}

enum Source {
    Hvsc(PathBuf),
    Index(PathBuf),
}

impl Options {
    fn source(&self) -> Result<Source, String> {
        let env = |name| std::env::var_os(name).map(PathBuf::from);
        if let Some(index) = &self.index {
            Ok(Source::Index(index.clone()))
        } else if let Some(hvsc) = &self.hvsc {
            Ok(Source::Hvsc(hvsc.clone()))
        } else if let Some(index) = env("SIDFIND_INDEX") {
            Ok(Source::Index(index))
        } else if let Some(hvsc) = env("SIDFIND_HVSC") {
            Ok(Source::Hvsc(hvsc))
        } else {
            Err("no HVSC location: use --hvsc <DIR> or --index <FILE> (or set SIDFIND_HVSC / SIDFIND_INDEX)".into())
        }
    }
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut o = Options {
        hvsc: None,
        index: None,
        build_index: None,
        window: None,
        sampling: None,
        top: 3,
        min_score: 10.0,
        cycles: 50_000_000,
        prgs: Vec::new(),
    };
    let mut args = std::env::args_os().skip(1);
    fn value(name: &str, v: Option<OsString>, what: &str) -> Result<OsString, String> {
        v.ok_or(format!("{name} needs {what}"))
    }
    fn number(name: &str, v: Option<OsString>) -> Result<usize, String> {
        value(name, v, "a positive number")?
            .to_str()
            .and_then(|v| v.parse().ok())
            .filter(|&n| n > 0)
            .ok_or(format!("{name} needs a positive number"))
    }
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--hvsc") => o.hvsc = Some(value("--hvsc", args.next(), "a directory")?.into()),
            Some("--index") => o.index = Some(value("--index", args.next(), "a file")?.into()),
            Some("--build-index") => o.build_index = Some(value("--build-index", args.next(), "a file")?.into()),
            Some("--window") => o.window = Some(number("--window", args.next())?),
            Some("--sampling") => o.sampling = Some(number("--sampling", args.next())?),
            Some("--top") => o.top = number("--top", args.next())?,
            Some("--cycles") => {
                o.cycles = value("--cycles", args.next(), "a number")?
                    .to_str()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--cycles needs a number")?
            }
            Some("--min-score") => o.min_score = number("--min-score", args.next())? as f64,
            Some(s) if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => o.prgs.push(arg.into()),
        }
    }
    if o.sampling.is_some() && o.build_index.is_none() {
        return Err("--sampling only applies to --build-index".into());
    }
    if o.build_index.is_some() {
        if o.hvsc.is_none() && std::env::var_os("SIDFIND_HVSC").is_none() {
            return Err("--build-index needs the HVSC to index: use --hvsc <DIR>".into());
        }
        if o.index.is_some() || !o.prgs.is_empty() {
            return Err("--build-index only builds the index; run sidfind again to use it".into());
        }
    } else if o.prgs.is_empty() {
        return Err("no PRG files given".into());
    }
    Ok(Some(o))
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
fn list_sids(hvsc: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    find_sids(hvsc, &mut files);
    if files.is_empty() {
        return Err(format!("no .sid files found below {}", hvsc.display()));
    }
    files.sort();
    Ok(files)
}

/// Runs `work(file number)` for every file on all cores.
fn for_each_parallel(count: usize, work: impl Fn(usize) + Sync) {
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(4, |n| n.get());
    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= count {
                        break;
                    }
                    work(i);
                }
            });
        }
    });
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

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(Some(o)) => o,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(opts: &Options) -> Result<(), String> {
    if let Some(out) = &opts.build_index {
        let hvsc = opts.hvsc.clone().or_else(|| std::env::var_os("SIDFIND_HVSC").map(PathBuf::from));
        let params = Params { window: opts.window.unwrap_or(DEFAULT_WINDOW), sampling: opts.sampling.unwrap_or(DEFAULT_INDEX_SAMPLING) };
        return build_index(&hvsc.unwrap(), out, params);
    }

    // Where the tunes come from decides the matching parameters: an index has its own.
    let (source, stored) = match opts.source()? {
        Source::Index(path) => {
            let file = fs::File::open(&path).map_err(|e| format!("cannot open index {}: {e}", path.display()))?;
            let stored = Stored::read(file).map_err(|e| format!("{}: {e}; build it again with --build-index", path.display()))?;
            if opts.window.is_some_and(|w| w != stored.params.window) {
                return Err(format!("the index was built with --window {}", stored.params.window));
            }
            eprintln!("using index {} ({} tunes)", path.display(), stored.tunes.len());
            (None, Some(stored))
        }
        Source::Hvsc(dir) => (Some(dir), None),
    };
    let params = stored
        .as_ref()
        .map_or(Params { window: opts.window.unwrap_or(DEFAULT_WINDOW), sampling: 1 }, |s| s.params);

    let mut index = Index::new(params);
    let mut notes = Vec::new();
    for (id, path) in opts.prgs.iter().enumerate() {
        let data = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let (load, body) = sid::split_prg(&data).ok_or(format!("{} is too short to be a PRG", path.display()))?;
        let mut sources = vec![body];
        let mut emulation = "not emulated".to_string();
        let run = (opts.cycles > 0).then(|| cpu::run_prg(load, body, opts.cycles));
        if let Some(run) = &run {
            emulation = format!("emulated {} cycles, {}", run.cycles, run.stop);
            sources.extend(run.chunks.iter().map(|c| &c[..]));
        }
        notes.push((emulation, index.add(id as u32, &sources)));
    }

    let mut found: Vec<Vec<Candidate>> = opts.prgs.iter().map(|_| Vec::new()).collect();
    if let Some(stored) = &stored {
        for (tune, hit) in index.scan_stored(stored) {
            let t = &stored.tunes[tune as usize];
            found[hit.query as usize].push(Candidate { name: t.path.clone(), totals: t.counts.map(|c| c as usize), hit });
        }
    } else if let Some(hvsc) = &source {
        let files = list_sids(hvsc)?;
        eprintln!("scanning {} SID files...", files.len());
        let found_shared = Mutex::new(&mut found);
        for_each_parallel(files.len(), |i| {
            let Ok(file) = fs::read(&files[i]) else { return };
            let Some(payload) = sid::payload(&file) else { return };
            let tune = fingerprints(payload, params);
            for hit in index.scan(&tune) {
                let query = hit.query as usize;
                let cand = Candidate { name: tune_name(hvsc, &files[i]), totals: count_kinds(&tune), hit };
                found_shared.lock().unwrap()[query].push(cand);
            }
        });
    }

    for (prg, (cands, note)) in opts.prgs.iter().zip(found.into_iter().zip(&notes)) {
        report(opts, params, prg, note, cands);
    }
    Ok(())
}

fn build_index(hvsc: &Path, out: &Path, params: Params) -> Result<(), String> {
    let files = list_sids(hvsc)?;
    eprintln!("indexing {} SID files...", files.len());
    let results: Mutex<Vec<Option<(String, Vec<u64>)>>> = Mutex::new(vec![None; files.len()]);
    for_each_parallel(files.len(), |i| {
        let Ok(file) = fs::read(&files[i]) else { return };
        let Some(payload) = sid::payload(&file) else { return };
        let fps = fingerprints(payload, params);
        results.lock().unwrap()[i] = Some((tune_name(hvsc, &files[i]), fps));
    });
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

    // written next to the target and renamed, so a failed build never leaves a broken index
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
    let size = fs::metadata(out).map_or(0, |m| m.len());
    eprintln!(
        "wrote {}: {} tunes, {} fingerprints, {:.0} MB",
        out.display(),
        stored.tunes.len(),
        stored.entries(),
        size as f64 / 1e6
    );
    Ok(())
}

fn report(opts: &Options, params: Params, prg: &Path, (emulation, windows): &(String, usize), cands: Vec<Candidate>) {
    println!("{}", prg.display());
    // A window shared by many tunes (common player code) says little; one shared by a
    // single tune says a lot. Weigh each window by 1/k^2, k being the number of candidates
    // containing it. Squaring matters: hundreds of windows shared by a whole family of tunes
    // (one composer's player and tables) must not add up to beat a few unique ones.
    // The sum is then scaled by the share of the tune that was found: a tune that is really
    // in the PRG is mostly there, while a tune that merely shares some unique bytes is not.
    // A sampled index sees about 1/scale of the windows, hence the factor.
    let mut freq: HashMap<u64, u32> = HashMap::new();
    for c in &cands {
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
    let mut scored: Vec<(f64, &Candidate)> = cands
        .iter()
        .map(|c| {
            let rarity: f64 = c.hit.windows.iter().map(|w| 1.0 / (freq[w] as f64).powi(2)).sum();
            (rarity * params.scale() * share(c), c)
        })
        .collect();
    scored.retain(|(score, _)| *score >= opts.min_score);
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    if scored.is_empty() {
        println!("   no match ({windows} distinct windows checked; {emulation})");
        println!("   the tune may be packed, relocated or not in HVSC (see --min-score, --window)");
        return;
    }
    for (score, c) in scored.iter().take(opts.top) {
        println!("   {:>5.1}% of tune data found, score {:>7.1}  /{}", 100.0 * share(c), score, c.name);
    }
}
