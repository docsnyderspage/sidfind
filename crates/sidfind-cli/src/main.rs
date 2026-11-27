use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use sidfind_core::scan::Params;
use sidfind_core::{build_index, identify, Control, Identification, ScanSettings, Source, DEFAULT_INDEX_SAMPLING, DEFAULT_WINDOW};

const USAGE: &str = "\
sidfind - find which HVSC tune is used in a C64 .prg file or VICE snapshot (.vsf)

USAGE:
    sidfind [OPTIONS] <FILE>...         identify the tunes in PRG files and VICE snapshots
                                        (a snapshot is recognised by its contents)
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
                     emulate and only match the file as it is). Not used for snapshots,
                     whose memory is used as it is.
    --min-score <N>  Ignore candidates whose score is below N (default 10). The score is
                     the number of PRG windows found in that tune, each weighted by 1/k^2
                     where k is the number of tunes containing it (so shared player code
                     and note tables count for almost nothing), multiplied by the share
                     of the tune's data that was found.
    -V, --version    Show the version.
    -h, --help       Show this help.
";

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

enum Parsed {
    Run(Options),
    Help,
    Version,
}

fn parse_args() -> Result<Parsed, String> {
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
            Some("-h" | "--help") => return Ok(Parsed::Help),
            Some("-V" | "--version") => return Ok(Parsed::Version),
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
        return Err("no PRG or VSF files given".into());
    }
    Ok(Parsed::Run(o))
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(Parsed::Run(o)) => o,
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Version) => {
            println!("sidfind {}", env!("CARGO_PKG_VERSION"));
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
    // status lines go to stderr, results to stdout
    let notice = |m: &str| eprintln!("{m}");
    let ctl = Control { notice: Some(&notice), ..Control::default() };

    if let Some(out) = &opts.build_index {
        let hvsc = opts.hvsc.clone().or_else(|| std::env::var_os("SIDFIND_HVSC").map(PathBuf::from));
        let params = Params { window: opts.window.unwrap_or(DEFAULT_WINDOW), sampling: opts.sampling.unwrap_or(DEFAULT_INDEX_SAMPLING) };
        let summary = build_index(&hvsc.unwrap(), out, params, &ctl)?;
        eprintln!(
            "wrote {}: {} tunes, {} fingerprints, {:.0} MB",
            out.display(),
            summary.tunes,
            summary.fingerprints,
            summary.bytes as f64 / 1e6
        );
        return Ok(());
    }

    let settings = ScanSettings { window: opts.window, top: opts.top, min_score: opts.min_score, cycles: opts.cycles };
    println!("SIDFind v{}", env!("CARGO_PKG_VERSION"));
    for found in identify(&opts.prgs, &opts.source()?, &settings, &ctl)? {
        report(&found);
    }
    Ok(())
}

fn report(found: &Identification) {
    println!("Scanning file {}", found.path.display());
    if found.matches.is_empty() {
        println!("   no match ({} distinct windows checked; {})", found.windows, found.emulation);
        println!("   the tune may be packed, relocated or not in HVSC (see --min-score, --window)");
        return;
    }
    for m in &found.matches {
        println!("   {:>5.1}% of tune data found, score {:>7.1}  /{}", 100.0 * m.share, m.score, m.name);
    }
}
