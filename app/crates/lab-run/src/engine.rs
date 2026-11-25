//! A whole run: builds every planned game with Carafe's packer, runs its scenario in the emulator, writes
//! `report.json` and `report.html` after every game and tells the front end what happens as [`Event`]s.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Instant, UNIX_EPOCH};

use carafe_core::BuildRecord;
use carafe_core::cancel::CancelToken;
use carafe_core::ports::{BuildProgress, PackTarget, Packer};
use carafe_core::wizard::{self, BuildRequest};
use lab_core::diagnostics::{self, Diagnostic};
use lab_core::emulator::EdenCpu;
use lab_core::lang::Lang;
use lab_core::milestones::Stage;
use lab_core::redact::redact;
use lab_core::scenario::{Scenario, Step};
use serde::Serialize;
use ts_rs::TS;

use crate::history::{self, EVENTS_FILE, RunSummary, SUMMARY_FILE};
use crate::ready::{self, ReadyNsp};
use crate::report::{self, BuildOutcome, GameReport, RunReport, RunStamp};
use crate::run::{Clock, Emulator, HOLD_FILE, LaunchRecord, Outcome, Runner, StepResult};

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
    /// The folder whose files become its save data before the first launch; `None` starts it without save data.
    pub save: Option<PathBuf>,
    /// Settings changes its launches find as made on the console's settings menu; see
    /// [`Emulator::console_changes`].
    pub console_changes: Option<String>,
    /// How the emulator runs its processor code; `None` keeps the emulator's own choice.
    pub emulator_cpu: Option<EdenCpu>,
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
    /// Built from the request with the other runtime of an A/B run between runtimes, into `nsp/alternate`.
    Alternate,
}

/// The other runtime an A/B run between runtimes compares the lab's with.
#[derive(Clone)]
pub struct Alternate<'a> {
    /// The packer that builds with it.
    pub packer: &'a dyn Packer,
    /// Files whose change makes its NSPs stale: its archive and hacBrewPack.
    pub build_inputs: Vec<PathBuf>,
    /// The runtime version its build records carry, such as the release tag.
    pub runtime_version: String,
}

/// The folder inside the NSP folder for the NSPs built with the other runtime of an A/B run.
pub const ALTERNATE_DIR: &str = "alternate";

/// The folder inside the work folder where each run builds its NSPs, in a folder named as the run's; the run removes
/// it when it ends.
pub const NSP_DIR: &str = "nsp";

/// The file in a game's run folder with every line of its NSP build, each with the seconds since the build started.
pub const BUILD_LOG_FILE: &str = "build.log";

struct Built {
    record: BuildRecord,
    seconds: f64,
    size_bytes: u64,
    reused: bool,
    nsp: PathBuf,
}

/// A game's NSP built ahead of its turn: the outcome, the events of the build in order and its log.
struct Prebuilt {
    built: Result<Built, String>,
    events: Vec<Event>,
    log: String,
}

/// What building an NSP takes, apart from the emulator, so the next game can be built while one is checked.
struct Builder<'a> {
    packer: &'a dyn Packer,
    keys: &'a Path,
    build_inputs: Vec<PathBuf>,
    runtime_version: &'a str,
    alternate: Option<Alternate<'a>>,
    stop: CancelToken,
}

/// Returns the short name of the NSP folder of the run in `folder`, such as `31011410` for `2026-10-31_01-14-10`:
/// the day and the time, so the build paths of a game stay short.
#[must_use]
pub fn nsp_folder(folder: &str) -> String {
    let digits: String = folder.chars().filter(char::is_ascii_digit).collect();
    let skip = digits.len().saturating_sub(8);
    digits[skip..].to_owned()
}

const SCRATCH_PREFIX: &str = "lab-run-";
const SCRATCH_LEASE: &str = ".lab-run-lease";
const SCRATCH_OWNER: &str = ".lab-run-managed";
const PENDING_MARKER_PREFIX: &str = ".lab-pending-";
const LAUNCH_MARKER: &str = ".lab-pending-launch";
const SCRATCH_PAYLOAD: &str = "nsp";

struct Scratch {
    dir: PathBuf,
    _lease: File,
}

impl Scratch {
    fn start(root: &Path, folder: &str) -> std::io::Result<Self> {
        fs::create_dir_all(root)?;
        let prefix = format!(
            "{SCRATCH_PREFIX}{}-{}",
            nsp_folder(folder),
            std::process::id()
        );
        for attempt in 0.. {
            let dir = root.join(format!("{prefix}-{attempt}"));
            match fs::create_dir(&dir) {
                Ok(()) => {
                    let lease = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(dir.join(SCRATCH_LEASE))?;
                    lease.lock()?;
                    fs::create_dir(dir.join(SCRATCH_PAYLOAD))?;
                    File::create(dir.join(SCRATCH_OWNER))?;
                    return Ok(Self { dir, _lease: lease });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        unreachable!()
    }

    fn collect(root: &Path, protected: &[PathBuf]) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(SCRATCH_PREFIX)
                || !entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
                || !plain_directory(&path)
                || !path.join(SCRATCH_OWNER).is_file()
            {
                continue;
            }
            if !path
                .canonicalize()
                .is_ok_and(|resolved| !protected.iter().any(|input| input.starts_with(&resolved)))
            {
                continue;
            }
            if let Ok(lease) = OpenOptions::new()
                .write(true)
                .open(path.join(SCRATCH_LEASE))
                && lease.try_lock().is_ok()
                && no_pending_work(&path)
            {
                let _ = Self::remove(&path);
            }
        }
    }

    fn remove(path: &Path) -> std::io::Result<()> {
        match fs::remove_dir_all(path.join(SCRATCH_PAYLOAD)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::remove_dir_all(path)
    }
}

fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const REPARSE_POINT: u32 = 0x400;
            metadata.is_dir() && metadata.file_attributes() & REPARSE_POINT == 0
        }
        #[cfg(not(windows))]
        {
            metadata.is_dir()
        }
    })
}

fn no_pending_work(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| {
        entries.all(|entry| {
            entry.is_ok_and(|item| {
                !item
                    .file_name()
                    .to_string_lossy()
                    .starts_with(PENDING_MARKER_PREFIX)
            })
        })
    })
}

fn start_build_marker(dir: &Path) -> std::io::Result<PathBuf> {
    for attempt in 0.. {
        let marker = dir.join(format!("{PENDING_MARKER_PREFIX}{attempt}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
            Ok(_) => return Ok(marker),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

fn keep_console_nsp(source: &Path, destination: &Path) -> std::io::Result<()> {
    if fs::hard_link(source, destination).is_ok() {
        return Ok(());
    }
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = std::io::copy(&mut input, &mut output).map(|_| ());
    drop(output);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

/// What the run is doing, for the window, the terminal and `--json`.
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
        /// Seconds since the build started; `None` for a note after the build.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        at: Option<f64>,
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
    /// The work folder: `runs/` inside it.
    pub work: &'a Path,
    /// The folder runs build their NSPs in, each in a folder of its own.
    pub nsp_root: PathBuf,
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
    /// The other runtime of an A/B run between runtimes; `None` for any other run.
    pub alternate: Option<Alternate<'a>>,
    /// Runs a game that failed once more, with detailed logs chosen by how it failed.
    pub retry_failed: bool,
    /// The folder of the reference pictures scenarios compare the screen with.
    pub references: PathBuf,
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
        fs::create_dir_all(&run_dir).map_err(|error| format!("{}: {error}", run_dir.display()))?;
        let inputs: std::io::Result<Vec<_>> = planned
            .iter()
            .filter_map(|game| match &game.nsp {
                NspSource::Ready(ready) => Some(ready.path.canonicalize()),
                _ => None,
            })
            .collect();
        if !self.build_only
            && self.emulator.nsp_mount_released()
            && let Ok(inputs) = &inputs
        {
            Scratch::collect(&self.nsp_root, inputs);
        }
        let _ = fs::remove_file(self.work.join("queue").join(HOLD_FILE));
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
        let scratch = Scratch::start(&self.nsp_root, folder)
            .map_err(|error| format!("{}: {error}", self.nsp_root.display()))?;
        on_event(&Event::RunStarted {
            dir: run_dir.display().to_string(),
            games: total,
        });
        let builder = self.builder();
        let result = thread::scope(|scope| -> Result<(), String> {
            let mut next = None;
            for (index, game) in planned.iter().enumerate() {
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
                let prebuilt = next
                    .take()
                    .and_then(|building: thread::ScopedJoinHandle<Prebuilt>| building.join().ok())
                    .unwrap_or_else(|| builder.build(game, &scratch));
                if !self.build_only
                    && let Some(following) = planned.get(index + 1)
                {
                    let builder = &builder;
                    let scratch = &scratch;
                    next = Some(scope.spawn(move || builder.build(following, scratch)));
                }
                let entry = self.one_game(game, prebuilt, &run_dir, &scratch, clock, &mut on_event);
                on_event(&Event::GameFinished {
                    name: entry.name.clone(),
                    outcome: entry.outcome(),
                });
                let again = if self.retry_failed && !self.build_only {
                    detailed(game, &entry)
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
                    let prebuilt = builder.build(&again, &scratch);
                    let entry =
                        self.one_game(&again, prebuilt, &run_dir, &scratch, clock, &mut on_event);
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
            Ok(())
        });
        if !self.build_only {
            self.emulator.finish();
        }
        if self.build_only || self.emulator.nsp_mount_released() {
            if let Err(error) = Scratch::remove(&scratch.dir) {
                on_event(&Event::BuildStage {
                    stage: format!("temporary NSP cleanup: {error}"),
                    at: None,
                });
            }
            if !self.build_only
                && let Ok(inputs) = &inputs
            {
                Scratch::collect(&self.nsp_root, inputs);
            }
        }
        result?;
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
        prebuilt: Prebuilt,
        run_dir: &Path,
        scratch: &Scratch,
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
        for event in &prebuilt.events {
            on_event(event);
        }
        if !prebuilt.log.is_empty() {
            let _ = fs::write(game_dir.join(BUILD_LOG_FILE), &prebuilt.log);
        }
        let Built {
            record,
            seconds,
            size_bytes,
            reused,
            nsp,
        } = match prebuilt.built {
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
            if let Err(error) = keep_console_nsp(&nsp, &kept) {
                entry.build = BuildOutcome::Failed {
                    error: format!("{}: {error}", kept.display()),
                };
                entry.failure = entry.failure_reason();
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
        self.emulator.use_cpu(game.emulator_cpu);
        self.emulator
            .console_changes(game.console_changes.as_deref());
        let runner = Runner::new(&mut *self.emulator, clock, record.title_id, nsp, &game_dir)
            .with_launch_marker(scratch.dir.join(LAUNCH_MARKER))
            .with_baseline(baseline, game.threshold)
            .with_input(record.settings.input.clone())
            .with_stop(self.stop.clone())
            .with_hold(self.work.join("queue").join(HOLD_FILE))
            .with_references(self.references.clone())
            .with_save(game.save.clone())
            .with_stages(|stage| (shared.borrow_mut())(&Event::Reached { stage }));
        let result = runner.run(&game.scenario, |step| {
            (shared.borrow_mut())(&Event::Step { step: step.clone() });
        });
        entry.log_excerpt = excerpt(&game_dir, &result.launches);
        entry.scenario = Some(result);
        entry.failure = entry.failure_reason();
        entry
    }
}

impl<'a> Engine<'a> {
    fn builder(&self) -> Builder<'a> {
        Builder {
            packer: self.packer,
            keys: self.keys,
            build_inputs: self.build_inputs.clone(),
            runtime_version: self.runtime_version,
            alternate: self.alternate.clone(),
            stop: self.stop.clone(),
        }
    }
}

impl Builder<'_> {
    fn build(&self, game: &Planned, scratch: &Scratch) -> Prebuilt {
        let mut events = Vec::new();
        let mut log = String::new();
        let built = match start_build_marker(&scratch.dir) {
            Ok(marker) => {
                let built = self.pack(
                    game,
                    &scratch.dir.join(SCRATCH_PAYLOAD),
                    &mut events,
                    &mut log,
                );
                let _ = fs::remove_file(marker);
                built
            }
            Err(error) => Err(format!("{}: {error}", scratch.dir.display())),
        };
        Prebuilt { built, events, log }
    }

    fn pack(
        &self,
        game: &Planned,
        nsp_dir: &Path,
        events: &mut Vec<Event>,
        log: &mut String,
    ) -> Result<Built, String> {
        let previous = match &game.nsp {
            NspSource::Build | NspSource::Alternate => None,
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
        let (packer, build_inputs, runtime_version, nsp_dir) = match (&game.nsp, &self.alternate) {
            (NspSource::Alternate, Some(alternate)) => {
                let dir = nsp_dir.join(ALTERNATE_DIR);
                fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
                (
                    alternate.packer,
                    &alternate.build_inputs,
                    alternate.runtime_version.as_str(),
                    dir,
                )
            }
            (NspSource::Alternate, None) => {
                return Err("the run has no other runtime to build with".to_owned());
            }
            _ => (
                self.packer,
                &self.build_inputs,
                self.runtime_version,
                nsp_dir.to_path_buf(),
            ),
        };
        let nsp_dir = nsp_dir.as_path();
        let request = game.request.clone()?;
        let record = wizard::plan(request, previous, game.entropy, runtime_version)
            .map_err(|error| error.to_string())?;
        let key = build_key(build_inputs, &record, &game.folder);
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
        let packed = packer.pack(
            &record,
            None,
            &target,
            &self.stop,
            &mut |update: BuildProgress| {
                let at = started.elapsed().as_secs_f64();
                let stage = format!("{:?}", update.stage);
                if last_stage != Some(update.stage) {
                    last_stage = Some(update.stage);
                    events.push(Event::BuildStage {
                        stage: stage.clone(),
                        at: Some(at),
                    });
                }
                if let Some(line) = &update.log {
                    log.push_str(&format!(
                        "{at:8.2} s  {stage:<8}  {}\n",
                        redact(line, self.keys)
                    ));
                }
            },
        );
        let stored = packed
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
    fn a_run_builds_in_a_folder_named_by_its_day_and_time() {
        assert_eq!(nsp_folder("2026-10-31_01-14-10"), "31011410");
        assert_eq!(nsp_folder("2026-10-31_01-14-10 · B"), "31011410");
    }

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
            save: None,
            console_changes: None,
            emulator_cpu: None,
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
    fn recovery_only_collects_managed_folders_without_active_consumers() {
        let root = std::env::temp_dir().join(format!("lab-nsp-recovery-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let active = Scratch::start(&root, "2026-11-25_11-00-00").unwrap();
        let orphan = Scratch::start(&root, "2026-11-25_11-00-00").unwrap();
        let building = Scratch::start(&root, "2026-11-25_11-00-00").unwrap();
        let orphan_path = orphan.dir.clone();
        let building_path = building.dir.clone();
        let build_marker = start_build_marker(&building_path).unwrap();
        let original = root.join("original.nsp");
        fs::write(&original, "user input").unwrap();
        let staged = orphan_path.join(SCRATCH_PAYLOAD).join("staged.nsp");
        fs::write(&staged, "temporary data").unwrap();
        drop(orphan);
        drop(building);

        Scratch::collect(&root, &[staged.canonicalize().unwrap()]);
        assert!(staged.exists());
        Scratch::collect(&root, &[]);
        assert!(active.dir.is_dir());
        assert!(building_path.is_dir());
        assert!(!orphan_path.exists());
        assert_eq!(fs::read_to_string(original).unwrap(), "user input");

        fs::remove_file(build_marker).unwrap();
        drop(active);
        Scratch::collect(&root, &[]);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn console_output_preserves_existing_files() {
        let root = std::env::temp_dir().join(format!("lab-console-nsp-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let input = root.join("temporary.nsp");
        let output = root.join("console.nsp");
        fs::write(&input, "new").unwrap();
        fs::write(&output, "old").unwrap();
        assert!(keep_console_nsp(&input, &output).is_err());
        assert_eq!(fs::read_to_string(&input).unwrap(), "new");
        assert_eq!(fs::read_to_string(&output).unwrap(), "old");
        fs::remove_file(&output).unwrap();
        keep_console_nsp(&input, &output).unwrap();
        fs::remove_file(input).unwrap();
        assert_eq!(fs::read_to_string(output).unwrap(), "new");
        fs::remove_dir_all(root).unwrap();
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
