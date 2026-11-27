//! Background work. The core runs on a worker thread; the UI thread only reads atomics and a
//! result slot (polled by a timer), so no widget is ever touched from another thread.

use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use sidfind_core::scan::Params;
use sidfind_core::{
    Control, DEFAULT_INDEX_SAMPLING, DEFAULT_WINDOW, Identification, IndexSummary, ScanSettings,
    Source, build_index, identify,
};

pub enum Outcome {
    Built {
        path: PathBuf,
        summary: IndexSummary,
    },
    Scanned(Vec<Identification>),
}

pub type JobResult = Result<Outcome, String>;

pub struct Job {
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    result: Arc<Mutex<Option<JobResult>>>,
}

impl Job {
    fn spawn(work: impl FnOnce(&Control) -> JobResult + Send + 'static) -> Job {
        let job = Job {
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
            result: Arc::new(Mutex::new(None)),
        };
        let (cancel, done, total, result) = (
            job.cancel.clone(),
            job.done.clone(),
            job.total.clone(),
            job.result.clone(),
        );
        thread::spawn(move || {
            // called from several worker threads, in any order: keep the highest count
            let progress = |d: usize, t: usize| {
                done.fetch_max(d, Ordering::Relaxed);
                total.store(t, Ordering::Relaxed);
            };
            let ctl = Control {
                progress: Some(&progress),
                cancel: Some(&cancel),
                notice: None,
            };
            // a panic must still produce a result, or the UI would wait for it forever
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| work(&ctl)))
                .unwrap_or_else(|_| Err("internal error: the background job crashed".into()));
            *result.lock().unwrap() = Some(outcome);
        });
        job
    }

    /// Builds an index of `hvsc` into `out`.
    pub fn build(hvsc: PathBuf, out: PathBuf) -> Job {
        Job::spawn(move |ctl| {
            let params = Params {
                window: DEFAULT_WINDOW,
                sampling: DEFAULT_INDEX_SAMPLING,
            };
            let summary = build_index(&hvsc, &out, params, ctl)?;
            Ok(Outcome::Built { path: out, summary })
        })
    }

    /// Identifies `file` against `source`.
    pub fn scan(file: PathBuf, source: Source, settings: ScanSettings) -> Job {
        Job::spawn(move |ctl| identify(&[file], &source, &settings, ctl).map(Outcome::Scanned))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Progress in percent; 0 while the total is not known yet.
    pub fn percent(&self) -> i32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            0
        } else {
            (self.done.load(Ordering::Relaxed) * 100 / total).min(100) as i32
        }
    }

    /// The result, once the worker has finished (returned at most once).
    pub fn take_result(&self) -> Option<JobResult> {
        self.result.lock().unwrap().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                (s >> 24) as u8
            })
            .collect()
    }

    /// A tiny HVSC with two tunes and a PRG holding the second, in a fresh temp folder.
    fn fixture(tag: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("sidfind-gui-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("A")).unwrap();
        for (name, seed) in [("one", 1u32), ("two", 2)] {
            let mut sid = vec![0u8; 0x76];
            sid[..4].copy_from_slice(b"PSID");
            sid[6..8].copy_from_slice(&0x76u16.to_be_bytes());
            sid[8..10].copy_from_slice(&0x1000u16.to_be_bytes());
            sid.extend(noise(3000, seed));
            std::fs::write(dir.join(format!("A/{name}.sid")), sid).unwrap();
        }
        let mut prg = vec![0x00, 0x10];
        prg.extend(noise(3000, 2));
        std::fs::write(dir.join("demo.prg"), prg).unwrap();
        (dir.clone(), dir.join("demo.prg"))
    }

    /// Polls like the UI timer does.
    fn wait(job: &Job) -> JobResult {
        let start = Instant::now();
        loop {
            if let Some(result) = job.take_result() {
                return result;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "job did not finish"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn build_then_scan_through_the_index() {
        let (dir, prg) = fixture("flow");
        let index = dir.join("hvsc.idx");

        let job = Job::build(dir.clone(), index.clone());
        let Ok(Outcome::Built { path, summary }) = wait(&job) else {
            panic!("build failed")
        };
        assert_eq!(path, index);
        assert_eq!(summary.tunes, 2);
        assert_eq!(job.percent(), 100);
        assert!(job.take_result().is_none(), "the result is handed out once");

        let settings = ScanSettings {
            cycles: 0,
            min_score: 1.0,
            ..ScanSettings::default()
        };
        let job = Job::scan(prg, Source::Index(index), settings);
        let Ok(Outcome::Scanned(found)) = wait(&job) else {
            panic!("scan failed")
        };
        assert_eq!(found[0].matches[0].name, "A/two.sid");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancel_stops_a_build_and_leaves_no_file() {
        let (dir, _) = fixture("cancel");
        let index = dir.join("cancelled.idx");
        let job = Job::build(dir.clone(), index.clone());
        job.cancel(); // may land before or after the worker finishes: both are valid outcomes
        match wait(&job) {
            Err(e) => {
                assert_eq!(e, sidfind_core::api::CANCELLED);
                assert!(!index.exists());
            }
            Ok(_) => assert!(index.exists()),
        }
        assert!(!dir.join("cancelled.idx.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_crashing_job_still_reports_a_result() {
        let job = Job::spawn(|_| panic!("boom"));
        assert!(wait(&job).err().unwrap().starts_with("internal error"));
    }

    #[test]
    fn errors_come_back_as_text() {
        let job = Job::scan(
            PathBuf::from("/nonexistent.prg"),
            Source::Hvsc(PathBuf::from("/nonexistent")),
            ScanSettings::default(),
        );
        assert!(wait(&job).err().unwrap().starts_with("cannot read"));
        assert_eq!(
            Job::build(PathBuf::from("/nonexistent"), PathBuf::from("/tmp/x.idx")).percent(),
            0
        );
    }
}
