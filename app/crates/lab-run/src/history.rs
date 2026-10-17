//! Past and current runs as the window and `carafe-lab runs` list them: the `summary.json` of every run folder.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::report::{BuildOutcome, GameReport, RunReport, RunStamp};
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
    /// What the run was made with; empty in summaries from before it was kept.
    #[serde(default)]
    pub stamp: RunStamp,
    /// The run went through to its end or was stopped; `false` while it goes, or when its process died.
    pub finished: bool,
    /// Names of the games planned, in run order.
    pub planned: Vec<String>,
    /// The games done so far.
    pub games: Vec<GameSummary>,
    /// The verdict when the run was an A/B run of one game.
    #[serde(default)]
    pub verdict: Option<Verdict>,
}

/// A game of a run in short.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    /// Seconds it took: the build and the scenario; zero in summaries from before it was kept.
    #[serde(default)]
    pub seconds: f64,
    /// Its folder inside the run's folder; empty in summaries from before it was kept.
    #[serde(default)]
    pub dir: String,
    /// Seconds from its first launch to its first frame; `None` when it never got there.
    #[serde(default)]
    pub first_frame: Option<f64>,
}

/// What each variant of an A/B run gave and whether they differ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Verdict {
    /// The game the variants ran.
    pub game: String,
    /// Variant A: the game's own settings.
    pub a: VariantResult,
    /// Variant B: the game's settings with the run's overrides.
    pub b: VariantResult,
    /// How much later B reached its first frame than A, in percent of A; negative when B was earlier.
    pub difference_percent: Option<f64>,
    /// The difference of the medians is larger than the spread of either variant's first frames.
    pub significant: bool,
}

/// What one variant of an A/B run gave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VariantResult {
    /// Times it ran.
    pub runs: usize,
    /// Times it did not fail.
    pub passed: usize,
    /// The median of its first-frame times.
    pub median_first_frame: Option<f64>,
    /// The largest first-frame time minus the smallest.
    pub spread: Option<f64>,
}

/// Suffix of the name variant A runs under.
pub const VARIANT_A: &str = " · A";
/// Suffix of the name variant B runs under.
pub const VARIANT_B: &str = " · B";

/// Returns the verdict of a run whose games are the two variants of one game, or `None` for any other run.
#[must_use]
pub fn verdict(games: &[GameSummary]) -> Option<Verdict> {
    let base = games.first()?.name.strip_suffix(VARIANT_A)?.to_owned();
    let name_a = format!("{base}{VARIANT_A}");
    let name_b = format!("{base}{VARIANT_B}");
    if games
        .iter()
        .any(|game| game.name != name_a && game.name != name_b)
    {
        return None;
    }
    let a = variant(games.iter().filter(|game| game.name == name_a));
    let b = variant(games.iter().filter(|game| game.name == name_b));
    let difference_percent = match (a.median_first_frame, b.median_first_frame) {
        (Some(first), Some(second)) if first > 0.0 => Some((second - first) / first * 100.0),
        _ => None,
    };
    let significant = match (a.median_first_frame, b.median_first_frame) {
        (Some(first), Some(second)) => {
            let spread = a.spread.unwrap_or(0.0).max(b.spread.unwrap_or(0.0));
            a.runs > 1 && b.runs > 1 && (second - first).abs() > spread
        }
        _ => false,
    };
    Some(Verdict {
        game: base,
        a,
        b,
        difference_percent,
        significant,
    })
}

fn variant<'a>(games: impl Iterator<Item = &'a GameSummary>) -> VariantResult {
    let games: Vec<&GameSummary> = games.collect();
    let mut times: Vec<f64> = games.iter().filter_map(|game| game.first_frame).collect();
    times.sort_by(f64::total_cmp);
    let median = match times.len() {
        0 => None,
        count if count % 2 == 1 => Some(times[count / 2]),
        count => Some(f64::midpoint(times[count / 2 - 1], times[count / 2])),
    };
    VariantResult {
        runs: games.len(),
        passed: games
            .iter()
            .filter(|game| game.outcome != Outcome::Fail)
            .count(),
        median_first_frame: median,
        spread: times
            .last()
            .zip(times.first())
            .map(|(high, low)| high - low),
    }
}

impl RunSummary {
    /// Creates the summary of `report` with the games `planned` for the run.
    #[must_use]
    pub fn of(report: &RunReport, planned: &[String], finished: bool) -> Self {
        let games: Vec<GameSummary> = report
            .games
            .iter()
            .map(|game| GameSummary {
                name: game.name.clone(),
                title_id: game.title_id.clone(),
                settings: game.settings.clone(),
                outcome: game.outcome(),
                seconds: game_seconds(game),
                dir: game.dir.clone(),
                first_frame: first_frame(game),
            })
            .collect();
        Self {
            dir: String::new(),
            started: report.started.clone(),
            seconds: report.seconds,
            emulator: report.emulator.clone(),
            runtime_version: report.runtime_version.clone(),
            stamp: report.stamp.clone(),
            finished,
            planned: planned.to_vec(),
            verdict: verdict(&games),
            games,
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

fn first_frame(game: &GameReport) -> Option<f64> {
    let launch = &game.scenario.as_ref()?.launches.first()?.launch;
    match &launch.loading_hidden {
        Some((at, reason)) if reason.is_success() => Some(f64::from(*at)),
        _ => None,
    }
}

fn game_seconds(game: &GameReport) -> f64 {
    let build = match game.build {
        BuildOutcome::Built { seconds, .. } => seconds,
        BuildOutcome::Failed { .. } => 0.0,
    };
    let scenario = game
        .scenario
        .as_ref()
        .and_then(|scenario| scenario.steps.last())
        .map_or(0.0, |step| step.at);
    build + scenario
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
            stamp: RunStamp::default(),
            finished: true,
            planned: Vec::new(),
            games: outcomes
                .iter()
                .map(|outcome| game("Game", *outcome, None))
                .collect(),
            verdict: None,
        }
    }

    fn game(name: &str, outcome: Outcome, first_frame: Option<f64>) -> GameSummary {
        GameSummary {
            name: name.to_owned(),
            title_id: String::new(),
            settings: String::new(),
            outcome,
            seconds: 0.0,
            dir: String::new(),
            first_frame,
        }
    }

    #[test]
    fn an_ab_run_gets_a_verdict_beyond_its_spread() {
        let games = [
            game("NFSU · A", Outcome::Pass, Some(100.0)),
            game("NFSU · B", Outcome::Pass, Some(80.0)),
            game("NFSU · A", Outcome::Pass, Some(104.0)),
            game("NFSU · B", Outcome::Fail, Some(82.0)),
        ];
        let verdict = verdict(&games).unwrap();
        assert_eq!(verdict.game, "NFSU");
        assert_eq!(verdict.a.median_first_frame, Some(102.0));
        assert_eq!(verdict.b.median_first_frame, Some(81.0));
        assert_eq!(verdict.b.passed, 1);
        assert!(verdict.significant);
        assert!(verdict.difference_percent.unwrap() < -20.0);
    }

    #[test]
    fn a_difference_inside_the_spread_is_not_flagged() {
        let games = [
            game("G · A", Outcome::Pass, Some(100.0)),
            game("G · B", Outcome::Pass, Some(95.0)),
            game("G · A", Outcome::Pass, Some(90.0)),
            game("G · B", Outcome::Pass, Some(99.0)),
        ];
        assert!(!verdict(&games).unwrap().significant);
    }

    #[test]
    fn a_plain_run_has_no_verdict() {
        assert_eq!(verdict(&[game("OpenTTD", Outcome::Pass, Some(30.0))]), None);
        assert_eq!(
            verdict(&[
                game("G · A", Outcome::Pass, None),
                game("H", Outcome::Pass, None)
            ]),
            None
        );
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
