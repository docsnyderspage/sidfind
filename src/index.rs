//! The optional precomputed index: the fingerprints of every HVSC tune, in a single file.
//!
//! Layout (all integers little endian):
//!
//! ```text
//! "SIDFIND\0"  u32 format version  u32 window  u32 sampling  u32 tunes  u64 entries
//! tunes x { u16 path length, path (UTF-8, relative to the HVSC root, '/' separated),
//!           u32 exact fingerprints, u32 masked fingerprints }
//! entries x u64 = fingerprint << 24 | tune number, sorted
//! ```
//!
//! Keeping the entries sorted makes "which tunes contain this fingerprint" a binary search.

use std::io::{self, Read, Write};

use crate::scan::Params;

/// Bump when the fingerprints a given byte string produces change (see `scan.rs`).
pub const FORMAT_VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"SIDFIND\0";
const TUNE_BITS: u32 = 24; // 40 bits of fingerprint above them fill the u64

pub struct Tune {
    /// Path relative to the HVSC root, without leading slash.
    pub path: String,
    /// Number of distinct exact and masked fingerprints the tune has.
    pub counts: [u32; 2],
}

pub struct Stored {
    pub params: Params,
    pub tunes: Vec<Tune>,
    entries: Vec<u64>,
}

impl Stored {
    pub const MAX_TUNES: usize = 1 << TUNE_BITS;

    /// `entries` are `(tune number, fingerprint)` pairs in any order.
    pub fn new(params: Params, tunes: Vec<Tune>, pairs: impl IntoIterator<Item = (u32, u64)>) -> Stored {
        assert!(tunes.len() <= Self::MAX_TUNES);
        let mut entries: Vec<u64> = pairs.into_iter().map(|(t, fp)| fp << TUNE_BITS | t as u64).collect();
        entries.sort_unstable();
        Stored { params, tunes, entries }
    }

    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// The numbers of all tunes that contain the fingerprint.
    pub fn tunes_with(&self, fp: u64) -> impl Iterator<Item = u32> + '_ {
        let first = self.entries.partition_point(|&e| e >> TUNE_BITS < fp);
        self.entries[first..]
            .iter()
            .take_while(move |&&e| e >> TUNE_BITS == fp)
            .map(|&e| (e & (Self::MAX_TUNES as u64 - 1)) as u32)
    }

    pub fn write(&self, mut w: impl Write) -> io::Result<()> {
        w.write_all(MAGIC)?;
        for v in [FORMAT_VERSION, self.params.window as u32, self.params.sampling as u32, self.tunes.len() as u32] {
            w.write_all(&v.to_le_bytes())?;
        }
        w.write_all(&(self.entries.len() as u64).to_le_bytes())?;
        for t in &self.tunes {
            w.write_all(&(t.path.len() as u16).to_le_bytes())?;
            w.write_all(t.path.as_bytes())?;
            for c in t.counts {
                w.write_all(&c.to_le_bytes())?;
            }
        }
        let mut chunk = Vec::with_capacity(1 << 20);
        for block in self.entries.chunks(1 << 17) {
            chunk.clear();
            block.iter().for_each(|e| chunk.extend_from_slice(&e.to_le_bytes()));
            w.write_all(&chunk)?;
        }
        w.flush()
    }

    pub fn read(mut r: impl Read) -> Result<Stored, String> {
        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let mut cur = Cursor { bytes: &bytes, at: 0 };
        if cur.take(8)? != MAGIC {
            return Err("this is not a sidfind index file".into());
        }
        let version = cur.u32()?;
        if version != FORMAT_VERSION {
            return Err(format!("the index has format version {version}, this sidfind uses {FORMAT_VERSION}"));
        }
        let (window, sampling, n_tunes) = (cur.u32()?, cur.u32()?, cur.u32()?);
        let n_entries = u64::from_le_bytes(cur.take(8)?.try_into().unwrap()) as usize;
        if window == 0 || sampling == 0 || n_tunes as usize > Self::MAX_TUNES {
            return Err("the index header is invalid".into());
        }
        let mut tunes = Vec::with_capacity(n_tunes as usize);
        for _ in 0..n_tunes {
            let len = u16::from_le_bytes(cur.take(2)?.try_into().unwrap()) as usize;
            let path = String::from_utf8(cur.take(len)?.to_vec()).map_err(|_| "a path in the index is not UTF-8")?;
            tunes.push(Tune { path, counts: [cur.u32()?, cur.u32()?] });
        }
        let raw = cur.take(n_entries.checked_mul(8).ok_or("the index header is invalid")?)?;
        let entries: Vec<u64> = raw.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
        let tune_mask = Self::MAX_TUNES as u64 - 1;
        let invalid = |&e: &u64| (e & tune_mask) as usize >= tunes.len();
        if entries.iter().any(invalid) || entries.windows(2).any(|w| w[0] > w[1]) {
            return Err("the index contains invalid entries".into());
        }
        Ok(Stored { params: Params { window: window as usize, sampling: sampling as usize }, tunes, entries })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.bytes.get(self.at..self.at.saturating_add(n)).ok_or("the file is truncated")?;
        self.at += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{fingerprints, Index};

    const P: Params = Params { window: 12, sampling: 64 };

    fn tune(path: &str, exact: u32) -> Tune {
        Tune { path: path.into(), counts: [exact, 0] }
    }

    fn sample() -> Stored {
        Stored::new(
            P,
            vec![tune("A/one.sid", 3), tune("B/two.sid", 2)],
            [(1, 10), (0, 10), (0, 7), (1, 99), (0, 1 << 39)],
        )
    }

    #[test]
    fn lookup_finds_all_tunes_for_a_fingerprint() {
        let s = sample();
        assert_eq!(s.tunes_with(10).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(s.tunes_with(7).collect::<Vec<_>>(), vec![0]);
        assert_eq!(s.tunes_with(1 << 39).collect::<Vec<_>>(), vec![0]);
        assert_eq!(s.tunes_with(8).count(), 0);
    }

    #[test]
    fn roundtrip() {
        let mut file = Vec::new();
        sample().write(&mut file).unwrap();
        let back = Stored::read(&file[..]).unwrap();
        assert_eq!(back.params, P);
        assert_eq!(back.tunes.iter().map(|t| (t.path.as_str(), t.counts)).collect::<Vec<_>>(), [("A/one.sid", [3, 0]), ("B/two.sid", [2, 0])]);
        assert_eq!(back.entries(), 5);
        assert_eq!(back.tunes_with(10).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn rejects_bad_files() {
        let mut file = Vec::new();
        sample().write(&mut file).unwrap();
        assert!(Stored::read(&b"not an index at all"[..]).err().unwrap().contains("not a sidfind"));
        assert!(Stored::read(&file[..file.len() - 3]).err().unwrap().contains("truncated"));
        let mut old = file.clone();
        old[8] = 99;
        assert!(Stored::read(&old[..]).err().unwrap().contains("format version"));
    }

    #[test]
    fn stored_scan_gives_the_same_matches_as_scanning_the_tune() {
        let tune_a: Vec<u8> = (0..3000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let tune_b: Vec<u8> = (0..3000u32).map(|i| (i.wrapping_mul(40503) >> 7) as u8).collect();
        let mut prg = vec![1u8, 2, 3];
        prg.extend_from_slice(&tune_b[500..2500]);
        let (fa, fb) = (fingerprints(&tune_a, P), fingerprints(&tune_b, P));
        let stored = Stored::new(
            P,
            vec![tune("a.sid", fa.len() as u32), tune("b.sid", fb.len() as u32)],
            fa.iter().map(|&f| (0, f)).chain(fb.iter().map(|&f| (1, f))),
        );
        let mut index = Index::new(P);
        index.add(0, &[&prg]);
        let live = index.scan(&fb);
        let via_index = index.scan_stored(&stored);
        assert_eq!(live.len(), 1);
        assert_eq!(via_index.len(), 1);
        let (tune_no, hit) = &via_index[0];
        assert_eq!(*tune_no, 1);
        let (mut x, mut y) = (live[0].windows.clone(), hit.windows.clone());
        x.sort();
        y.sort();
        assert_eq!(x, y);
        assert!(!x.is_empty());
    }
}
