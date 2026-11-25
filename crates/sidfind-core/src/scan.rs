//! Finds byte windows that a set of PRGs (the queries) share with tune payloads.
//!
//! Two kinds of windows are used. Exact windows find data that was copied verbatim. Masked
//! windows find code that was relocated: the 16-bit operands of absolute-address instructions
//! are wildcards, since those are the bytes a relocator rewrites.
//!
//! A window is represented by a 40-bit fingerprint (39 bits of hash and the kind of window). With sampling width 1 every window is
//! kept (exact, used when scanning HVSC on the fly). With a larger width k, only the smallest
//! fingerprint of every k consecutive windows is kept ("winnowing"), which shrinks the data
//! about (k+1)/2 times while still guaranteeing that any shared region of at least
//! `window + k - 1` bytes shares a fingerprint. That is what makes a small, precomputed index
//! of HVSC possible.
//!
//! NOTE: changing anything that affects which fingerprints a byte string produces (hashing,
//! window sizes, masking rules, winnowing) makes existing index files wrong, so bump
//! `index::FORMAT_VERSION` too. `fingerprints_are_stable` fails when that happens.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::index::Stored;
use crate::opcodes;

/// Windows with fewer distinct byte values (zero runs, fill patterns) match everything.
const MIN_DISTINCT: usize = 4;
const BASE: u64 = 0x0000_0100_0000_01b3;
const FILTER_BITS: u32 = 24;
/// Masked windows carry less information per byte, so they are longer than exact ones.
const MASKED_EXTRA: usize = 4;
/// Distinct values required among the bytes of a masked window that are not wildcards.
const MASKED_MIN_DISTINCT: usize = 5;
/// Keeps the hash spaces of exact and masked windows apart.
const MASKED_SALT: u64 = 0x6d61_736b_6564_2121;
pub const FINGERPRINT_BITS: u32 = 40;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Params {
    /// Window size in bytes.
    pub window: usize,
    /// Sampling width: 1 keeps every window.
    pub sampling: usize,
}

impl Params {
    /// How many windows one fingerprint stands for, on average.
    pub fn scale(&self) -> f64 {
        (self.sampling as f64 + 1.0) / 2.0
    }
}

pub struct Index {
    params: Params,
    windows: HashMap<u64, Vec<u32>>,
    filter: Vec<u64>,
}

pub struct Hit {
    pub query: u32,
    /// The distinct fingerprints the query and the tune share.
    pub windows: Vec<u64>,
}

/// The sorted, distinct fingerprints of the exact and masked windows of `data`. The lowest
/// bit of a fingerprint tells which kind it is (see [`is_masked`]).
pub fn fingerprints(data: &[u8], p: Params) -> Vec<u64> {
    let mut out = Vec::new();
    let mut stream = Vec::new();
    for masked in [false, true] {
        stream.clear();
        if masked {
            for_each_masked_window(data, p.window + MASKED_EXTRA, |_, h| stream.push(mix(h)));
        } else {
            for_each_window(data, p.window, |_, h| stream.push(mix(h)));
        }
        let mut kept = Vec::new();
        winnow(&stream, p.sampling, &mut kept);
        out.extend(kept.into_iter().map(|v| (v >> (65 - FINGERPRINT_BITS)) << 1 | masked as u64));
    }
    out.sort_unstable();
    out.dedup();
    out
}

pub fn is_masked(fingerprint: u64) -> bool {
    fingerprint & 1 == 1
}

/// How many exact and how many masked fingerprints there are.
pub fn count_kinds(fingerprints: &[u64]) -> [usize; 2] {
    let masked = fingerprints.iter().filter(|&&f| is_masked(f)).count();
    [fingerprints.len() - masked, masked]
}

/// Keeps the smallest value of every `k` consecutive values (all values if `k` is 1; the
/// overall smallest if there are fewer than `k`).
fn winnow(stream: &[u64], k: usize, out: &mut Vec<u64>) {
    if k <= 1 {
        out.extend_from_slice(stream);
    } else if stream.len() <= k {
        out.extend(stream.iter().min());
    } else {
        let mut candidates: VecDeque<usize> = VecDeque::new();
        for (i, &v) in stream.iter().enumerate() {
            while candidates.back().is_some_and(|&j| stream[j] >= v) {
                candidates.pop_back();
            }
            candidates.push_back(i);
            while candidates.front().is_some_and(|&j| j + k <= i) {
                candidates.pop_front();
            }
            if i + 1 >= k {
                out.push(stream[candidates[0]]);
            }
        }
    }
}

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

impl Index {
    pub fn new(params: Params) -> Self {
        assert!(params.window > 0 && params.sampling > 0);
        Index { params, windows: HashMap::new(), filter: vec![0; 1 << (FILTER_BITS - 6)] }
    }

    /// Registers a query under the given id; its data may come from several sources (e.g. the
    /// raw file and its unpacked memory). Returns how many distinct fingerprints it contributes.
    pub fn add(&mut self, id: u32, sources: &[&[u8]]) -> usize {
        let distinct: HashSet<u64> =
            sources.iter().flat_map(|data| fingerprints(data, self.params)).collect();
        for &fp in &distinct {
            self.windows.entry(fp).or_default().push(id);
            let bit = filter_bit(fp);
            self.filter[bit >> 6] |= 1 << (bit & 63);
        }
        distinct.len()
    }

    /// Matches the fingerprints of one tune (as returned by [`fingerprints`]) against the queries.
    pub fn scan(&self, tune: &[u64]) -> Vec<Hit> {
        let mut shared: HashMap<u32, Vec<u64>> = HashMap::new();
        for &fp in tune {
            let bit = filter_bit(fp);
            if self.filter[bit >> 6] & (1 << (bit & 63)) == 0 {
                continue;
            }
            for &q in self.windows.get(&fp).into_iter().flatten() {
                shared.entry(q).or_default().push(fp);
            }
        }
        shared.into_iter().map(|(query, windows)| Hit { query, windows }).collect()
    }

    /// Matches the queries against every tune of a precomputed index.
    pub fn scan_stored(&self, stored: &Stored) -> Vec<(u32, Hit)> {
        let mut shared: HashMap<(u32, u32), Vec<u64>> = HashMap::new();
        for (&fp, queries) in &self.windows {
            for tune in stored.tunes_with(fp) {
                for &q in queries {
                    shared.entry((tune, q)).or_default().push(fp);
                }
            }
        }
        shared.into_iter().map(|((tune, query), windows)| (tune, Hit { query, windows })).collect()
    }
}

fn filter_bit(fp: u64) -> usize {
    (fp.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - FILTER_BITS)) as usize
}

/// Calls `f(position, hash)` for every window of `data` that is not low-entropy.
pub fn for_each_window(data: &[u8], w: usize, mut f: impl FnMut(usize, u64)) {
    if data.len() < w {
        return;
    }
    let base_pow_w = (0..w).fold(1u64, |p, _| p.wrapping_mul(BASE));
    let mut counts = [0u16; 256];
    let mut distinct = 0usize;
    let mut h = 0u64;
    for (i, &b) in data.iter().enumerate() {
        if counts[b as usize] == 0 {
            distinct += 1;
        }
        counts[b as usize] += 1;
        h = h.wrapping_mul(BASE).wrapping_add(b as u64 + 1);
        if i >= w {
            let out = data[i - w];
            h = h.wrapping_sub((out as u64 + 1).wrapping_mul(base_pow_w));
            counts[out as usize] -= 1;
            if counts[out as usize] == 0 {
                distinct -= 1;
            }
        }
        if i + 1 >= w && distinct >= MIN_DISTINCT {
            f(i + 1 - w, h);
        }
    }
}

/// Calls `f(position, hash)` for every window of `data` in which at least one absolute
/// operand was masked out. Windows without one are the same as exact windows and are skipped.
///
/// Instructions are decoded from the start of each window, so there is no need to know where
/// code starts in the data: a window that starts on an instruction boundary decodes the same
/// way in the tune and in the relocated copy.
pub fn for_each_masked_window(data: &[u8], w: usize, mut f: impl FnMut(usize, u64)) {
    let mut buf = vec![0u8; w];
    let mut kept = vec![false; w];
    for pos in 0..data.len().saturating_sub(w - 1) {
        buf.copy_from_slice(&data[pos..pos + w]);
        kept.iter_mut().for_each(|k| *k = true);
        let mut masked = false;
        let mut i = 0;
        while i < w {
            let info = opcodes::info(buf[i]).unwrap_or(opcodes::DATA);
            if info.abs {
                for j in (i + 1)..(i + 3).min(w) {
                    buf[j] = 0;
                    kept[j] = false;
                    masked = true;
                }
            }
            i += info.len;
        }
        if !masked {
            continue;
        }
        let mut seen = [false; 256];
        let distinct = (0..w).filter(|&i| kept[i] && !std::mem::replace(&mut seen[buf[i] as usize], true)).count();
        if distinct < MASKED_MIN_DISTINCT {
            continue;
        }
        let h = buf.iter().fold(MASKED_SALT, |h, &b| h.wrapping_mul(BASE).wrapping_add(b as u64 + 1));
        f(pos, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const EXACT: Params = Params { window: 12, sampling: 1 };
    pub const SAMPLED: Params = Params { window: 12, sampling: 64 };

    pub fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                (s >> 24) as u8
            })
            .collect()
    }

    /// Code made of absolute-address instructions with the given operand offset applied.
    pub fn code(n: usize, relocate_by: u16) -> Vec<u8> {
        let mut s = 5u32;
        let mut out = Vec::new();
        for _ in 0..n {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let opcodes = [0xbd, 0x9d, 0x8d, 0xad, 0x20, 0xa9, 0x85, 0xca, 0xd0, 0xc9];
            let op = opcodes[(s >> 24) as usize % opcodes.len()];
            out.push(op);
            let info = opcodes::info(op).unwrap();
            let operand = (s >> 8) as u16;
            if info.abs {
                out.extend_from_slice(&operand.wrapping_add(relocate_by).to_le_bytes());
            } else if info.len == 2 {
                out.push(operand as u8);
            }
        }
        out
    }

    #[test]
    fn rolling_hash_equals_fresh_hash() {
        let data = noise(200, 1);
        let w = 12;
        let mut rolled = Vec::new();
        for_each_window(&data, w, |p, h| rolled.push((p, h)));
        for (p, h) in rolled {
            let mut fresh = None;
            for_each_window(&data[p..p + w], w, |_, x| fresh = Some(x));
            assert_eq!(Some(h), fresh, "window at {p}");
        }
    }

    #[test]
    fn low_entropy_windows_are_skipped() {
        let mut n = 0;
        for_each_window(&[0u8; 100], 12, |_, _| n += 1);
        assert_eq!(n, 0);
    }

    #[test]
    fn finds_embedded_payload() {
        let tune = noise(300, 7);
        let mut prg = noise(500, 9);
        prg[100..400].copy_from_slice(&tune);
        let mut index = Index::new(EXACT);
        index.add(0, &[&prg]);
        assert!(index.scan(&fingerprints(&noise(300, 11), EXACT)).is_empty());
        let tune_fps = fingerprints(&tune, EXACT);
        let hits = index.scan(&tune_fps);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].windows.len(), tune_fps.len());
    }

    #[test]
    fn finds_code_whose_absolute_operands_were_rewritten() {
        let tune = code(200, 0);
        let moved = code(200, 0x0123);
        assert_ne!(tune, moved);
        let mut prg = noise(300, 3);
        prg.extend_from_slice(&moved);
        prg.extend(noise(300, 4));
        let mut index = Index::new(EXACT);
        index.add(0, &[&prg]);
        let tune_fps = fingerprints(&tune, EXACT);
        let hits = index.scan(&tune_fps);
        assert_eq!(hits.len(), 1);
        // The code is found through its masked windows, not through exact ones. Only windows
        // that start on an instruction boundary decode the same in both copies (a window that
        // starts inside an instruction reads a rewritten operand as an opcode), so about half
        // of them match.
        let (found, total) = (count_kinds(&hits[0].windows), count_kinds(&tune_fps));
        assert!(found[1] * 10 > total[1] * 4, "masked: {} of {}", found[1], total[1]);
        assert!(found[0] * 10 < total[0], "exact: {} of {}", found[0], total[0]);
        // unrelated code is not matched just for being code
        let unrelated: Vec<u8> = tune.iter().map(|b| b ^ 0x55).collect();
        assert!(index.scan(&fingerprints(&unrelated, EXACT)).is_empty());
    }

    #[test]
    fn sampling_still_finds_every_long_enough_shared_region() {
        // any shared region of at least window + sampling - 1 = 75 bytes must share a fingerprint
        for seed in 0..40 {
            let shared = noise(80, 1000 + seed);
            let mut a = noise(400, seed);
            let mut b = noise(300, seed + 500);
            a.splice(150..150, shared.iter().copied());
            b.splice(70..70, shared.iter().copied());
            let fb = fingerprints(&b, SAMPLED);
            assert!(fingerprints(&a, SAMPLED).iter().any(|f| fb.binary_search(f).is_ok()), "seed {seed}");
        }
    }

    #[test]
    fn sampling_shrinks_the_fingerprint_set() {
        let data = noise(20_000, 3);
        let (all, some) = (fingerprints(&data, EXACT).len(), fingerprints(&data, SAMPLED).len());
        assert!(some * 20 < all && some * 40 > all / 2, "{some} of {all}");
        assert!(!fingerprints(&noise(30, 5), SAMPLED).is_empty(), "short tunes keep a fingerprint");
    }

    #[test]
    fn fingerprints_are_stable() {
        // If this fails, existing index files no longer match: update the value and bump
        // index::FORMAT_VERSION in the same change.
        let mut data = noise(2000, 42);
        data.extend(code(300, 0));
        let fps = fingerprints(&data, SAMPLED);
        let digest = fps.iter().fold(0u64, |h, &f| h.rotate_left(5) ^ f);
        assert_eq!((fps.len(), digest), (155, 1910212068307223857));
    }
}
