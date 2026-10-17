//! Past and current runs as the window and `carafe-lab runs` list them: the `summary.json` of every run folder.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::console::{self, ConsoleVerdict};
use crate::engine::{DETAILED, READY};
use crate::report::{BuildOutcome, GameReport, RunReport, RunStamp};
use crate::run::{Outcome, PROBLEMS_FILE};
use lab_core::frames::FrameSummary;
use lab_core::perf::PerfSummary;
use lab_core::problems::{self, Problem, ProblemCounts};
use lab_core::resources::ResourceSummary;
use lab_core::versions::{self, Sample, VersionPerf};

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
    /// The run only built the NSPs, for the console.
    #[serde(default)]
    pub build_only: bool,
    /// What the console said about its games; read from `console.json`, empty in the summary file.
    #[serde(default)]
    pub console: Vec<ConsoleVerdict>,
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
    /// The emulator's performance during its first launch; `None` when the emulator did not tell it.
    #[serde(default)]
    pub perf: Option<PerfSummary>,
    /// The container's peak memory and processor use during its first launch; `None` when not measured.
    #[serde(default)]
    pub resources: Option<ResourceSummary>,
    /// Error and warning lines in the logs of all its launches; `None` when no logs were read.
    #[serde(default)]
    pub problems: Option<ProblemCounts>,
    /// The frame times of its first launch in short; `None` when the emulator recorded none.
    #[serde(default)]
    pub frames: Option<FrameSummary>,
    /// Why it failed, in English; `None` when nothing failed.
    #[serde(default)]
    pub failure: Option<String>,
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
    /// How much higher B's median FPS was than A's, in percent of A's; negative when lower.
    #[serde(default)]
    pub fps_difference_percent: Option<f64>,
    /// The difference of the median FPS is larger than the spread of either variant's median FPS.
    #[serde(default)]
    pub fps_significant: bool,
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
    /// The median of its launches' median FPS.
    #[serde(default)]
    pub median_fps: Option<f64>,
    /// The highest of its launches' median FPS minus the lowest.
    #[serde(default)]
    pub fps_spread: Option<f64>,
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
    let significant = beyond_spread(
        (a.median_first_frame, a.spread, a.runs),
        (b.median_first_frame, b.spread, b.runs),
    );
    let fps_difference_percent = match (a.median_fps, b.median_fps) {
        (Some(first), Some(second)) if first > 0.0 => Some((second - first) / first * 100.0),
        _ => None,
    };
    let fps_significant = beyond_spread(
        (a.median_fps, a.fps_spread, a.runs),
        (b.median_fps, b.fps_spread, b.runs),
    );
    Some(Verdict {
        game: base,
        a,
        b,
        difference_percent,
        significant,
        fps_difference_percent,
        fps_significant,
    })
}

fn beyond_spread(
    a: (Option<f64>, Option<f64>, usize),
    b: (Option<f64>, Option<f64>, usize),
) -> bool {
    match (a.0, b.0) {
        (Some(first), Some(second)) => {
            let spread = a.1.unwrap_or(0.0).max(b.1.unwrap_or(0.0));
            a.2 > 1 && b.2 > 1 && (second - first).abs() > spread
        }
        _ => false,
    }
}

fn median_and_spread(mut values: Vec<f64>) -> (Option<f64>, Option<f64>) {
    values.sort_by(f64::total_cmp);
    let median = match values.len() {
        0 => None,
        count if count % 2 == 1 => Some(values[count / 2]),
        count => Some(f64::midpoint(values[count / 2 - 1], values[count / 2])),
    };
    let spread = values
        .last()
        .zip(values.first())
        .map(|(high, low)| high - low);
    (median, spread)
}

fn variant<'a>(games: impl Iterator<Item = &'a GameSummary>) -> VariantResult {
    let games: Vec<&GameSummary> = games.collect();
    let (median_first_frame, spread) =
        median_and_spread(games.iter().filter_map(|game| game.first_frame).collect());
    let (median_fps, fps_spread) = median_and_spread(
        games
            .iter()
            .filter_map(|game| game.perf.map(|perf| perf.median_fps))
            .collect(),
    );
    VariantResult {
        runs: games.len(),
        passed: games
            .iter()
            .filter(|game| game.outcome != Outcome::Fail)
            .count(),
        median_first_frame,
        spread,
        median_fps,
        fps_spread,
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
                outcome: if report.build_only && matches!(game.build, BuildOutcome::Built { .. }) {
                    Outcome::Console
                } else {
                    game.outcome()
                },
                seconds: game_seconds(game),
                dir: game.dir.clone(),
                first_frame: first_frame(game),
                perf: game
                    .scenario
                    .as_ref()
                    .and_then(|scenario| scenario.launches.first())
                    .and_then(|launch| launch.perf),
                resources: game
                    .scenario
                    .as_ref()
                    .and_then(|scenario| scenario.launches.first())
                    .and_then(|launch| launch.resources),
                problems: game.scenario.as_ref().and_then(|scenario| {
                    scenario
                        .launches
                        .iter()
                        .filter_map(|launch| launch.problems)
                        .reduce(|sum, counts| ProblemCounts {
                            errors: sum.errors + counts.errors,
                            warnings: sum.warnings + counts.warnings,
                        })
                }),
                frames: game
                    .scenario
                    .as_ref()
                    .and_then(|scenario| scenario.launches.first())
                    .and_then(|launch| launch.frames),
                failure: game.failure_reason(),
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
            build_only: report.build_only,
            console: Vec::new(),
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
///
/// A game that waited for the console and has its verdict passes or fails by it; a failing verdict's note becomes
/// the reason it failed.
#[must_use]
pub fn read(runs: &Path, dir: &str) -> Option<RunSummary> {
    let text = fs::read_to_string(runs.join(dir).join(SUMMARY_FILE)).ok()?;
    let mut summary: RunSummary = serde_json::from_str(&text).ok()?;
    summary.dir = dir.to_owned();
    summary.console = console::read(&runs.join(dir));
    for game in &mut summary.games {
        let verdict = summary
            .console
            .iter()
            .find(|verdict| verdict.game == game.dir);
        if let Some(verdict) = verdict.filter(|_| game.outcome == Outcome::Console) {
            game.outcome = if verdict.works {
                Outcome::Pass
            } else {
                Outcome::Fail
            };
            if !verdict.works {
                game.failure = Some(if verdict.note.is_empty() {
                    "fails on the console".to_owned()
                } else {
                    format!("fails on the console: {}", verdict.note)
                });
            }
        }
    }
    Some(summary)
}

/// Returns every game's performance at every Carafe commit of `runs`, oldest commit first; A/B variants and games
/// that never reached their first frame are left out.
#[must_use]
pub fn version_performance(runs: &[RunSummary]) -> Vec<VersionPerf> {
    let samples: Vec<(&str, &str, Sample)> = runs
        .iter()
        .rev()
        .filter_map(|run| Some((run, run.stamp.carafe_commit.as_deref()?)))
        .flat_map(|(run, commit)| {
            run.games
                .iter()
                .filter(|game| {
                    ![VARIANT_A, VARIANT_B, DETAILED, READY]
                        .iter()
                        .any(|suffix| game.name.ends_with(suffix))
                })
                .filter(|game| game.first_frame.is_some())
                .map(move |game| (game.name.as_str(), commit, sample_of(game)))
        })
        .collect();
    versions::by_version(samples)
}

/// Returns the performance of `game` at the newest Carafe commit of `runs` other than `current`, to compare a new
/// run with; `None` when the game has no run at another commit.
#[must_use]
pub fn baseline(runs: &[RunSummary], game: &str, current: Option<&str>) -> Option<VersionPerf> {
    let newest_other = runs
        .iter()
        .filter_map(|run| run.stamp.carafe_commit.as_deref())
        .find(|commit| Some(*commit) != current && !commit.ends_with("+changes"))?;
    version_performance(runs)
        .into_iter()
        .find(|perf| perf.game == game && perf.commit == newest_other)
}

fn sample_of(game: &GameSummary) -> Sample {
    Sample {
        first_frame: game.first_frame,
        fps: game.perf.map(|perf| perf.median_fps),
        p99_ms: game.frames.map(|frames| frames.p99_ms),
        peak_mb: game
            .resources
            .map(|resources| f64::from(resources.peak_memory_mb)),
    }
}

/// The problems in a game's logs in one run, and those the same game's run before did not have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameProblems {
    /// Every problem of all the game's launches, counts added.
    pub problems: Vec<Problem>,
    /// The problems the run before did not have; every problem when there is no run before.
    pub new: Vec<Problem>,
    /// The folder of the run compared with; `None` when the game has no run before.
    pub compared_with: Option<String>,
}

/// Returns the problems of the game in the folder `game_dir` of the run `dir`, compared with the newest older run of
/// a game of the same name.
#[must_use]
pub fn game_problems(runs: &Path, dir: &str, game_dir: &str) -> GameProblems {
    let problems = read_problems(&runs.join(dir).join(game_dir));
    let name = read(runs, dir).and_then(|summary| {
        summary
            .games
            .into_iter()
            .find(|game| game.dir == game_dir)
            .map(|game| game.name)
    });
    let before = name.and_then(|name| {
        list(runs)
            .into_iter()
            .filter(|run| run.dir.as_str() < dir)
            .find_map(|run| {
                let game = run
                    .games
                    .iter()
                    .find(|game| game.name == name && !game.dir.is_empty())?;
                let path = runs.join(&run.dir).join(&game.dir);
                path.is_dir()
                    .then(|| (run.dir.clone(), read_problems(&path)))
            })
    });
    match before {
        Some((compared_with, earlier)) => GameProblems {
            new: problems::new_since(&problems, &earlier),
            problems,
            compared_with: Some(compared_with),
        },
        None => GameProblems {
            new: problems.clone(),
            problems,
            compared_with: None,
        },
    }
}

fn read_problems(game_dir: &Path) -> Vec<Problem> {
    let Ok(entries) = fs::read_dir(game_dir) else {
        return Vec::new();
    };
    let mut launches: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(PROBLEMS_FILE))
        .filter(|file| file.is_file())
        .collect();
    launches.sort();
    problems::merge(launches.iter().map(|file| {
        fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Problem>>(&text).ok())
            .unwrap_or_default()
    }))
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
            build_only: false,
            console: Vec::new(),
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
            perf: None,
            resources: None,
            problems: None,
            frames: None,
            failure: None,
        }
    }

    #[test]
    fn a_console_verdict_decides_the_game_that_waited_for_it() {
        let runs = std::env::temp_dir().join(format!("lab-history-console-{}", std::process::id()));
        let _ = fs::remove_dir_all(&runs);
        let run_dir = runs.join("2026-10-17_20-00-00");
        fs::create_dir_all(&run_dir).unwrap();
        let mut run = summary(&[]);
        run.build_only = true;
        run.games = vec![
            GameSummary {
                dir: "A".to_owned(),
                ..game("A", Outcome::Console, None)
            },
            GameSummary {
                dir: "B".to_owned(),
                ..game("B", Outcome::Console, None)
            },
            GameSummary {
                dir: "C".to_owned(),
                ..game("C", Outcome::Console, None)
            },
        ];
        fs::write(
            run_dir.join(SUMMARY_FILE),
            serde_json::to_string(&run).unwrap(),
        )
        .unwrap();
        for (game, works, note) in [("A", true, ""), ("B", false, "black screen")] {
            console::record(
                &run_dir,
                ConsoleVerdict {
                    game: game.to_owned(),
                    works,
                    note: note.to_owned(),
                    at: "2026-10-17 21:00".to_owned(),
                },
            )
            .unwrap();
        }
        let read = read(&runs, "2026-10-17_20-00-00").unwrap();
        let outcomes: Vec<Outcome> = read.games.iter().map(|game| game.outcome).collect();
        assert_eq!(outcomes, [Outcome::Pass, Outcome::Fail, Outcome::Console]);
        assert_eq!(
            read.games[1].failure.as_deref(),
            Some("fails on the console: black screen")
        );
        let _ = fs::remove_dir_all(&runs);
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
    fn the_verdict_compares_the_median_fps_too() {
        let with_fps = |name: &str, fps: f64| GameSummary {
            perf: Some(PerfSummary {
                samples: 10,
                median_fps: fps,
                low_fps: fps - 5.0,
                median_speed: 1.0,
            }),
            ..game(name, Outcome::Pass, Some(60.0))
        };
        let games = [
            with_fps("NFSU · A", 20.0),
            with_fps("NFSU · B", 30.0),
            with_fps("NFSU · A", 22.0),
            with_fps("NFSU · B", 31.0),
        ];
        let verdict = verdict(&games).unwrap();
        assert_eq!(verdict.a.median_fps, Some(21.0));
        assert!(verdict.fps_significant);
        assert!(verdict.fps_difference_percent.unwrap() > 40.0);
        assert!(!verdict.significant);
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
