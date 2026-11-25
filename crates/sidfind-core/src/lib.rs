//! Identify which HVSC tune a C64 PRG or VICE snapshot uses.
//!
//! The building blocks (`cpu`, `scan`, `index`, `sid`, `vsf`) are pure. [`api`] ties them into
//! the workflow shared by the command line tool and the GUI: it never prints and never exits,
//! it reports through return values and an optional [`api::Control`].

pub mod api;
pub mod cpu;
pub mod index;
pub mod opcodes;
pub mod scan;
pub mod sid;
pub mod vsf;

pub use api::{
    build_index, identify, Control, Identification, IndexSummary, Match, ScanSettings, Source, DEFAULT_INDEX_SAMPLING,
    DEFAULT_WINDOW,
};
