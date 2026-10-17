//! Past and current runs as the window and `carafe-lab runs` list them: the `summary.json` of every run folder.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::report::RunReport;
use crate::run::Outcome;

/// The summary every run keeps next to its report and rewrites after every game.
pub const SUMMARY_FILE: &str = "summary.json";

/// Every event of a run, one JSON line each, so a window that opens later can follow it.
pub const EVENTS_FILE: &str = "events.jsonl";

/// A run in short.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunSummary {
    /// The run's folder inside `runs/`; empty in the file, filled in when the folder is read.
    #[serde(default)]
    pub dir: String,
    /// When the run started, `YYYY-MM-DD HH:MM` in UTC.
    pub started: String,
    /// Seconds the run has taken so far.
    pub seconds: f64,
    /// The emulator and its version.
    pub emulator: String,
    /// Runtime version the NSPs were built with.
    pub runtime_version: String,
    /// The run went through to its end or was stopped; `false` while it goes, or when its process died.
    pub finished: bool,
    /// Names of the games planned, in run order.
    pub planned: Vec<String>,
    /// The games done so far.
    pub games: Vec<GameSummary>,
}

/// A game of a run in short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameSummary {
    /// The game's name in `lab.json`.
    pub name: String,
    /// Its Title ID; empty if the build failed before it had one.
    pub title_id: String,
    /// Its settings in a few words.
    pub settings: String,
    /// Its worst outcome.
    pub outcome: Outcome,
}

impl RunSummary {
    /// Creates the summary of `report` with the games `planned` for the run.
    #[must_use]
    pub fn of(report: &RunReport, planned: &[String], finished: bool) -> Self {
        Self {
            dir: String::new(),
            started: report.started.clone(),
            seconds: report.seconds,
            emulator: report.emulator.clone(),
            runtime_version: report.runtime_version.clone(),
            finished,
            planned: planned.to_vec(),
            games: report
                .games
                .iter()
                .map(|game| GameSummary {
                    name: game.name.clone(),
                    title_id: game.title_id.clone(),
                    settings: game.settings.clone(),
                    outcome: game.outcome(),
                })
                .collect(),
        }
    }

    /// Returns how many games ended with each outcome: pass, warning, console, fail.
    #[must_use]
    pub fn counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for game in &self.games {
            let slot = match game.outcome {
                Outcome::Pass | Outcome::Skip => 0,
                Outcome::Warn => 1,
                Outcome::Console => 2,
                Outcome::Fail => 3,
            };
            counts[slot] += 1;
        }
        counts
    }
}

/// Returns the runs in `runs` that have a summary, newest first.
///
/// A folder without a readable summary is left out: it is a run from before summaries or not a run.
#[must_use]
pub fn list(runs: &Path) -> Vec<RunSummary> {
    let Ok(entries) = fs::read_dir(runs) else {
        return Vec::new();
    };
    let mut dirs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    dirs.sort_unstable_by(|a, b| b.cmp(a));
    dirs.iter().filter_map(|dir| read(runs, dir)).collect()
}

/// Returns the summary of the run `dir` in `runs`, or `None` if it has none or it cannot be read.
#[must_use]
pub fn read(runs: &Path, dir: &str) -> Option<RunSummary> {
    let text = fs::read_to_string(runs.join(dir).join(SUMMARY_FILE)).ok()?;
    let mut summary: RunSummary = serde_json::from_str(&text).ok()?;
    summary.dir = dir.to_owned();
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(outcomes: &[Outcome]) -> RunSummary {
        RunSummary {
            dir: String::new(),
            started: "2026-10-17 06:28".to_owned(),
            seconds: 1.0,
            emulator: "Eden".to_owned(),
            runtime_version: "dev".to_owned(),
            finished: true,
            planned: Vec::new(),
            games: outcomes
                .iter()
                .map(|outcome| GameSummary {
                    name: "Game".to_owned(),
                    title_id: String::new(),
                    settings: String::new(),
                    outcome: *outcome,
                })
                .collect(),
        }
    }

    #[test]
    fn a_skipped_game_counts_as_passed() {
        let counts = summary(&[Outcome::Skip, Outcome::Fail, Outcome::Warn]).counts();
        assert_eq!(counts, [1, 1, 0, 1]);
    }

    #[test]
    fn the_folder_is_not_kept_in_the_file() {
        let json = r#"{"started":"s","seconds":0,"emulator":"e","runtimeVersion":"v","finished":false,
            "planned":["A"],"games":[]}"#;
        let read: RunSummary = serde_json::from_str(json).unwrap();
        assert_eq!(read.dir, "");
        assert_eq!(read.planned, ["A"]);
    }
}
