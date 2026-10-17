//! Carafe Lab's runs: a scenario's steps against an [`run::Emulator`], the report of a run, the history of runs,
//! the queue for the emulator and keeping the runs folder small.

pub mod engine;
pub mod history;
pub mod queue;
pub mod ready;
pub mod report;
pub mod run;
pub mod tidy;
