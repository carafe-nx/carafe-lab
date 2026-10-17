//! A whole run: builds every planned game with Carafe's packer, runs its scenario in the emulator, writes
//! `report.json` and `report.html` after every game and tells the front end what happens as [`Event`]s.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use carafe_core::BuildRecord;
use carafe_core::cancel::CancelToken;
use carafe_core::ports::{BuildProgress, PackTarget, Packer};
use carafe_core::wizard::{self, BuildRequest};
use lab_core::diagnostics::{self, Diagnostic};
use lab_core::lang::Lang;
use lab_core::milestones::Stage;
use lab_core::redact::redact;
use lab_core::scenario::{Scenario, Step};
use serde::Serialize;
use ts_rs::TS;

use crate::history::{self, EVENTS_FILE, RunSummary, SUMMARY_FILE};
use crate::ready::{self, ReadyNsp};
use crate::report::{self, BuildOutcome, GameReport, RunReport, RunStamp};
use crate::run::{Clock, Emulator, LaunchRecord, Outcome, Runner, StepResult};

/// A game the run is going to build and check.
#[derive(Debug, Clone)]
pub struct Planned {
    /// Its name in the report and the name of its folder in the run.
    pub name: String,
    /// The game folder.
    pub folder: PathBuf,
    /// What to build, or why it cannot be built.
    pub request: Result<BuildRequest, String>,
    /// Entropy of its Title ID, so the game keeps its data in the emulator between runs.
    pub entropy: u64,
    /// Its settings in a few words, for the report.
    pub label: String,
    /// What to do in the game.
    pub scenario: Scenario,
    /// The percentage the game may get slower by against the last Carafe version before it fails.
    pub threshold: u32,
    /// Where its NSP comes from.
    pub nsp: NspSource,
    /// The diagnostics its launches run with.
    pub diagnostics: Vec<Diagnostic>,
}

/// Where a planned game's NSP comes from.
#[derive(Debug, Clone, Default)]
pub enum NspSource {
    /// Built from the request with the lab's runtime.
    #[default]
    Build,
    /// A ready NSP, run as it is; the request is not used.
    Ready(Box<ReadyNsp>),
    /// Built again with the lab's runtime from the request made of this ready NSP's record, keeping its Title ID.
    Rebuild(Box<BuildRecord>),
}

struct Built {
    record: BuildRecord,
    seconds: f64,
    size_bytes: u64,
    reused: bool,
    nsp: PathBuf,
}

/// What the run is doing, for the window, the live page and `--json`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "event")]
#[ts(export)]
pub enum Event {
    /// Another run holds the emulator; this one waits for its turn.
    #[serde(rename_all = "camelCase")]
    Waiting {
        /// Who holds the emulator.
        holder: String,
        /// Runs ahead of this one, the holder's included.
        ahead: usize,
    },
    /// The run started; its files go into `dir`.
    #[serde(rename_all = "camelCase")]
    RunStarted {
        /// The run's folder.
        dir: String,
        /// How many games are planned.
        games: usize,
    },
    /// A game's turn came; `index` counts from 1.
    #[serde(rename_all = "camelCase")]
    GameStarted {
        /// Its place in the run.
        index: usize,
        /// Its name.
        name: String,
    },
    /// The NSP build reached a stage.
    BuildStage {
        /// The stage, as Carafe names it.
        stage: String,
    },
    /// The NSP is ready.
    #[serde(rename_all = "camelCase")]
    Built {
        /// The game's Title ID.
        title_id: String,
        /// Seconds the build took; zero when an earlier build was reused.
        seconds: f64,
        /// The NSP was built earlier from the same game, settings and runtime.
        reused: bool,
    },
    /// The NSP could not be built.
    BuildFailed {
        /// Why.
        error: String,
    },
    /// The game's logs will appear in `dir` while it runs.
    LaunchLogs {
        /// The folder the emulator writes the game's logs to.
        dir: String,
    },
    /// The launch reached a new start-up stage while a step waited for it.
    Reached {
        /// The stage.
        stage: Stage,
    },
    /// A step of the scenario ended.
    Step {
        /// What happened.
        step: StepResult,
    },
    /// The game is done.
    GameFinished {
        /// The game's name.
        name: String,
        /// Its worst outcome.
        outcome: Outcome,
    },
    /// The run was stopped before every game had its turn.
    Stopped {
        /// How many games were left out.
        left: usize,
    },
    /// The run is over.
    #[serde(rename_all = "camelCase")]
    RunFinished {
        /// `report.html`.
        report: String,
        /// Games that passed.
        passed: usize,
        /// Games that passed with warnings.
        warned: usize,
        /// Games that need the console's word.
        console: usize,
        /// Games that failed.
        failed: usize,
    },
}

/// Everything a run needs from outside.
pub struct Engine<'a> {
    /// The emulator the games run in.
    pub emulator: &'a mut dyn Emulator,
    /// Carafe's packer.
    pub packer: &'a dyn Packer,
    /// The keys file the packer takes.
    pub keys: &'a Path,
    /// Files whose change makes every NSP stale: the runtime archive and hacBrewPack.
    pub build_inputs: Vec<PathBuf>,
    /// The work folder: `nsp/` and `runs/` inside it.
    pub work: &'a Path,
    /// Language of `report.html`.
    pub lang: Lang,
    /// The runtime version the build records carry.
    pub runtime_version: &'a str,
    /// The emulator in a few words, for the report.
    pub emulator_label: String,
    /// What the run is made with.
    pub stamp: RunStamp,
    /// Only build the NSPs.
    pub build_only: bool,
    /// Runs a game that failed once more, with detailed logs chosen by how it failed.
    pub retry_failed: bool,
    /// Stops the run between games and the current scenario between steps.
    pub stop: CancelToken,
}

/// Suffix of the name a failed game runs again under, with detailed logs.
pub const DETAILED: &str = " · detailed";

/// Suffix of the name a ready NSP runs under as it is, with the runtime inside it.
pub const READY: &str = " · NSP";

/// How a game failed, as far as its first failed step tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symptom {
    /// It never reached its first frame: a hang or a stall at start-up.
    NoFirstFrame,
    /// The game process crashed.
    Crashed,
    /// A picture check failed: a black screen, the wrong picture, no change.
    Picture,
    /// Anything else, the emulator's own end among it.
    Other,
}

impl Symptom {
    /// Returns how `game` failed; `None` when it did not fail or its build did.
    #[must_use]
    pub fn of(game: &GameReport) -> Option<Self> {
        let step = game
            .scenario
            .as_ref()?
            .steps
            .iter()
            .find(|step| step.outcome == Outcome::Fail)?;
        Some(match &step.step {
            Step::WaitFor { .. } if step.detail.starts_with("the game crashed") => Self::Crashed,
            Step::WaitFor { .. } if step.detail.starts_with("the emulator") => Self::Other,
            Step::WaitFor { .. } => Self::NoFirstFrame,
            Step::ExpectChange { .. } | Step::ExpectPattern { .. } | Step::Screenshot { .. } => {
                Self::Picture
            }
            _ => Self::Other,
        })
    }

    /// Returns the diagnostics that tell most about this kind of failure.
    #[must_use]
    pub fn diagnostics(self) -> Vec<Diagnostic> {
        match self {
            Self::NoFirstFrame => vec![
                Diagnostic::Verbose,
                Diagnostic::Profiler,
                Diagnostic::Stacks,
            ],
            Self::Crashed | Self::Other => vec![Diagnostic::Verbose],
            Self::Picture => vec![Diagnostic::Verbose, Diagnostic::Hud],
        }
    }
}

/// Returns `game` with `diagnostics` turned on as well: those of the NSP in its build request, all of them in the
/// list the engine hands the emulator, their names in its label.
#[must_use]
pub fn with_diagnostics(mut game: Planned, diagnostics: &[Diagnostic]) -> Planned {
    if let Ok(request) = game.request.as_mut() {
        for diagnostic in diagnostics {
            diagnostic.apply(&mut request.settings);
        }
    }
    let mut all: Vec<Diagnostic> = game
        .diagnostics
        .iter()
        .chain(diagnostics)
        .copied()
        .collect();
    all.sort_unstable();
    all.dedup();
    if !diagnostics.is_empty() {
        game.label = format!("{} · {}", game.label, diagnostics::label(diagnostics));
    }
    game.diagnostics = all;
    game
}

/// Returns `game` once more with the diagnostics chosen by how `report` says it failed; `None` when the game did not
/// fail, could not be built or is a ready NSP run as it is.
#[must_use]
pub fn detailed(game: &Planned, report: &GameReport) -> Option<Planned> {
    if matches!(game.nsp, NspSource::Ready(_)) {
        return None;
    }
    let symptom = Symptom::of(report)?;
    game.request.as_ref().ok()?;
    let again = Planned {
        name: format!("{}{DETAILED}", game.name),
        ..game.clone()
    };
    Some(with_diagnostics(again, &symptom.diagnostics()))
}

impl Engine<'_> {
    /// Runs `planned` in order; the run's folder is named `folder` inside `runs/`.
    ///
    /// # Errors
    ///
    /// A message when the run's folders cannot be made or the report cannot be written; a game that fails to build
    /// or to run is a result in the report, not an error.
    pub fn run(
        &mut self,
        planned: Vec<Planned>,
        folder: &str,
        started: &str,
        clock: &dyn Clock,
        on_event: &mut dyn FnMut(&Event),
    ) -> Result<RunReport, String> {
        let run_dir = self.work.join("runs").join(folder);
        let nsp_dir = self.work.join("nsp");
        fs::create_dir_all(&run_dir).map_err(|error| format!("{}: {error}", run_dir.display()))?;
        fs::create_dir_all(&nsp_dir).map_err(|error| format!("{}: {error}", nsp_dir.display()))?;
        let mut report = RunReport {
            started: started.to_owned(),
            runtime_version: self.runtime_version.to_owned(),
            emulator: self.emulator_label.clone(),
            stamp: self.stamp.clone(),
            seconds: 0.0,
            games: Vec::new(),
            build_only: self.build_only,
        };
        let total = planned.len();
        let mut names: Vec<String> = planned.iter().map(|game| game.name.clone()).collect();
        let mut events = OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir.join(EVENTS_FILE))
            .ok();
        let mut on_event = |event: &Event| {
            record_event(events.as_mut(), event);
            on_event(event);
        };
        write_summary(&run_dir, &report, &names, false)?;
        on_event(&Event::RunStarted {
            dir: run_dir.display().to_string(),
            games: total,
        });
        for (index, game) in planned.into_iter().enumerate() {
            if self.stop.is_cancelled() {
                on_event(&Event::Stopped {
                    left: total - index,
                });
                break;
            }
            on_event(&Event::GameStarted {
                index: index + 1,
                name: game.name.clone(),
            });
            let entry = self.one_game(&game, &run_dir, &nsp_dir, clock, &mut on_event);
            on_event(&Event::GameFinished {
                name: entry.name.clone(),
                outcome: entry.outcome(),
            });
            let again = if self.retry_failed && !self.build_only {
                detailed(&game, &entry)
            } else {
                None
            };
            report.games.push(entry);
            report.seconds = clock.now().as_secs_f64();
            write_report(&run_dir, &report, self.lang)?;
            write_summary(&run_dir, &report, &names, false)?;
            if let Some(again) = again.filter(|_| !self.stop.is_cancelled()) {
                names.push(again.name.clone());
                on_event(&Event::GameStarted {
                    index: index + 1,
                    name: again.name.clone(),
                });
                let entry = self.one_game(&again, &run_dir, &nsp_dir, clock, &mut on_event);
                on_event(&Event::GameFinished {
                    name: entry.name.clone(),
                    outcome: entry.outcome(),
                });
                report.games.push(entry);
                report.seconds = clock.now().as_secs_f64();
                write_report(&run_dir, &report, self.lang)?;
                write_summary(&run_dir, &report, &names, false)?;
            }
        }
        if !self.build_only {
            self.emulator.finish();
        }
        write_report(&run_dir, &report, self.lang)?;
        write_summary(&run_dir, &report, &names, true)?;
        let [passed, warned, console, failed] = report.counts();
        on_event(&Event::RunFinished {
            report: run_dir.join("report.html").display().to_string(),
            passed,
            warned,
            console,
            failed,
        });
        Ok(report)
    }

    fn one_game(
        &mut self,
        game: &Planned,
        run_dir: &Path,
        nsp_dir: &Path,
        clock: &dyn Clock,
        on_event: &mut dyn FnMut(&Event),
    ) -> GameReport {
        let base = safe_name(&game.name);
        let dir_name = std::iter::once(base.clone())
            .chain((2..).map(|number| format!("{base}-{number}")))
            .find(|name| !run_dir.join(name).exists())
            .unwrap_or(base);
        let game_dir = run_dir.join(&dir_name);
        let _ = fs::create_dir_all(&game_dir);
        let title = game.request.as_ref().map_or_else(
            |_| game.name.clone(),
            |request| request.metadata.title.clone(),
        );
        let mut entry = GameReport {
            name: game.name.clone(),
            title,
            title_id: String::new(),
            dir: dir_name,
            settings: game.label.clone(),
            build: BuildOutcome::Failed {
                error: String::new(),
            },
            scenario: None,
            log_excerpt: Vec::new(),
            failure: None,
        };
        let built = self.build(game, nsp_dir, on_event);
        let Built {
            record,
            seconds,
            size_bytes,
            reused,
            nsp,
        } = match built {
            Ok(built) => built,
            Err(error) => {
                on_event(&Event::BuildFailed {
                    error: error.clone(),
                });
                entry.build = BuildOutcome::Failed { error };
                entry.failure = entry.failure_reason();
                return entry;
            }
        };
        on_event(&Event::Built {
            title_id: record.title_id.to_string(),
            seconds,
            reused,
        });
        entry.title_id = record.title_id.to_string();
        entry.build = BuildOutcome::Built {
            seconds,
            size_bytes,
        };
        if self.build_only {
            let kept = game_dir.join(format!("{}.nsp", record.title_id));
            if fs::hard_link(&nsp, &kept).is_err() {
                on_event(&Event::BuildStage {
                    stage: format!("the NSP stays only in {}", nsp.display()),
                });
            }
            return entry;
        }
        if let Some(dir) = self.emulator.live_logs(record.title_id) {
            on_event(&Event::LaunchLogs {
                dir: dir.display().to_string(),
            });
        }
        let shared = RefCell::new(on_event);
        let baseline = history::baseline(
            &history::list(run_dir.parent().unwrap_or(run_dir)),
            &game.name,
            self.stamp.carafe_commit.as_deref(),
        );
        self.emulator.diagnose(&game.diagnostics);
        let runner = Runner::new(&mut *self.emulator, clock, record.title_id, nsp, &game_dir)
            .with_baseline(baseline, game.threshold)
            .with_input(record.settings.input.clone())
            .with_stop(self.stop.clone())
            .with_stages(|stage| (shared.borrow_mut())(&Event::Reached { stage }));
        let result = runner.run(&game.scenario, |step| {
            (shared.borrow_mut())(&Event::Step { step: step.clone() });
        });
        entry.log_excerpt = excerpt(&game_dir, &result.launches);
        entry.scenario = Some(result);
        entry.failure = entry.failure_reason();
        entry
    }

    fn build(
        &self,
        game: &Planned,
        nsp_dir: &Path,
        on_event: &mut dyn FnMut(&Event),
    ) -> Result<Built, String> {
        let previous = match &game.nsp {
            NspSource::Build => None,
            NspSource::Ready(ready) => {
                let staged = ready::stage(ready, nsp_dir)
                    .map_err(|error| format!("{}: {error}", ready.path.display()))?;
                return Ok(Built {
                    record: ready.record.clone(),
                    seconds: 0.0,
                    size_bytes: ready.size_bytes,
                    reused: true,
                    nsp: staged,
                });
            }
            NspSource::Rebuild(record) => Some(record.as_ref()),
        };
        let request = game.request.clone()?;
        let record = wizard::plan(request, previous, game.entropy, self.runtime_version)
            .map_err(|error| error.to_string())?;
        let key = build_key(&self.build_inputs, &record, &game.folder);
        let key_path = nsp_dir.join(format!("{}.build-key", record.title_id));
        let nsp_path = nsp_dir.join(format!("{}.nsp", record.title_id));
        if let (Some(key), Ok(kept), Ok(nsp)) =
            (&key, fs::read_to_string(&key_path), fs::metadata(&nsp_path))
            && kept.trim() == key
        {
            return Ok(Built {
                record,
                seconds: 0.0,
                size_bytes: nsp.len(),
                reused: true,
                nsp: nsp_path,
            });
        }
        let _ = fs::remove_file(&key_path);
        let target = PackTarget {
            keys_path: self.keys.to_string_lossy().into_owned(),
            library_dir: nsp_dir.to_string_lossy().into_owned(),
        };
        let started = Instant::now();
        let mut last_stage = None;
        let mut progress = |update: BuildProgress| {
            if last_stage != Some(update.stage) {
                last_stage = Some(update.stage);
                on_event(&Event::BuildStage {
                    stage: format!("{:?}", update.stage),
                });
            }
        };
        let stored = self
            .packer
            .pack(&record, None, &target, &CancelToken::new(), &mut progress)
            .map_err(|error| redact(&format!("{error} ({})", game.folder.display()), self.keys))?;
        if let Some(key) = key {
            let _ = fs::write(&key_path, key);
        }
        Ok(Built {
            record,
            seconds: started.elapsed().as_secs_f64(),
            size_bytes: stored.size_bytes,
            reused: false,
            nsp: nsp_path,
        })
    }
}

/// Returns a digest of everything an NSP is built from: the build record, `inputs` and the size and time of every
/// file in the game folder.
///
/// Returns `None` when any of them cannot be read, so the NSP is built again.
fn build_key(inputs: &[PathBuf], record: &BuildRecord, folder: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(record).ok()?);
    for input in inputs {
        hasher.update(file_stamp(input)?.as_bytes());
    }
    let mut pending = vec![folder.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(format!(
                    "{}|{}",
                    path.strip_prefix(folder).ok()?.display(),
                    file_stamp(&path)?
                ));
            }
        }
    }
    files.sort();
    for file in files {
        hasher.update(file.as_bytes());
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn file_stamp(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}:{}", metadata.len(), modified.as_nanos()))
}

fn excerpt(game_dir: &Path, launches: &[LaunchRecord]) -> Vec<String> {
    let mut lines = Vec::new();
    for launch in launches {
        let dir = game_dir.join(&launch.logs);
        let overlay = fs::read_to_string(dir.join("carafe-overlay.log")).unwrap_or_default();
        let runtime =
            fs::read_to_string(dir.join("logs").join("autorun_runtime.log")).unwrap_or_default();
        lines.push(format!("— launch {} —", launch.number));
        lines.extend(report::log_excerpt(&overlay, &runtime));
    }
    lines
}

fn record_event(file: Option<&mut File>, event: &Event) {
    if let (Some(file), Ok(line)) = (file, serde_json::to_string(event)) {
        let _ = writeln!(file, "{line}");
    }
}

fn write_summary(
    run_dir: &Path,
    report: &RunReport,
    planned: &[String],
    finished: bool,
) -> Result<(), String> {
    let summary = RunSummary::of(report, planned, finished);
    let json = serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?;
    fs::write(run_dir.join(SUMMARY_FILE), json).map_err(|error| error.to_string())
}

fn write_report(run_dir: &Path, report: &RunReport, lang: Lang) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|error| error.to_string())?;
    fs::write(run_dir.join("report.json"), json).map_err(|error| error.to_string())?;
    fs::write(run_dir.join("report.html"), report.html(lang)).map_err(|error| error.to_string())
}

/// Returns `name` with every character a folder name should not have replaced by `_`.
#[must_use]
pub fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_read_as_tagged_json() {
        let json = serde_json::to_string(&Event::Built {
            title_id: "0100000000001000".to_owned(),
            seconds: 0.0,
            reused: true,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"event":"built","titleId":"0100000000001000","seconds":0.0,"reused":true}"#
        );
    }

    fn failed_at(step: Step, detail: &str) -> GameReport {
        GameReport {
            name: "G".to_owned(),
            title: "G".to_owned(),
            title_id: String::new(),
            dir: "G".to_owned(),
            settings: String::new(),
            build: BuildOutcome::Built {
                seconds: 1.0,
                size_bytes: 1,
            },
            scenario: Some(crate::run::ScenarioResult {
                steps: vec![StepResult {
                    step,
                    outcome: Outcome::Fail,
                    detail: detail.to_owned(),
                    at: 1.0,
                    screenshot: None,
                    presses: Vec::new(),
                }],
                launches: Vec::new(),
                save: Vec::new(),
                emulator_gaps: Vec::new(),
            }),
            log_excerpt: Vec::new(),
            failure: None,
        }
    }

    #[test]
    fn a_failed_game_runs_again_with_logs_chosen_by_how_it_failed() {
        let wait = Step::WaitFor {
            milestone: lab_core::scenario::Milestone::FirstFrame,
            timeout: 120,
        };
        let planned = Planned {
            name: "G".to_owned(),
            folder: PathBuf::from("D:\\Games\\Game"),
            request: Ok(sample_request()),
            entropy: 1,
            label: "dxvk".to_owned(),
            scenario: Scenario::standard(120),
            threshold: 20,
            nsp: NspSource::Build,
            diagnostics: Vec::new(),
        };
        let stalled = detailed(&planned, &failed_at(wait.clone(), "not reached in 120 s")).unwrap();
        assert_eq!(stalled.name, "G · detailed");
        let settings = &stalled.request.as_ref().unwrap().settings;
        assert!(settings.debug.verbose && settings.debug.profile);
        assert_eq!(
            stalled.diagnostics,
            [
                Diagnostic::Verbose,
                Diagnostic::Profiler,
                Diagnostic::Stacks
            ]
        );
        assert_eq!(stalled.label, "dxvk · verbose, profiler, stacks");
        let crashed = detailed(
            &planned,
            &failed_at(wait, "the game crashed (exit code 0xc0000005)"),
        )
        .unwrap();
        assert!(!crashed.request.as_ref().unwrap().settings.debug.profile);
        let black = detailed(
            &planned,
            &failed_at(
                Step::Screenshot {
                    name: "menu".to_owned(),
                },
                "black",
            ),
        )
        .unwrap();
        assert_eq!(
            black.request.as_ref().unwrap().settings.graphics.hud,
            carafe_core::settings::DxvkHud::Full
        );
        let mut passed = failed_at(Step::Launch, "");
        if let Some(scenario) = passed.scenario.as_mut() {
            scenario.steps[0].outcome = Outcome::Pass;
        }
        assert!(detailed(&planned, &passed).is_none());
    }

    #[test]
    fn folder_names_keep_only_safe_characters() {
        assert_eq!(safe_name("Need for Speed: U2"), "Need_for_Speed__U2");
    }

    #[test]
    fn a_missing_input_makes_no_key() {
        let record_missing = build_key(
            &[PathBuf::from("Z:\\nowhere\\carafe-runtime.bin")],
            &sample_record(),
            Path::new("."),
        );
        assert_eq!(record_missing, None);
    }

    fn sample_record() -> BuildRecord {
        wizard::plan(sample_request(), None, 1, "dev").unwrap()
    }

    fn sample_request() -> BuildRequest {
        BuildRequest {
            title_id: None,
            source: carafe_core::record::GameSource {
                folder: "D:\\Games\\Game".to_owned(),
                executable: "game.exe".to_owned(),
                arch: carafe_core::record::Arch::X86,
                arch_override: None,
                arguments: Vec::new(),
            },
            metadata: carafe_core::metadata::Metadata::new("Game".to_owned()),
            settings: carafe_core::settings::AutorunSettings::default(),
            icon: None,
        }
    }
}
