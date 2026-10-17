//! Carafe Lab's core: the lab file, scenarios, log milestones, picture comparison, settings sets and the probe
//! programs' runs.
//!
//! Everything here is pure and covered by unit tests; the emulator and the files of a run are reached by the
//! crates around it.

pub mod config;
pub mod emulator;
pub mod firmware;
pub mod input_check;
pub mod lang;
pub mod local;
pub mod matrix;
pub mod milestones;
pub mod perf;
pub mod picture;
pub mod probes;
pub mod problems;
pub mod redact;
pub mod resources;
pub mod roadmap;
pub mod scenario;
pub mod time;
pub mod view;
