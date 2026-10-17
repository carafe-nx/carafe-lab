//! Running a scenario against an emulator and recording what each step found.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carafe_core::TitleId;
use carafe_core::cancel::CancelToken;
use carafe_core::settings::{InputSettings, SwitchButton};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use lab_core::crash;
use lab_core::diagnostics::Diagnostic;
use lab_core::frames::{self, FrameSummary};
use lab_core::input_check::{self, PressCheck};
use lab_core::lang::Lang;
use lab_core::milestones::{Exit, Launch, Stage};
use lab_core::perf::{self, PerfSample, PerfSummary};
use lab_core::picture::{self, Change, PatternCheck, Thumbnail};
use lab_core::problems::{self, Problem, ProblemCounts};
use lab_core::resources::{self, ResourceSample, ResourceSummary};
use lab_core::scenario::{Milestone, Scenario, Step};
use lab_core::timeline;
use lab_core::versions::{self, VersionPerf};

use crate::report;

const POLL: Duration = Duration::from_millis(500);
const GLIMPSE_EVERY: Duration = Duration::from_secs(10);

/// Name of the folder in a launch's log folder with a small picture of the screen every ten seconds.
pub const TIMELINE_DIR: &str = "timeline";

/// The file in a launch's folder with the stacks of the game's threads at the moment it was found hung.
pub const BACKTRACES_FILE: &str = "backtraces.txt";

/// The file in the queue folder whose presence holds the running scenario while a person plays.
pub const HOLD_FILE: &str = "hold";

/// Lines of an emulator log, in lower case, that report something missing on purpose: the loader asks for
/// Atmosphère's `svcGetInfo` 65000 only to learn whether the kernel is Mesosphère and goes on without it.
const EXPECTED_GAPS: [&str; 1] = ["unimplemented svcgetinfo id=0x000000000000fde8"];

/// Name of the log the input probe writes.
pub const PROBE_INPUT_LOG: &str = "probe-input.log";

/// An error of the emulator or of the files around it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmulatorError {
    /// The emulator could not be started or controlled.
    #[error("emulator: {0}")]
    Control(String),
    /// A file of the run could not be read or written.
    #[error("file: {0}")]
    Io(String),
}

/// How a game ended after [`Emulator::close`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Closed {
    /// It ended by itself after the close request.
    ByRequest,
    /// It did not end in time and was stopped.
    Forced,
}

/// A file of the game's save data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFile {
    /// Path inside the save data, with `/`.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 of the contents in hex.
    pub sha256: String,
}

/// The logs of the current launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameLogs {
    /// `carafe-overlay.log`.
    pub overlay: String,
    /// `logs/autorun_runtime.log`.
    pub runtime: String,
}

/// An emulator running one game at a time.
pub trait Emulator {
    /// Starts the NSP `nsp`.
    ///
    /// # Errors
    ///
    /// [`EmulatorError`] if the emulator does not start.
    fn launch(&mut self, nsp: &Path) -> Result<(), EmulatorError>;

    /// Returns whether the game still runs.
    fn running(&mut self) -> bool;

    /// Returns why the emulator itself ended, once [`Emulator::running`] has seen it end; `None` if it did not
    /// end or the reason is unknown.
    fn end_reason(&self) -> Option<String> {
        None
    }

    /// Holds `buttons` together for `hold`, then releases them.
    ///
    /// # Errors
    ///
    /// [`EmulatorError::Control`] if the input does not reach the emulator.
    fn press(&mut self, buttons: &[SwitchButton], hold: Duration) -> Result<(), EmulatorError>;

    /// Saves a PNG picture of the screen to `dest`.
    ///
    /// # Errors
    ///
    /// [`EmulatorError`] if the picture cannot be taken or written.
    fn screenshot(&mut self, dest: &Path) -> Result<(), EmulatorError>;

    /// Asks the game to close as the HOME Menu does; stops it by force after `timeout`.
    ///
    /// # Errors
    ///
    /// [`EmulatorError::Control`] if the emulator cannot be reached.
    fn close(&mut self, timeout: Duration) -> Result<Closed, EmulatorError>;

    /// Returns the logs of the current launch of `title`; empty texts for missing files.
    fn logs(&self, title: TitleId) -> GameLogs;

    /// Returns the folder the game's logs appear in while it runs, when the emulator writes them to the host.
    fn live_logs(&self, _title: TitleId) -> Option<PathBuf> {
        None
    }

    /// Moves the logs of `title` from the emulator's SD card into `dest`, so the next launch starts without them.
    ///
    /// # Errors
    ///
    /// [`EmulatorError::Io`] if the files cannot be moved.
    fn take_logs(&mut self, title: TitleId, dest: &Path) -> Result<(), EmulatorError>;

    /// Empties the device save data of `title`, so a scenario starts like a first launch.
    ///
    /// # Errors
    ///
    /// [`EmulatorError::Io`] if the save data cannot be removed.
    fn clear_save(&mut self, title: TitleId) -> Result<(), EmulatorError>;

    /// Returns the files of the device save data of `title`, sorted by path; empty if there is none.
    fn save_files(&self, title: TitleId) -> Vec<SaveFile>;

    /// Returns the emulator's own log of the current or last launch.
    fn emulator_log(&self) -> String;

    /// Returns the text of the first file named `name` the game of `title` has written to its save data or the
    /// SD card; `None` if there is none.
    fn find_file(&self, title: TitleId, name: &str) -> Option<String>;

    /// Ends whatever the emulator keeps running between games, once the run has no more games for it.
    fn finish(&mut self) {}

    /// Returns the emulator's performance readings since the current or last launch started; empty when it does
    /// not tell them.
    fn launch_perf(&self) -> Vec<PerfSample> {
        Vec::new()
    }

    /// Saves a small, cheap picture of the screen to `dest`, for the timeline of a launch; does nothing when the
    /// emulator cannot.
    ///
    /// # Errors
    ///
    /// [`EmulatorError`] if the picture cannot be taken or saved.
    fn glimpse(&mut self, _dest: &Path) -> Result<(), EmulatorError> {
        Ok(())
    }

    /// Returns the emulator's memory and processor readings since the current or last launch started; empty when it
    /// does not tell them.
    fn launch_resources(&self) -> Vec<ResourceSample> {
        Vec::new()
    }

    /// Turns on the emulator's diagnostics in `diagnostics` for the next launches and turns off the others; those
    /// that are not the emulator's, or that it does not have, are left alone.
    fn diagnose(&mut self, _diagnostics: &[Diagnostic]) {}

    /// Returns the stacks of every thread of the running game and the modules it has loaded, read without stopping
    /// it for good; `None` when [`Diagnostic::Stacks`] is off or the stacks cannot be read.
    fn backtraces(&mut self) -> Option<String> {
        None
    }
}

/// Time for the runner: real in a run, simulated in tests.
pub trait Clock {
    /// Returns the time since an arbitrary start.
    fn now(&self) -> Duration;
    /// Waits `duration`.
    fn sleep(&self, duration: Duration);
}

/// What a step found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Outcome {
    /// It went as expected.
    Pass,
    /// Not done: an earlier step failed.
    Skip,
    /// The emulator cannot show this; check it on the console.
    Console,
    /// Unexpected, but not a failure.
    Warn,
    /// It failed.
    Fail,
}

/// What a step found, as written into the report.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepResult {
    /// The step.
    pub step: Step,
    /// What it found.
    pub outcome: Outcome,
    /// Details in English for the report and the console.
    pub detail: String,
    /// Seconds since the scenario started, when the step ended.
    pub at: f64,
    /// File name of the screenshot the step took or compared, inside the game's folder of the run.
    pub screenshot: Option<String>,
    /// What each button press gave, for [`Step::ExpectInput`]; empty for other steps.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub presses: Vec<PressCheck>,
}

/// One launch of the game within a scenario.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecord {
    /// Number of the launch, from one.
    pub number: usize,
    /// Its milestones.
    pub launch: Launch,
    /// Folder with its logs, relative to the game's folder of the run.
    pub logs: String,
    /// The emulator's performance while the game drew; `None` when the emulator does not tell it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perf: Option<PerfSummary>,
    /// The container's peak memory and processor use over the launch; `None` when the emulator does not tell it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceSummary>,
    /// Error and warning lines in the launch's logs, listed in its `problems.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problems: Option<ProblemCounts>,
    /// The times of the launch's frames in short, from its `frametimes.csv`; `None` when the emulator recorded none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<FrameSummary>,
    /// The crash Eden logged during the launch in one line, kept whole in its `crash.txt`; `None` when none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash: Option<String>,
}

/// Everything a scenario found for one game.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioResult {
    /// Steps in order.
    pub steps: Vec<StepResult>,
    /// Launches in order.
    pub launches: Vec<LaunchRecord>,
    /// Save data files after the last step.
    pub save: Vec<SaveFile>,
    /// Lines of the emulator log that point at something the emulator does not implement.
    pub emulator_gaps: Vec<String>,
}

impl ScenarioResult {
    /// Returns the worst outcome of the steps; [`Outcome::Pass`] if there are none.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        self.steps
            .iter()
            .map(|step| step.outcome)
            .max()
            .unwrap_or(Outcome::Pass)
    }
}

/// Runs scenarios: owns the emulator for one game.
pub struct Runner<'a> {
    emulator: &'a mut dyn Emulator,
    clock: &'a dyn Clock,
    title: TitleId,
    nsp: PathBuf,
    dir: PathBuf,
    started: Duration,
    running: bool,
    launch_open: bool,
    launches: Vec<LaunchRecord>,
    remembered: Option<Vec<SaveFile>>,
    screenshots: Vec<(String, String)>,
    input: InputSettings,
    stop: Option<CancelToken>,
    pressed: Vec<SwitchButton>,
    stage: Stage,
    on_stage: Option<Box<dyn FnMut(Stage) + 'a>>,
    launch_started: Duration,
    launched_ms: Option<u64>,
    lab_lines: Vec<(Duration, String)>,
    hold: Option<PathBuf>,
    references: Option<PathBuf>,
    last_glimpse: Duration,
    baseline: Option<VersionPerf>,
    threshold: u32,
}

impl<'a> Runner<'a> {
    /// Creates a runner for the game `title` built into `nsp`; screenshots and logs go into `dir`.
    pub fn new(
        emulator: &'a mut dyn Emulator,
        clock: &'a dyn Clock,
        title: TitleId,
        nsp: impl Into<PathBuf>,
        dir: impl Into<PathBuf>,
    ) -> Self {
        let started = clock.now();
        Self {
            emulator,
            clock,
            title,
            nsp: nsp.into(),
            dir: dir.into(),
            started,
            running: false,
            launch_open: false,
            launches: Vec::new(),
            remembered: None,
            screenshots: Vec::new(),
            input: InputSettings::default(),
            pressed: Vec::new(),
            stop: None,
            stage: Stage::NotStarted,
            on_stage: None,
            launch_started: started,
            launched_ms: None,
            lab_lines: Vec::new(),
            hold: None,
            references: None,
            last_glimpse: started,
            baseline: None,
            threshold: versions::DEFAULT_THRESHOLD,
        }
    }

    /// Returns the runner checking [`Step::ExpectPerformance`] against `baseline`, the game at the newest other
    /// Carafe version, with `threshold` percent as the game's own threshold.
    #[must_use]
    pub fn with_baseline(mut self, baseline: Option<VersionPerf>, threshold: u32) -> Self {
        self.baseline = baseline;
        self.threshold = threshold;
        self
    }

    fn save_backtraces(&mut self) -> bool {
        let Some(record) = self.launches.last() else {
            return false;
        };
        let file = self.dir.join(&record.logs).join(BACKTRACES_FILE);
        let Some(text) = self.emulator.backtraces() else {
            return false;
        };
        file.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(&file, text).is_ok()
    }

    fn glimpse(&mut self) {
        let now = self.clock.now();
        if !self.launch_open || !self.running || now - self.last_glimpse < GLIMPSE_EVERY {
            return;
        }
        self.last_glimpse = now;
        let Some(record) = self.launches.last() else {
            return;
        };
        let seconds = (now - self.launch_started).as_secs();
        let file = self
            .dir
            .join(&record.logs)
            .join(TIMELINE_DIR)
            .join(format!("{seconds:04}s.jpg"));
        let _ = self.emulator.glimpse(&file);
    }

    /// Returns the runner telling `on_stage` every start-up stage a launch reaches while a step waits for it.
    #[must_use]
    pub fn with_stages(mut self, on_stage: impl FnMut(Stage) + 'a) -> Self {
        self.on_stage = Some(Box::new(on_stage));
        self
    }

    /// Sets the input settings the game was built with; [`Step::ExpectInput`] checks against them.
    #[must_use]
    pub fn with_input(mut self, input: InputSettings) -> Self {
        self.input = input;
        self
    }

    /// Sets the token that stops the run: once it is cancelled, the waiting step ends and the rest are skipped.
    #[must_use]
    pub fn with_stop(mut self, stop: CancelToken) -> Self {
        self.stop = Some(stop);
        self
    }

    fn stopped(&self) -> bool {
        self.stop.as_ref().is_some_and(CancelToken::is_cancelled)
    }

    /// Sets the folder the reference pictures of [`Step::ExpectScreen`] are in.
    #[must_use]
    pub fn with_references(mut self, references: impl Into<PathBuf>) -> Self {
        self.references = Some(references.into());
        self
    }

    /// Sets the file whose presence holds the scenario: while it is there, no step starts and a milestone's wait does
    /// not count, so a person can play and hand the game back.
    #[must_use]
    pub fn with_hold(mut self, hold: impl Into<PathBuf>) -> Self {
        self.hold = Some(hold.into());
        self
    }

    fn held(&self) -> bool {
        self.hold.as_ref().is_some_and(|hold| hold.exists())
    }

    fn wait_while_held(&mut self) {
        if !self.held() {
            return;
        }
        self.lab_lines.push((
            self.clock.now(),
            "taken by hand: the scenario waits".to_owned(),
        ));
        while self.held() && !self.stopped() {
            self.glimpse();
            self.clock.sleep(POLL);
        }
        self.lab_lines.push((
            self.clock.now(),
            "handed back: the scenario goes on".to_owned(),
        ));
    }

    /// Runs `scenario` step by step.
    ///
    /// After a failed step the steps that need the game are skipped until the next launch; the save data checks
    /// still run. The game is always closed at the end and the logs of every launch are kept.
    pub fn run(
        mut self,
        scenario: &Scenario,
        mut on_step: impl FnMut(&StepResult),
    ) -> ScenarioResult {
        let mut steps = Vec::with_capacity(scenario.steps.len());
        let mut broken = false;
        if let Err(error) = self.emulator.clear_save(self.title) {
            broken = true;
            let result = self.result(
                &Step::ExpectSave,
                Outcome::Fail,
                format!("old save data stays: {error}"),
                None,
            );
            on_step(&result);
            steps.push(result);
        }
        for step in &scenario.steps {
            let independent = matches!(
                step,
                Step::Launch
                    | Step::ExpectSave
                    | Step::ExpectSaveKept
                    | Step::ExpectPerformance { .. }
            );
            self.wait_while_held();
            let result = if self.stopped() {
                self.result(step, Outcome::Skip, "the run was stopped".to_owned(), None)
            } else if broken && !independent {
                self.result(
                    step,
                    Outcome::Skip,
                    "an earlier step failed".to_owned(),
                    None,
                )
            } else {
                self.step(step)
            };
            if matches!(step, Step::Launch) {
                broken = false;
            }
            if result.outcome == Outcome::Fail {
                broken = true;
            }
            self.lab_lines.push((
                self.started + Duration::from_secs_f64(result.at),
                format!(
                    "{}: {:?}, {}",
                    report::step_title(&result, Lang::En),
                    result.outcome,
                    result.detail
                ),
            ));
            on_step(&result);
            steps.push(result);
        }
        if self.running {
            let _ = self.emulator.close(Duration::from_secs(30));
            self.running = false;
        }
        self.finish_launch();
        let save = self.emulator.save_files(self.title);
        let emulator_gaps = emulator_gaps(&self.emulator.emulator_log());
        if !emulator_gaps.is_empty() {
            for step in &mut steps {
                if step.outcome == Outcome::Fail && matches!(step.step, Step::WaitFor { .. }) {
                    step.outcome = Outcome::Console;
                    step.detail
                        .push_str("; the emulator lacks something the runtime needs");
                }
            }
        }
        ScenarioResult {
            steps,
            launches: self.launches,
            save,
            emulator_gaps,
        }
    }

    fn step(&mut self, step: &Step) -> StepResult {
        match step {
            Step::Launch => self.launch(step),
            Step::WaitFor { milestone, timeout } => self.wait_for(step, *milestone, *timeout),
            Step::Wait { seconds } => {
                let until = self.clock.now() + Duration::from_secs_f32(*seconds);
                while self.clock.now() < until {
                    if self.stopped() {
                        return self.result(
                            step,
                            Outcome::Skip,
                            "the run was stopped".to_owned(),
                            None,
                        );
                    }
                    self.glimpse();
                    self.clock
                        .sleep(POLL.min(until.saturating_sub(self.clock.now())));
                }
                self.result(step, Outcome::Pass, format!("waited {seconds} s"), None)
            }
            Step::Press { buttons, hold } => {
                let names: Vec<&str> = buttons.iter().map(|b| b.autorun_name()).collect();
                match self.emulator.press(buttons, Duration::from_secs_f32(*hold)) {
                    Ok(()) => {
                        if let [button] = buttons.as_slice() {
                            self.pressed.push(*button);
                        }
                        self.result(
                            step,
                            Outcome::Pass,
                            format!("pressed {}", names.join("+")),
                            None,
                        )
                    }
                    Err(error) => self.result(step, Outcome::Fail, error.to_string(), None),
                }
            }
            Step::Screenshot { name } => self.screenshot(step, name),
            Step::ExpectScreen {
                name,
                reference,
                tolerance,
            } => self.expect_screen(step, name, reference, *tolerance),
            Step::ExpectChange {
                from,
                to,
                soft,
                quiet,
            } => self.expect_change(step, from, to, *soft, quiet),
            Step::ExpectSave => {
                let files = self.emulator.save_files(self.title);
                if files.is_empty() {
                    self.remembered = Some(files);
                    self.result(
                        step,
                        Outcome::Warn,
                        "the game has written no save data".to_owned(),
                        None,
                    )
                } else {
                    let bytes: u64 = files.iter().map(|file| file.size).sum();
                    let detail = format!("{} files, {bytes} bytes", files.len());
                    self.remembered = Some(files);
                    self.result(step, Outcome::Pass, detail, None)
                }
            }
            Step::ExpectSaveKept => self.expect_save_kept(step),
            Step::ExpectPerformance { threshold } => {
                self.expect_performance(step, threshold.unwrap_or(self.threshold))
            }
            Step::ExpectPattern { name } => self.expect_pattern(step, name),
            Step::ExpectInput => self.expect_input(step),
            Step::RandomPresses { seconds, seed } => self.random_presses(step, *seconds, *seed),
            Step::Close { timeout } => self.close(step, *timeout),
        }
    }

    fn expect_pattern(&mut self, step: &Step, name: &str) -> StepResult {
        let Some(file) = self
            .screenshots
            .iter()
            .find(|(shot, _)| shot == name)
            .map(|(_, file)| file.clone())
        else {
            return self.result(step, Outcome::Fail, format!("no picture {name}"), None);
        };
        let check = std::fs::read(self.dir.join(&file))
            .map_err(|error| error.to_string())
            .and_then(|png| PatternCheck::from_png(&png).map_err(|error| error.to_string()));
        match check {
            Ok(check) if check.matches_probe() => self.result(
                step,
                Outcome::Pass,
                "the four quadrants are right".to_owned(),
                Some(file),
            ),
            Ok(check) => {
                let seen: Vec<String> = check
                    .quadrants
                    .iter()
                    .map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
                    .collect();
                let detail = format!("the quadrants are {}", seen.join(" "));
                self.result(step, Outcome::Fail, detail, Some(file))
            }
            Err(error) => self.result(step, Outcome::Fail, error, Some(file)),
        }
    }

    fn expect_input(&mut self, step: &Step) -> StepResult {
        let Some(log) = self.emulator.find_file(self.title, PROBE_INPUT_LOG) else {
            return self.result(
                step,
                Outcome::Fail,
                format!("the probe wrote no {PROBE_INPUT_LOG}"),
                None,
            );
        };
        let presses = input_check::check(&self.input, &self.pressed, &log);
        let wrong: Vec<String> = presses
            .iter()
            .filter(|press| !press.ok())
            .map(|press| {
                format!(
                    "{}: expected {}, got {}",
                    press.button.autorun_name(),
                    input_check::describe(&press.expected),
                    input_check::describe(&press.observed)
                )
            })
            .collect();
        let mut result = if wrong.is_empty() {
            let detail = format!("all {} presses gave what the bindings say", presses.len());
            self.result(step, Outcome::Pass, detail, None)
        } else {
            let detail = format!(
                "{} of {} presses differ; {}",
                wrong.len(),
                presses.len(),
                wrong.join("; ")
            );
            self.result(step, Outcome::Fail, detail, None)
        };
        result.presses = presses;
        result
    }

    fn launch(&mut self, step: &Step) -> StepResult {
        if self.running {
            let _ = self.emulator.close(Duration::from_secs(10));
            self.running = false;
        }
        self.stage = Stage::NotStarted;
        self.finish_launch();
        let number = self.launches.len() + 1;
        if number == 1
            && let Err(error) = self
                .emulator
                .take_logs(self.title, &self.dir.join("before-launch-1"))
        {
            return self.result(step, Outcome::Fail, error.to_string(), None);
        }
        self.launches.push(LaunchRecord {
            number,
            launch: Launch::default(),
            logs: format!("launch-{number}"),
            perf: None,
            resources: None,
            problems: None,
            frames: None,
            crash: None,
        });
        self.launch_open = true;
        self.launch_started = self.clock.now();
        self.launched_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|since| u64::try_from(since.as_millis()).ok());
        self.last_glimpse = self.launch_started;
        self.pressed.clear();
        match self.emulator.launch(&self.nsp) {
            Ok(()) => {
                self.running = true;
                self.result(
                    step,
                    Outcome::Pass,
                    format!("launch {number} started"),
                    None,
                )
            }
            Err(error) => self.result(step, Outcome::Fail, error.to_string(), None),
        }
    }

    fn wait_for(&mut self, step: &Step, milestone: Milestone, timeout: u32) -> StepResult {
        let mut deadline = self.clock.now() + Duration::from_secs(u64::from(timeout));
        let waited_from = self.clock.now();
        loop {
            if self.stopped() {
                return self.result(step, Outcome::Skip, "the run was stopped".to_owned(), None);
            }
            let launch = self.current_launch();
            if launch.stage() > self.stage {
                self.stage = launch.stage();
                if let Some(on_stage) = self.on_stage.as_mut() {
                    on_stage(self.stage);
                }
            }
            let reached = match milestone {
                Milestone::Wine => launch.stage() >= Stage::Wine,
                Milestone::LoadingScreen => launch.stage() >= Stage::LoadingScreen,
                Milestone::FirstFrame => launch.stage() == Stage::FirstFrame,
                Milestone::Commit => !launch.commits.is_empty(),
            };
            let seconds = (self.clock.now() - waited_from).as_secs_f64();
            if reached {
                let detail = match (&launch.loading_hidden, milestone) {
                    (Some((at, reason)), Milestone::FirstFrame) => {
                        format!("first frame at {at} s of the launch: {}", reason.text())
                    }
                    _ => format!("reached after {seconds:.0} s"),
                };
                return self.result(step, Outcome::Pass, detail, None);
            }
            let gave_up = launch
                .loading_hidden
                .as_ref()
                .is_some_and(|(_, reason)| !reason.is_success());
            let crashed = matches!(launch.exit, Some(Exit::Code(_)));
            let ended = !self.emulator.running();
            if gave_up || crashed || ended || self.clock.now() >= deadline {
                let why = if crashed {
                    format!("the game crashed ({})", describe_exit(launch.exit))
                } else if gave_up {
                    "the loading screen gave up: no picture in time".to_owned()
                } else if ended {
                    self.emulator
                        .end_reason()
                        .unwrap_or_else(|| "the game ended before it".to_owned())
                } else if self.save_backtraces() {
                    format!("not reached in {timeout} s, thread stacks in {BACKTRACES_FILE}")
                } else {
                    format!("not reached in {timeout} s")
                };
                let stalls = if launch.stalls > 0 {
                    format!(", {} stall reports", launch.stalls)
                } else {
                    String::new()
                };
                let detail = format!(
                    "{why}; got as far as: {}{stalls}",
                    stage_name(launch.stage())
                );
                if ended {
                    self.running = false;
                }
                return self.result(step, Outcome::Fail, detail, None);
            }
            if self.held() {
                deadline += POLL;
            }
            self.glimpse();
            self.clock.sleep(POLL);
        }
    }

    fn expect_screen(
        &mut self,
        step: &Step,
        name: &str,
        reference: &str,
        tolerance: f32,
    ) -> StepResult {
        let shot = self.screenshot(step, name);
        if shot.outcome == Outcome::Fail {
            return shot;
        }
        let file = format!("{name}.png");
        let plain = Path::new(reference)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        let Some(path) = self
            .references
            .as_ref()
            .filter(|_| plain)
            .map(|dir| dir.join(reference))
        else {
            return self.result(
                step,
                Outcome::Fail,
                format!("{reference} is not a path in the references folder"),
                Some(file),
            );
        };
        let read = |path: &Path| {
            std::fs::read(path)
                .ok()
                .and_then(|png| Thumbnail::from_png(&png).ok())
        };
        match (read(&self.dir.join(&file)), read(&path)) {
            (Some(now), Some(expected)) => {
                let difference = now.difference(&expected).mean;
                if difference <= f64::from(tolerance) {
                    self.result(
                        step,
                        Outcome::Pass,
                        format!("shows {reference} (difference {difference:.1})"),
                        Some(file),
                    )
                } else {
                    self.result(
                        step,
                        Outcome::Fail,
                        format!("differs from {reference}: {difference:.1}, more than {tolerance}"),
                        Some(file),
                    )
                }
            }
            (_, None) => self.result(
                step,
                Outcome::Warn,
                format!(
                    "no reference picture {reference} yet: make one from this screenshot in History"
                ),
                Some(file),
            ),
            (None, _) => self.result(
                step,
                Outcome::Fail,
                "the screenshot does not read".to_owned(),
                Some(file),
            ),
        }
    }

    fn screenshot(&mut self, step: &Step, name: &str) -> StepResult {
        let file = format!("{name}.png");
        match self.emulator.screenshot(&self.dir.join(&file)) {
            Ok(()) => {
                self.screenshots.push((name.to_owned(), file.clone()));
                let blank = std::fs::read(self.dir.join(&file))
                    .ok()
                    .and_then(|png| Thumbnail::from_png(&png).ok())
                    .is_some_and(|thumb| thumb.is_blank());
                if blank {
                    self.result(
                        step,
                        Outcome::Warn,
                        "the screen is one flat colour".to_owned(),
                        Some(file),
                    )
                } else {
                    self.result(step, Outcome::Pass, format!("saved {file}"), Some(file))
                }
            }
            Err(error) => self.result(step, Outcome::Fail, error.to_string(), None),
        }
    }

    fn expect_change(
        &mut self,
        step: &Step,
        from: &str,
        to: &str,
        soft: bool,
        quiet: &[[String; 2]],
    ) -> StepResult {
        let thumb = |name: &str| {
            let file = self
                .screenshots
                .iter()
                .find(|(shot, _)| shot == name)
                .map(|(_, file)| file.clone())?;
            let png = std::fs::read(self.dir.join(&file)).ok()?;
            Some((file, Thumbnail::from_png(&png).ok()?))
        };
        let (Some((_, before)), Some((after_file, after))) = (thumb(from), thumb(to)) else {
            let outcome = if soft { Outcome::Warn } else { Outcome::Fail };
            let detail = format!("the pictures {from} and {to} cannot be read");
            return self.result(step, outcome, detail, None);
        };
        let difference = before.difference(&after);
        let own = quiet
            .iter()
            .filter_map(|[first, second]| {
                let (_, first) = thumb(first)?;
                let (_, second) = thumb(second)?;
                Some(first.difference(&second))
            })
            .max_by(|a, b| a.mean.total_cmp(&b.mean));
        match picture::press_change(own, difference) {
            Change::Changed => {
                let detail = format!("the screen changed (difference {:.1})", difference.mean);
                self.result(step, Outcome::Pass, detail, Some(after_file))
            }
            Change::MovesOnItsOwn => {
                let detail = format!(
                    "the screen changes on its own about as much (difference {:.1}, without input {:.1}), so the \
                     pictures cannot tell",
                    difference.mean,
                    own.map_or(0.0, |own| own.mean)
                );
                self.result(step, Outcome::Warn, detail, Some(after_file))
            }
            Change::Unchanged => {
                let outcome = if soft { Outcome::Warn } else { Outcome::Fail };
                let detail = format!(
                    "the screen did not change (difference {:.1})",
                    difference.mean
                );
                self.result(step, outcome, detail, Some(after_file))
            }
        }
    }

    fn expect_performance(&mut self, step: &Step, threshold: u32) -> StepResult {
        let Some(base) = self.baseline.clone() else {
            return self.result(
                step,
                Outcome::Pass,
                "no earlier Carafe version of this game to compare with".to_owned(),
                None,
            );
        };
        let first = self.launches.first();
        let first_frame = first.and_then(|record| match &record.launch.loading_hidden {
            Some((at, reason)) if reason.is_success() => Some(f64::from(*at)),
            _ => None,
        });
        let fps = first
            .and_then(|record| record.perf)
            .map(|perf| perf.median_fps);
        let mut worse = Vec::new();
        let mut within = Vec::new();
        if let (Some(before), Some(now)) = (base.first_frame, first_frame) {
            match versions::slower_than(before.median, now, false, threshold) {
                Some(percent) => worse.push(format!(
                    "first frame at {now:.0} s against {:.0} s (+{percent:.0} %)",
                    before.median
                )),
                None => within.push(format!(
                    "first frame at {now:.0} s against {:.0} s",
                    before.median
                )),
            }
        }
        if let (Some(before), Some(now)) = (base.fps, fps) {
            match versions::slower_than(before.median, now, true, threshold) {
                Some(percent) => worse.push(format!(
                    "median FPS {now:.0} against {:.0} (−{percent:.0} %)",
                    before.median
                )),
                None => within.push(format!("median FPS {now:.0} against {:.0}", before.median)),
            }
        }
        if !worse.is_empty() {
            let detail = format!(
                "slower than {} by more than {threshold} %: {}",
                base.commit,
                worse.join(", ")
            );
            return self.result(step, Outcome::Fail, detail, None);
        }
        if within.is_empty() {
            return self.result(
                step,
                Outcome::Pass,
                format!("nothing measured to compare with {}", base.commit),
                None,
            );
        }
        let detail = format!(
            "within {threshold} % of {}: {}",
            base.commit,
            within.join(", ")
        );
        self.result(step, Outcome::Pass, detail, None)
    }

    fn expect_save_kept(&mut self, step: &Step) -> StepResult {
        let remembered = self.remembered.clone().unwrap_or_default();
        let now = self.emulator.save_files(self.title);
        if remembered.is_empty() {
            return self.result(
                step,
                Outcome::Warn,
                "there was no save data to keep".to_owned(),
                None,
            );
        }
        let missing: Vec<&str> = remembered
            .iter()
            .filter(|file| !now.iter().any(|kept| kept.path == file.path))
            .map(|file| file.path.as_str())
            .collect();
        let changed: Vec<&str> = remembered
            .iter()
            .filter(|file| {
                now.iter()
                    .any(|kept| kept.path == file.path && kept.sha256 != file.sha256)
            })
            .map(|file| file.path.as_str())
            .collect();
        if !missing.is_empty() {
            let detail = format!("lost after the restart: {}", missing.join(", "));
            self.result(step, Outcome::Fail, detail, None)
        } else if !changed.is_empty() {
            let detail = format!("rewritten after the restart: {}", changed.join(", "));
            self.result(step, Outcome::Warn, detail, None)
        } else {
            let detail = format!("all {} files kept", remembered.len());
            self.result(step, Outcome::Pass, detail, None)
        }
    }

    fn random_presses(&mut self, step: &Step, seconds: u32, seed: u64) -> StepResult {
        const BUTTONS: [SwitchButton; 13] = [
            SwitchButton::A,
            SwitchButton::B,
            SwitchButton::X,
            SwitchButton::Y,
            SwitchButton::L,
            SwitchButton::R,
            SwitchButton::Zl,
            SwitchButton::Zr,
            SwitchButton::Plus,
            SwitchButton::Up,
            SwitchButton::Down,
            SwitchButton::Left,
            SwitchButton::Right,
        ];
        let mut state = seed.max(1);
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let end = self.clock.now() + Duration::from_secs(u64::from(seconds));
        let mut count = 0u32;
        while self.clock.now() < end {
            if self.stopped() {
                return self.result(step, Outcome::Skip, "the run was stopped".to_owned(), None);
            }
            let crashed = matches!(self.current_launch().exit, Some(Exit::Code(_)));
            if crashed || !self.emulator.running() {
                self.running = false;
                let why = if crashed {
                    format!(
                        "the game crashed ({})",
                        describe_exit(self.current_launch().exit)
                    )
                } else {
                    self.emulator
                        .end_reason()
                        .unwrap_or_else(|| "the game ended".to_owned())
                };
                return self.result(
                    step,
                    Outcome::Fail,
                    format!("{why} after {count} random presses, seed {seed}"),
                    None,
                );
            }
            let button = BUTTONS[usize::try_from(next() % BUTTONS.len() as u64).unwrap_or(0)];
            let hold = Duration::from_millis(50 + next() % 450);
            if let Err(error) = self.emulator.press(&[button], hold) {
                return self.result(step, Outcome::Fail, error.to_string(), None);
            }
            count += 1;
            self.glimpse();
            self.clock.sleep(Duration::from_millis(100 + next() % 900));
        }
        self.result(
            step,
            Outcome::Pass,
            format!("{count} random presses, seed {seed}, the game still runs"),
            None,
        )
    }

    fn close(&mut self, step: &Step, timeout: u32) -> StepResult {
        let closed = self.emulator.close(Duration::from_secs(u64::from(timeout)));
        self.running = false;
        let launch = self.current_launch();
        self.finish_launch();
        match closed {
            Ok(Closed::ByRequest) => {
                let detail = if launch.commits_ok() && launch.shutdown {
                    "closed on request, the overlay shut down".to_owned()
                } else if !launch.commits_ok() {
                    "closed on request, but a save data commit failed".to_owned()
                } else {
                    "closed on request".to_owned()
                };
                let outcome = if launch.commits_ok() {
                    Outcome::Pass
                } else {
                    Outcome::Fail
                };
                self.result(step, outcome, detail, None)
            }
            Ok(Closed::Forced) => {
                let detail = match self.emulator.end_reason() {
                    Some(reason) => format!("did not close by itself: {reason}"),
                    None => format!("did not close in {timeout} s and was stopped"),
                };
                self.result(step, Outcome::Warn, detail, None)
            }
            Err(error) => self.result(step, Outcome::Fail, error.to_string(), None),
        }
    }

    fn current_launch(&self) -> Launch {
        let logs = self.emulator.logs(self.title);
        Launch::read(&logs.overlay, &logs.runtime)
    }

    fn finish_launch(&mut self) {
        if !self.launch_open {
            return;
        }
        self.launch_open = false;
        let Some(record) = self.launches.last_mut() else {
            return;
        };
        let logs = self.emulator.logs(self.title);
        record.launch = Launch::read(&logs.overlay, &logs.runtime);
        record.perf = match &record.launch.loading_hidden {
            Some((at, reason)) if reason.is_success() => {
                perf::summarize(&perf::after(&self.emulator.launch_perf(), *at))
            }
            _ => None,
        };
        let samples = self.emulator.launch_resources();
        record.resources = resources::summarize(&samples);
        let dest = self.dir.join(&record.logs);
        let _ = self.emulator.take_logs(self.title, &dest);
        record.problems = write_problems(&dest);
        let eden_log = std::fs::read(dest.join("eden_log.txt"))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        record.crash = crash::find(&eden_log).map(|found| {
            let _ = std::fs::write(dest.join(crash::FILE), &found.text);
            found.summary()
        });
        let read = |path: PathBuf| {
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default()
        };
        let overlay = read(dest.join("carafe-overlay.log"));
        let runtime = read(dest.join("logs").join("autorun_runtime.log"));
        let lab = self
            .lab_lines
            .iter()
            .filter(|(at, _)| *at >= self.launch_started)
            .map(|(at, text)| ((*at - self.launch_started).as_secs_f64(), text.clone()))
            .collect();
        let sources = timeline::Sources {
            lab,
            eden: &eden_log,
            overlay: &overlay,
            runtime: &runtime,
            resources: &samples,
            launched_ms: self.launched_ms,
        };
        let _ = std::fs::write(
            dest.join(timeline::FILE),
            timeline::render(&timeline::merge(&sources)),
        );
        record.frames = std::fs::read_to_string(dest.join(frames::FILE))
            .ok()
            .and_then(|text| frames::summarize(&frames::parse(&text)));
    }

    fn result(
        &self,
        step: &Step,
        outcome: Outcome,
        detail: String,
        screenshot: Option<String>,
    ) -> StepResult {
        StepResult {
            step: step.clone(),
            outcome,
            detail,
            at: (self.clock.now() - self.started).as_secs_f64(),
            screenshot,
            presses: Vec::new(),
        }
    }
}

/// Name of the file in a launch's log folder that lists the errors and warnings of its logs.
pub const PROBLEMS_FILE: &str = "problems.json";

fn write_problems(dir: &Path) -> Option<ProblemCounts> {
    let mut files = Vec::new();
    collect_logs(dir, &mut files);
    if files.is_empty() {
        return None;
    }
    files.sort();
    let found: Vec<Problem> = files
        .iter()
        .flat_map(|file| {
            let name = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = std::fs::read(file)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            problems::problems(problems::source_of(&name), &text)
        })
        .collect();
    let json = serde_json::to_string_pretty(&found).ok()?;
    std::fs::write(dir.join(PROBLEMS_FILE), format!("{json}\n")).ok()?;
    Some(problems::count(&found))
}

fn collect_logs(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() {
            collect_logs(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "log" || extension == "txt")
        {
            files.push(path);
        }
    }
}

fn describe_exit(exit: Option<Exit>) -> String {
    match exit {
        Some(Exit::Code(code)) => format!("exit code 0x{code:08x}"),
        Some(Exit::Clean) => "exit code 0".to_owned(),
        None => "no exit code".to_owned(),
    }
}

/// Returns the name of a stage for the report.
#[must_use]
pub fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::NotStarted => "nothing: the loader or the runtime did not start",
        Stage::Overlay => "the overlay mounted the game",
        Stage::Wine => "Wine started",
        Stage::LoadingScreen => "the loading screen",
        Stage::FirstFrame => "the game's first frame",
    }
}

/// Returns the lines of an emulator log that report something the emulator does not implement.
#[must_use]
pub fn emulator_gaps(log: &str) -> Vec<String> {
    let mut gaps: Vec<String> = Vec::new();
    for line in log.lines() {
        let lower = line.to_ascii_lowercase();
        if EXPECTED_GAPS
            .iter()
            .any(|expected| lower.contains(expected))
        {
            continue;
        }
        let missing = lower.contains("unimplemented")
            || lower.contains("not implemented")
            || lower.contains("unknown ioctl");
        let relevant = lower.contains("nvdrv")
            || lower.contains("nvhost")
            || lower.contains("nvmap")
            || lower.contains("svc")
            || lower.contains("kernel");
        if missing && relevant {
            let text = line.trim();
            let message = without_timestamp(text);
            if !gaps.iter().any(|seen| without_timestamp(seen) == message) {
                gaps.push(text.to_owned());
            }
        }
        if gaps.len() >= 20 {
            break;
        }
    }
    gaps
}

fn without_timestamp(line: &str) -> &str {
    match line.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
        Some((_, message)) => message.trim_start(),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::VecDeque;

    use super::*;

    struct FakeClock(Cell<Duration>);

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
        fn sleep(&self, duration: Duration) {
            self.0.set(self.0.get() + duration);
        }
    }

    #[derive(Default)]
    struct FakeEmulator {
        script: VecDeque<GameLogs>,
        current: GameLogs,
        running: bool,
        presses: Vec<Vec<SwitchButton>>,
        launches: usize,
        save: Vec<SaveFile>,
        save_after_relaunch: Option<Vec<SaveFile>>,
        close_result: Option<Closed>,
        emulator_log: String,
        shots: Vec<Vec<u8>>,
        probe_log: Option<String>,
    }

    impl Emulator for FakeEmulator {
        fn launch(&mut self, _nsp: &Path) -> Result<(), EmulatorError> {
            self.launches += 1;
            self.running = true;
            self.current = GameLogs::default();
            if self.launches == 2
                && let Some(save) = self.save_after_relaunch.take()
            {
                self.save = save;
            }
            Ok(())
        }
        fn running(&mut self) -> bool {
            self.running
        }
        fn press(
            &mut self,
            buttons: &[SwitchButton],
            _hold: Duration,
        ) -> Result<(), EmulatorError> {
            self.presses.push(buttons.to_vec());
            Ok(())
        }
        fn screenshot(&mut self, dest: &Path) -> Result<(), EmulatorError> {
            let png = if self.shots.is_empty() {
                Vec::new()
            } else {
                self.shots.remove(0)
            };
            std::fs::write(dest, png).map_err(|error| EmulatorError::Io(error.to_string()))
        }
        fn close(&mut self, _timeout: Duration) -> Result<Closed, EmulatorError> {
            self.running = false;
            Ok(self.close_result.unwrap_or(Closed::ByRequest))
        }
        fn logs(&self, _title: TitleId) -> GameLogs {
            self.current.clone()
        }
        fn take_logs(&mut self, _title: TitleId, _dest: &Path) -> Result<(), EmulatorError> {
            Ok(())
        }
        fn clear_save(&mut self, _title: TitleId) -> Result<(), EmulatorError> {
            Ok(())
        }
        fn save_files(&self, _title: TitleId) -> Vec<SaveFile> {
            self.save.clone()
        }
        fn emulator_log(&self) -> String {
            self.emulator_log.clone()
        }
        fn find_file(&self, _title: TitleId, name: &str) -> Option<String> {
            (name == PROBE_INPUT_LOG)
                .then(|| self.probe_log.clone())
                .flatten()
        }
    }

    impl FakeEmulator {
        fn advance(&mut self) {
            if let Some(next) = self.script.pop_front() {
                self.current = next;
            }
        }
    }

    struct Stepping<'a> {
        clock: &'a FakeClock,
        emulator: std::rc::Rc<std::cell::RefCell<FakeEmulator>>,
    }

    impl Clock for Stepping<'_> {
        fn now(&self) -> Duration {
            self.clock.now()
        }
        fn sleep(&self, duration: Duration) {
            self.clock.sleep(duration);
            self.emulator.borrow_mut().advance();
        }
    }

    struct Shared(std::rc::Rc<std::cell::RefCell<FakeEmulator>>);

    impl Emulator for Shared {
        fn launch(&mut self, nsp: &Path) -> Result<(), EmulatorError> {
            self.0.borrow_mut().launch(nsp)
        }
        fn running(&mut self) -> bool {
            self.0.borrow_mut().running()
        }
        fn press(&mut self, buttons: &[SwitchButton], hold: Duration) -> Result<(), EmulatorError> {
            self.0.borrow_mut().press(buttons, hold)
        }
        fn screenshot(&mut self, dest: &Path) -> Result<(), EmulatorError> {
            self.0.borrow_mut().screenshot(dest)
        }
        fn close(&mut self, timeout: Duration) -> Result<Closed, EmulatorError> {
            self.0.borrow_mut().close(timeout)
        }
        fn logs(&self, title: TitleId) -> GameLogs {
            self.0.borrow().logs(title)
        }
        fn take_logs(&mut self, title: TitleId, dest: &Path) -> Result<(), EmulatorError> {
            self.0.borrow_mut().take_logs(title, dest)
        }
        fn clear_save(&mut self, title: TitleId) -> Result<(), EmulatorError> {
            self.0.borrow_mut().clear_save(title)
        }
        fn save_files(&self, title: TitleId) -> Vec<SaveFile> {
            self.0.borrow().save_files(title)
        }
        fn emulator_log(&self) -> String {
            self.0.borrow().emulator_log()
        }
        fn find_file(&self, title: TitleId, name: &str) -> Option<String> {
            self.0.borrow().find_file(title, name)
        }
    }

    fn title() -> TitleId {
        TitleId::parse("0563ab630a362000").unwrap()
    }

    fn logs(overlay: &str, runtime: &str) -> GameLogs {
        GameLogs {
            overlay: overlay.to_owned(),
            runtime: runtime.to_owned(),
        }
    }

    fn started() -> GameLogs {
        logs(
            "[carafe-overlay] started 2026-10-12 10:00:00\n[carafe-overlay] [CARAFE] loading screen drawn at 0 s, 1280x720\n",
            "[INIT] virtual memory ready\n",
        )
    }

    fn first_frame() -> GameLogs {
        logs(
            "[carafe-overlay] started 2026-10-12 10:00:00\n[carafe-overlay] [CARAFE] loading screen drawn at 0 s, 1280x720\n[carafe-overlay] [CARAFE] loading screen hidden after 34 s: the game drew its picture\n",
            "[INIT] virtual memory ready\n",
        )
    }

    fn save(hash: &str) -> Vec<SaveFile> {
        vec![SaveFile {
            path: "save/game.sav".to_owned(),
            size: 120,
            sha256: hash.to_owned(),
        }]
    }

    fn run(emulator: FakeEmulator, scenario: &Scenario) -> (ScenarioResult, FakeEmulator) {
        run_against(emulator, scenario, None)
    }

    fn run_against(
        emulator: FakeEmulator,
        scenario: &Scenario,
        baseline: Option<VersionPerf>,
    ) -> (ScenarioResult, FakeEmulator) {
        let shared = std::rc::Rc::new(std::cell::RefCell::new(emulator));
        let clock = FakeClock(Cell::new(Duration::ZERO));
        let stepping = Stepping {
            clock: &clock,
            emulator: shared.clone(),
        };
        let dir = std::env::temp_dir().join(format!("carafe-lab-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut adapter = Shared(shared.clone());
        let result = Runner::new(&mut adapter, &stepping, title(), "game.nsp", &dir)
            .with_baseline(baseline, versions::DEFAULT_THRESHOLD)
            .run(scenario, |_| {});
        drop(adapter);
        drop(stepping);
        let emulator = std::rc::Rc::try_unwrap(shared).ok().unwrap().into_inner();
        (result, emulator)
    }

    fn waiting(timeout: u32) -> Scenario {
        Scenario {
            steps: vec![
                Step::Launch,
                Step::WaitFor {
                    milestone: Milestone::FirstFrame,
                    timeout,
                },
                Step::Press {
                    buttons: vec![SwitchButton::A],
                    hold: 0.1,
                },
                Step::Close { timeout: 10 },
            ],
        }
    }

    fn measured_at(first_frame: f64) -> VersionPerf {
        VersionPerf {
            game: "G".to_owned(),
            commit: "aaa1111".to_owned(),
            first_frame: Some(versions::Spread {
                median: first_frame,
                spread: 2.0,
                count: 3,
            }),
            fps: None,
            p99_ms: None,
            peak_mb: None,
        }
    }

    #[test]
    fn a_game_slower_than_the_last_version_beyond_its_threshold_fails() {
        let mut scenario = waiting(60);
        scenario
            .steps
            .push(Step::ExpectPerformance { threshold: None });
        let emulator = || FakeEmulator {
            script: VecDeque::from(vec![started(), first_frame()]),
            ..FakeEmulator::default()
        };
        let (slower, _) = run_against(emulator(), &scenario, Some(measured_at(20.0)));
        let step = slower.steps.last().unwrap();
        assert_eq!(step.outcome, Outcome::Fail, "{}", step.detail);
        assert!(
            step.detail.contains("aaa1111") && step.detail.contains("34 s against 20 s"),
            "{}",
            step.detail
        );
        let (close, _) = run_against(emulator(), &scenario, Some(measured_at(30.0)));
        assert_eq!(close.steps.last().unwrap().outcome, Outcome::Pass);
        let (first, _) = run_against(emulator(), &scenario, None);
        let step = first.steps.last().unwrap();
        assert_eq!(step.outcome, Outcome::Pass);
        assert!(step.detail.contains("no earlier"), "{}", step.detail);
    }

    #[test]
    fn waits_until_the_first_frame() {
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![started(), started(), first_frame()]),
            ..FakeEmulator::default()
        };
        let (result, emulator) = run(emulator, &waiting(60));
        assert_eq!(
            result.steps[1].outcome,
            Outcome::Pass,
            "{}",
            result.steps[1].detail
        );
        assert!(result.steps[1].detail.contains("34 s"));
        assert_eq!(emulator.presses, vec![vec![SwitchButton::A]]);
        assert_eq!(result.outcome(), Outcome::Pass);
        assert_eq!(result.launches.len(), 1);
        assert_eq!(result.launches[0].launch.loading_drawn_at, Some(0));
    }

    #[test]
    fn a_timeout_fails_and_skips_what_needs_the_game() {
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![started()]),
            ..FakeEmulator::default()
        };
        let (result, emulator) = run(emulator, &waiting(5));
        assert_eq!(result.steps[1].outcome, Outcome::Fail);
        assert!(result.steps[1].detail.contains("the loading screen"));
        assert_eq!(result.steps[2].outcome, Outcome::Skip);
        assert_eq!(result.steps[3].outcome, Outcome::Skip);
        assert!(emulator.presses.is_empty());
        assert!(!emulator.running);
    }

    struct Releasing<'a> {
        clock: &'a FakeClock,
        hold: PathBuf,
        release_at: Duration,
    }

    impl Clock for Releasing<'_> {
        fn now(&self) -> Duration {
            self.clock.now()
        }
        fn sleep(&self, duration: Duration) {
            self.clock.sleep(duration);
            if self.clock.now() >= self.release_at {
                let _ = std::fs::remove_file(&self.hold);
            }
        }
    }

    #[test]
    fn random_presses_with_one_seed_press_the_same_buttons() {
        let scenario = Scenario {
            steps: vec![
                Step::Launch,
                Step::RandomPresses {
                    seconds: 10,
                    seed: 7,
                },
                Step::Close { timeout: 10 },
            ],
        };
        let (first, emulator) = run(FakeEmulator::default(), &scenario);
        let (_, again) = run(FakeEmulator::default(), &scenario);
        assert_eq!(
            first.steps[1].outcome,
            Outcome::Pass,
            "{}",
            first.steps[1].detail
        );
        assert!(emulator.presses.len() >= 10);
        assert_eq!(emulator.presses, again.presses);
    }

    #[test]
    fn a_held_scenario_starts_no_step_until_the_game_is_handed_back() {
        let dir = std::env::temp_dir().join(format!("carafe-lab-hold-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let hold = dir.join(HOLD_FILE);
        std::fs::write(&hold, "").unwrap();
        let clock = FakeClock(Cell::new(Duration::ZERO));
        let releasing = Releasing {
            clock: &clock,
            hold: hold.clone(),
            release_at: Duration::from_secs(20),
        };
        let mut emulator = FakeEmulator::default();
        let result = Runner::new(&mut emulator, &releasing, title(), "game.nsp", &dir)
            .with_hold(&hold)
            .run(&waiting(5), |_| {});
        assert!(
            result.steps[0].at >= 20.0,
            "launched at {}",
            result.steps[0].at
        );
        assert_eq!(result.steps[1].outcome, Outcome::Fail);
        assert!(!hold.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_fails_at_once() {
        let crashed = logs(
            "[carafe-overlay] started 2026-10-12 10:00:00\n",
            "[INIT] virtual memory ready\n[EXIT] NtTerminateProcess(self) exit_code=0xc0000005\n",
        );
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![crashed]),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &waiting(600));
        assert_eq!(result.steps[1].outcome, Outcome::Fail);
        assert!(result.steps[1].detail.contains("0xc0000005"));
        assert!(result.steps[1].at < 5.0);
    }

    #[test]
    fn an_emulator_gap_turns_a_failure_into_a_console_check() {
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![started()]),
            emulator_log:
                "[ 12.3] Service.NVDRV <Error> nvmap.cpp:Ioctl1: Unimplemented ioctl=0x0F\n"
                    .to_owned(),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &waiting(5));
        assert_eq!(result.steps[1].outcome, Outcome::Console);
        assert_eq!(result.emulator_gaps.len(), 1);
        assert_eq!(result.outcome(), Outcome::Console);
    }

    #[test]
    fn a_relaunch_starts_over_after_a_failure() {
        let scenario = Scenario {
            steps: vec![
                Step::Launch,
                Step::WaitFor {
                    milestone: Milestone::FirstFrame,
                    timeout: 2,
                },
                Step::Close { timeout: 5 },
                Step::Launch,
                Step::Close { timeout: 5 },
            ],
        };
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![started()]),
            ..FakeEmulator::default()
        };
        let (result, emulator) = run(emulator, &scenario);
        assert_eq!(result.steps[2].outcome, Outcome::Skip);
        assert_eq!(result.steps[3].outcome, Outcome::Pass);
        assert_eq!(emulator.launches, 2);
    }

    #[test]
    fn save_data_must_survive_the_restart() {
        let scenario = Scenario::standard(30);
        let lost = FakeEmulator {
            script: VecDeque::from(vec![first_frame()]),
            save: save("aa"),
            save_after_relaunch: Some(Vec::new()),
            ..FakeEmulator::default()
        };
        let (result, _) = run(lost, &scenario);
        let kept = result
            .steps
            .iter()
            .find(|step| step.step == Step::ExpectSaveKept)
            .unwrap();
        assert_eq!(kept.outcome, Outcome::Fail, "{}", kept.detail);
        let fine = FakeEmulator {
            script: VecDeque::from(vec![first_frame()]),
            save: save("aa"),
            ..FakeEmulator::default()
        };
        let (result, _) = run(fine, &scenario);
        let kept = result
            .steps
            .iter()
            .find(|step| step.step == Step::ExpectSaveKept)
            .unwrap();
        assert_eq!(kept.outcome, Outcome::Pass, "{}", kept.detail);
    }

    #[test]
    fn a_forced_close_is_a_warning() {
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![first_frame()]),
            close_result: Some(Closed::Forced),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &waiting(60));
        assert_eq!(result.steps[3].outcome, Outcome::Warn);
    }

    #[test]
    fn a_failed_commit_fails_the_close() {
        let failed = logs(
            "[carafe-overlay] started 2026-10-12 10:00:00\n[carafe-overlay] [CARAFE] loading screen hidden after 3 s: Vulkan frames\n[carafe-overlay] commit on close request at 9.000 s: rc=0x00002EE2, 0 files reopened, 0 lost, 9 us\n",
            "",
        );
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![failed]),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &waiting(60));
        assert_eq!(
            result.steps[3].outcome,
            Outcome::Fail,
            "{}",
            result.steps[3].detail
        );
    }

    fn input_scenario() -> Scenario {
        Scenario {
            steps: vec![
                Step::Launch,
                Step::WaitFor {
                    milestone: Milestone::FirstFrame,
                    timeout: 60,
                },
                Step::Press {
                    buttons: vec![SwitchButton::A],
                    hold: 0.3,
                },
                Step::Press {
                    buttons: vec![SwitchButton::Zr],
                    hold: 0.3,
                },
                Step::ExpectInput,
                Step::Close { timeout: 10 },
            ],
        }
    }

    #[test]
    fn the_input_probe_log_is_checked_against_the_bindings() {
        let log = "\
1.000 pad buttons=0x2000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
1.200 pad buttons=0x0000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
3.000 pad buttons=0x0000 lt=0 rt=255 lx=0 ly=0 rx=0 ry=0
";
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![first_frame()]),
            probe_log: Some(log.to_owned()),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &input_scenario());
        let check = &result.steps[4];
        assert_eq!(check.outcome, Outcome::Pass, "{}", check.detail);
        assert_eq!(check.presses.len(), 2);
    }

    #[test]
    fn a_wrong_button_fails_the_input_check() {
        let log = "1.000 pad buttons=0x1000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0\n3.000 pad buttons=0x0000 lt=0 rt=255 lx=0 ly=0 rx=0 ry=0\n";
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![first_frame()]),
            probe_log: Some(log.to_owned()),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &input_scenario());
        let check = &result.steps[4];
        assert_eq!(check.outcome, Outcome::Fail);
        assert!(
            check.detail.contains("A: expected pad B, got pad A"),
            "{}",
            check.detail
        );
    }

    #[test]
    fn no_probe_log_fails_the_input_check() {
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![first_frame()]),
            ..FakeEmulator::default()
        };
        let (result, _) = run(emulator, &input_scenario());
        assert_eq!(result.steps[4].outcome, Outcome::Fail);
    }

    #[test]
    fn the_mesosphere_probe_is_not_a_gap() {
        let log = "[  3.1] Kernel.SVC <Error> core/hle/kernel/svc/svc_info.cpp:268:GetInfo: Unimplemented svcGetInfo id=0x000000000000FDE8\n";
        assert!(emulator_gaps(log).is_empty());
    }

    #[test]
    fn a_stop_skips_the_rest_and_closes_the_game() {
        let emulator = FakeEmulator {
            script: VecDeque::from(vec![started()]),
            ..FakeEmulator::default()
        };
        let shared = std::rc::Rc::new(std::cell::RefCell::new(emulator));
        let clock = FakeClock(Cell::new(Duration::ZERO));
        let stepping = Stepping {
            clock: &clock,
            emulator: shared.clone(),
        };
        let stop = CancelToken::new();
        stop.cancel();
        let mut adapter = Shared(shared.clone());
        let dir = std::env::temp_dir();
        let result = Runner::new(&mut adapter, &stepping, title(), "game.nsp", &dir)
            .with_stop(stop)
            .run(&waiting(600), |_| {});
        assert!(
            result
                .steps
                .iter()
                .all(|step| step.outcome == Outcome::Skip)
        );
        assert_eq!(shared.borrow().launches, 0);
    }

    #[test]
    fn gaps_are_read_from_the_emulator_log() {
        let log = "\
[  1.0] Kernel <Info> boot
[  2.0] Service.NVDRV <Error> Unimplemented ioctl 0x0F
[  2.1] Service.NVDRV <Error> Unimplemented ioctl 0x0F
[  3.0] Service.HID <Warning> unimplemented function 12
";
        assert_eq!(
            emulator_gaps(log),
            vec!["[  2.0] Service.NVDRV <Error> Unimplemented ioctl 0x0F".to_owned()]
        );
    }
}
