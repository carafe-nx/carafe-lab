//! `carafe-lab`: builds games and the probe programs, runs them in Eden and writes the report.

mod cli;
mod live;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cli::{Args, Command};
use crate::live::Live;
use carafe_core::cancel::CancelToken;
use carafe_core::metadata::Metadata;
use carafe_core::ports::FolderInspector;
use carafe_core::record::GameSource;
use carafe_core::settings::{AutorunSettings, InputMode};
use carafe_core::wizard::BuildRequest;
use carafe_folder::DiskFolderInspector;
use carafe_pack::HacBrewPacker;
use lab_core::config::{self, Config, Game};
use lab_core::emulator::{EdenCpu, Gpu};
use lab_core::lang::Lang;
use lab_core::local::{self, LocalSettings};
use lab_core::matrix;
use lab_core::probes::{self, ProbeRun};
use lab_core::time::Utc;
use lab_core::view::LabView;
use lab_eden::Eden;
use lab_run::engine::{Engine, Event, Planned};
use lab_run::history::{self, VARIANT_A, VARIANT_B};
use lab_run::queue::{Holding, Queue, QueueEntry, QueueState, Turn, unix_now};
use lab_run::report::{self, RunStamp};
use lab_run::run::{Clock, Outcome, StepResult};
use serde::Serialize;

const REPOSITORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");
const DESKTOP: &str = "app/crates/carafe-desktop";
const PAGE_IDLE: Duration = Duration::from_secs(60);
const PAGE_LONGEST: Duration = Duration::from_secs(30 * 60);
const QUEUE_POLL: Duration = Duration::from_secs(2);
const RUNTIME_VERSION: &str = match option_env!("CARAFE_RUNTIME_VERSION") {
    Some(version) => version,
    None => "dev",
};

struct RealClock(Instant);

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

struct Lab {
    args: Args,
    work: PathBuf,
    gpu: Gpu,
    cpu: EdenCpu,
    lang: Lang,
    timeout: u32,
    keys: PathBuf,
    carafe: PathBuf,
    image: String,
    desktop: PathBuf,
    packer: HacBrewPacker,
    eden: Eden,
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse(&raw) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}\n\n{}", cli::HELP);
            return ExitCode::from(2);
        }
    };
    if args.command == Command::Help {
        print!("{}", cli::HELP);
        return ExitCode::SUCCESS;
    }
    match start(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("carafe-lab: {error}");
            ExitCode::from(1)
        }
    }
}

fn start(args: Args) -> Result<ExitCode, String> {
    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| repository().join(config::CONFIG_FILE));
    let config = Config::load(&config_path).map_err(|error| error.to_string())?;
    match &args.command {
        Command::Status => return show_status(&config, &config_path, args.json),
        Command::Games => return show_games(&config, args.json),
        Command::Runs => return show_runs(&config, args.json),
        Command::Stop { id } => return stop_run(&config, id.as_deref()),
        _ => {}
    }
    let keys = console_files(args.keys.as_deref(), &config_path)?;
    let gpu = args.gpu.unwrap_or(config.gpu);
    let desktop = carafe_checkout(&config).join(DESKTOP);
    let packer = HacBrewPacker::new(
        desktop.join("resources").join("carafe-runtime.bin"),
        desktop.join("binaries").join(hacbrewpack_name()),
    );
    let mut eden = Eden::new(
        config.work.join("eden").join("user"),
        &keys,
        gpu,
        config.emulator_cpu,
    )
    .with_settings(config.eden_settings.clone())
    .with_one_eden(config.one_eden);
    if let Some(image) = &config.eden_image {
        eden = eden.with_image(image.clone());
    }
    let lab = Lab {
        work: config.work.clone(),
        gpu,
        cpu: config.emulator_cpu,
        lang: args.lang.unwrap_or(config.lang),
        timeout: args.timeout.unwrap_or(config.timeout),
        args,
        keys,
        carafe: carafe_checkout(&config),
        image: config
            .eden_image
            .clone()
            .unwrap_or_else(|| lab_eden::IMAGE.to_owned()),
        desktop,
        packer,
        eden,
    };
    match lab.args.command.clone() {
        Command::Gpu => {
            let info = lab.eden.gpu_info().map_err(|error| error.to_string())?;
            print!("{info}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Run { set } => lab.run_games(&config, set.as_deref()),
        Command::Probes => lab.run_probes(),
        Command::Help | Command::Status | Command::Games | Command::Runs | Command::Stop { .. } => {
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    config: String,
    work: String,
    carafe: String,
    carafe_commit: Option<String>,
    runtime_built: bool,
    queue: QueueState,
}

fn show_status(config: &Config, config_path: &Path, json: bool) -> Result<ExitCode, String> {
    let carafe = carafe_checkout(config);
    let status = Status {
        config: config_path.display().to_string(),
        work: config.work.display().to_string(),
        carafe_commit: git_commit(&carafe),
        runtime_built: carafe
            .join(DESKTOP)
            .join("resources")
            .join("carafe-runtime.bin")
            .is_file(),
        carafe: carafe.display().to_string(),
        queue: queue_of(config).state(unix_now()),
    };
    if json {
        print_json(&status)?;
        return Ok(ExitCode::SUCCESS);
    }
    println!("lab file: {}", status.config);
    println!("work: {}", status.work);
    println!(
        "carafe: {} at {}{}",
        status.carafe,
        status
            .carafe_commit
            .as_deref()
            .unwrap_or("an unknown commit"),
        if status.runtime_built {
            ""
        } else {
            ", runtime not built"
        }
    );
    match &status.queue.holder {
        Some(holder) => println!(
            "emulator: held by {} running {} (queue id {})",
            holder.who, holder.what, holder.id
        ),
        None => println!("emulator: free"),
    }
    for (place, entry) in status.queue.waiting.iter().enumerate() {
        println!(
            "  waiting {}: {} running {} (queue id {})",
            place + 1,
            entry.who,
            entry.what,
            entry.id
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn show_games(config: &Config, json: bool) -> Result<ExitCode, String> {
    let view = LabView::of(config);
    if json {
        print_json(&view)?;
        return Ok(ExitCode::SUCCESS);
    }
    for game in &view.games {
        let state = if game.enabled { "on " } else { "off" };
        let sets = if game.sets.is_empty() {
            String::new()
        } else {
            format!("  [{}]", game.sets.join(", "))
        };
        println!(
            "{state} {}  {} steps  {}{sets}",
            game.name,
            game.scenario.steps.len(),
            game.folder
        );
    }
    for (set, names) in &view.sets {
        println!("set {set}: {}", names.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

fn show_runs(config: &Config, json: bool) -> Result<ExitCode, String> {
    let runs = history::list(&config.work.join("runs"));
    if json {
        print_json(&runs)?;
        return Ok(ExitCode::SUCCESS);
    }
    for run in &runs {
        let [passed, warned, console, failed] = run.counts();
        let state = if run.finished { "" } else { "  (not finished)" };
        println!(
            "{}  {}/{} games: {passed} passed, {warned} warned, {console} console, {failed} failed{state}",
            run.dir,
            run.games.len(),
            run.planned.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn stop_run(config: &Config, id: Option<&str>) -> Result<ExitCode, String> {
    let queue = queue_of(config);
    let id = match id {
        Some(id) => id.to_owned(),
        None => queue
            .state(unix_now())
            .holder
            .map(|holder| holder.id)
            .ok_or("no run holds the emulator")?,
    };
    queue.request_stop(&id)?;
    println!("asked the run {id} to stop");
    Ok(ExitCode::SUCCESS)
}

fn print_json(value: &impl Serialize) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    println!("{text}");
    Ok(())
}

fn queue_of(config: &Config) -> Queue {
    Queue::new(config.work.join("queue"))
}

fn git_commit(checkout: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_commit_with_changes(checkout: &Path) -> Option<String> {
    let commit = git_commit(checkout)?;
    let changed = std::process::Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|output| !output.stdout.is_empty());
    Some(if changed {
        format!("{commit}+changes")
    } else {
        commit
    })
}

fn file_hash(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = format!("{:x}", hasher.finalize());
    Some(digest[..12].to_owned())
}

fn image_id(image: &str) -> Option<String> {
    let output = std::process::Command::new("docker")
        .args(["image", "ls", "--quiet", image])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!id.is_empty()).then_some(id)
}

impl Lab {
    fn run_games(mut self, config: &Config, set: Option<&str>) -> Result<ExitCode, String> {
        let chosen = if self.args.games.is_empty() {
            config.select(set)
        } else {
            config.pick(&self.args.games)
        };
        let games: Vec<&Game> = chosen
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|game| self.wanted(&game.name))
            .collect();
        if games.is_empty() {
            return Err("no games to run: enable some in lab.json or name a set".to_owned());
        }
        let repeat = self.args.repeat as usize;
        let mut what = match (set, &self.args.only) {
            (_, Some(only)) => only.clone(),
            _ if !self.args.games.is_empty() => self.args.games.join(", "),
            (Some(set), None) => set.to_owned(),
            (None, None) => "enabled games".to_owned(),
        };
        let planned: Vec<Planned> = if let Some(ab) = self.args.ab.clone() {
            let [game] = games.as_slice() else {
                return Err(
                    "an A/B run takes exactly one game: name it with --games or --only".to_owned(),
                );
            };
            let overrides: serde_json::Value =
                serde_json::from_str(&ab).map_err(|error| format!("--ab: {error}"))?;
            let settings_b = config::settings_over(&game.settings, &overrides)
                .map_err(|error| format!("--ab: {error}"))?;
            what = format!("A/B {}", game.name);
            (0..repeat)
                .flat_map(|_| {
                    [
                        self.plan(game, format!("{}{VARIANT_A}", game.name), &game.settings),
                        self.plan(game, format!("{}{VARIANT_B}", game.name), &settings_b),
                    ]
                })
                .collect()
        } else {
            games
                .iter()
                .flat_map(|game| std::iter::repeat_n(*game, repeat))
                .map(|game| self.plan(game, game.name.clone(), &game.settings))
                .collect()
        };
        if repeat > 1 {
            what = format!("{what} ×{repeat}");
        }
        self.execute(planned, &what)
    }

    fn plan(&self, game: &Game, name: String, settings: &AutorunSettings) -> Planned {
        let request = DiskFolderInspector
            .inspect(&game.folder.to_string_lossy())
            .map_err(|error| error.to_string())
            .and_then(|report| {
                game.request(&report).ok_or_else(|| {
                    "the folder has no game .exe; name one with \"executable\"".to_owned()
                })
            })
            .map(|mut request| {
                request.settings = settings.clone();
                request
            });
        Planned {
            entropy: config::name_entropy(&name),
            name,
            folder: game.folder.clone(),
            request,
            label: settings_label(settings),
            scenario: game.scenario(self.timeout),
        }
    }

    fn run_probes(mut self) -> Result<ExitCode, String> {
        let builds = self
            .args
            .probes_dir
            .clone()
            .unwrap_or_else(|| repository().join("tools").join("probes").join("build"));
        let mut combinations = matrix::pairwise();
        if self.args.random > 0 {
            let seed = self.args.seed.unwrap_or_else(|| now().date_number());
            combinations.extend(matrix::random(seed, self.args.random));
        }
        let mut runs = probes::input_runs(self.timeout);
        runs.extend(probes::graphics_runs(&combinations, self.timeout));
        if !self.args.json {
            println!(
                "probes: {} runs, {} of them pairwise over {} combinations",
                runs.len(),
                matrix::pairwise().len(),
                matrix::full_count()
            );
        }
        let folders = self.work.join("probes");
        let mut planned = Vec::new();
        for run in runs.iter().filter(|run| self.wanted(&run.name)) {
            let folder = folders.join(&run.name);
            let prepared = prepare_probe(&builds, &folder, run);
            planned.push(probe_plan(run, folder, prepared));
        }
        self.execute(planned, "probes")
    }

    fn wanted(&self, name: &str) -> bool {
        self.args
            .only
            .as_ref()
            .is_none_or(|only| name.eq_ignore_ascii_case(only))
    }

    fn execute(&mut self, planned: Vec<Planned>, what: &str) -> Result<ExitCode, String> {
        let json = self.args.json;
        let stop = CancelToken::new();
        let holding = if self.args.build_only {
            None
        } else {
            let who = self.args.who.as_deref().unwrap_or("command line");
            let queue = Queue::new(self.work.join("queue"));
            match wait_for_emulator(&queue, who, what, json)? {
                Some(entry) => Some(Holding::start(queue, entry, stop.clone())),
                None => {
                    if !json {
                        println!("stopped while waiting for the emulator");
                    }
                    return Ok(ExitCode::from(4));
                }
            }
        };
        let started_at = now();
        let clock = RealClock(Instant::now());
        let emulator_label = if self.args.build_only {
            "—".to_owned()
        } else {
            let version = self.eden.version().map_err(|error| error.to_string())?;
            format!(
                "Eden {version}, {}, {}",
                gpu_name(self.gpu),
                self.cpu.name()
            )
        };
        let lang = self.lang;
        let total = planned.len();
        let live = if json {
            None
        } else {
            let live = Live::start(lang, &started_at.minutes(), total, !self.args.build_only)?;
            live.watch_presses(self.eden.press_log());
            println!("live page: {}", live.url());
            if !self.args.no_open {
                open_in_browser(&live.url());
            }
            Some(live)
        };
        if let Some(live) = &live {
            let page = live.stop_token();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !page.is_cancelled() && !stop.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(500));
                }
                stop.cancel();
            });
        }
        let stamp = RunStamp {
            carafe_commit: git_commit_with_changes(&self.carafe),
            runtime_hash: file_hash(&self.desktop.join("resources").join("carafe-runtime.bin")),
            lab_commit: git_commit_with_changes(&repository()),
            eden_image: if self.args.build_only {
                None
            } else {
                image_id(&self.image)
            },
        };
        let mut engine = Engine {
            emulator: &mut self.eden,
            packer: &self.packer,
            keys: &self.keys,
            build_inputs: vec![
                self.desktop.join("resources").join("carafe-runtime.bin"),
                self.desktop.join("binaries").join(hacbrewpack_name()),
            ],
            work: &self.work,
            lang,
            runtime_version: RUNTIME_VERSION,
            emulator_label,
            stamp,
            build_only: self.args.build_only,
            stop,
        };
        let mut on_event = |event: &Event| {
            if let (Event::RunStarted { dir, .. }, Some(holding)) = (event, &holding) {
                holding.set_run_dir(dir);
            }
            if json {
                if let Ok(line) = serde_json::to_string(event) {
                    println!("{line}");
                }
            } else {
                print_event(event, total);
            }
            if let Some(live) = &live {
                show_on_page(live, event, lang);
            }
        };
        let report = engine.run(
            planned,
            &started_at.folder(),
            &started_at.minutes(),
            &clock,
            &mut on_event,
        )?;
        drop(holding);
        if let Some(live) = live {
            live.finish();
            println!(
                "the live page stays up; close it with its button or the tab, or press Ctrl+C"
            );
            let closed = live.wait_for_close(PAGE_IDLE, PAGE_LONGEST);
            println!("live page closed: {closed:?}");
        }
        let [_, _, _, failed] = report.counts();
        Ok(if failed == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(3)
        })
    }
}

/// Puts the run in the emulator's queue and waits for its turn.
///
/// # Returns
///
/// The run's entry, which now holds the emulator, or `None` if someone asked the run to stop while it waited.
fn wait_for_emulator(
    queue: &Queue,
    who: &str,
    what: &str,
    json: bool,
) -> Result<Option<QueueEntry>, String> {
    let mut entry = queue.join(who, what, std::process::id(), unix_now())?;
    let mut shown = None;
    loop {
        if queue.stop_requested(&entry.id) {
            queue.leave(&entry);
            return Ok(None);
        }
        match queue.try_take(&mut entry, unix_now()) {
            Ok(Turn::Taken) => return Ok(Some(entry)),
            Ok(Turn::Wait { holder, ahead }) => {
                if shown.as_ref() != Some(&(holder.clone(), ahead)) {
                    let event = Event::Waiting {
                        holder: holder.clone(),
                        ahead,
                    };
                    if json {
                        print_json_line(&event);
                    } else {
                        print_event(&event, 0);
                    }
                    shown = Some((holder, ahead));
                }
            }
            Err(error) => {
                queue.leave(&entry);
                return Err(error);
            }
        }
        std::thread::sleep(QUEUE_POLL);
        queue.beat_waiting(&mut entry, unix_now())?;
    }
}

fn print_json_line(value: &impl Serialize) {
    if let Ok(line) = serde_json::to_string(value) {
        println!("{line}");
    }
}

fn print_event(event: &Event, total: usize) {
    match event {
        Event::Waiting { holder, ahead } if holder.is_empty() => {
            println!("waiting for the emulator: {ahead} runs ahead");
        }
        Event::Waiting { holder, ahead } => {
            println!("waiting for the emulator: {holder} holds it, {ahead} runs ahead");
        }
        Event::GameStarted { index, name } => println!("[{index}/{total}] {name}"),
        Event::BuildStage { stage } => println!("  {stage}"),
        Event::Built {
            title_id,
            seconds,
            reused,
        } => {
            if *reused {
                println!("  reused the NSP built from the same game, settings and runtime");
            }
            println!("  built {title_id} in {seconds:.0} s");
        }
        Event::BuildFailed { error } => println!("  build failed: {error}"),
        Event::Step { step } => print_step(step),
        Event::Reached { stage } => println!("  reached: {}", lab_run::run::stage_name(*stage)),
        Event::Stopped { left } => println!("stopped from the live page; {left} games left out"),
        Event::RunFinished { report, .. } => println!("report: {report}"),
        Event::RunStarted { .. } | Event::LaunchLogs { .. } | Event::GameFinished { .. } => {}
    }
}

fn show_on_page(live: &Live, event: &Event, lang: Lang) {
    match event {
        Event::RunStarted { dir, .. } => live.set_run_dir(Path::new(dir)),
        Event::GameStarted { index, name } => live.begin_game(*index, name),
        Event::LaunchLogs { dir } => live.begin_run(Path::new(dir)),
        Event::Step { step } => live.step(step, lang),
        Event::GameFinished { outcome, .. } => live.end_game(*outcome),
        _ => {}
    }
}

fn carafe_checkout(config: &Config) -> PathBuf {
    config.carafe_checkout(&repository())
}

fn repository() -> PathBuf {
    config::tidy(Path::new(REPOSITORY))
}

fn probe_plan(run: &ProbeRun, folder: PathBuf, prepared: Result<(), String>) -> Planned {
    let request = prepared.map(|()| BuildRequest {
        title_id: None,
        source: GameSource {
            folder: folder.to_string_lossy().into_owned(),
            executable: format!("probe-{}.exe", run.probe.name()),
            arch: run.arch,
            arch_override: None,
            arguments: Vec::new(),
        },
        metadata: Metadata::new(run.name.clone()),
        settings: run.settings.clone(),
        icon: None,
    });
    Planned {
        name: run.name.clone(),
        folder,
        request,
        entropy: config::name_entropy(&run.name),
        label: run.label.clone(),
        scenario: run.scenario.clone(),
    }
}

fn prepare_probe(builds: &Path, folder: &Path, run: &ProbeRun) -> Result<(), String> {
    let file = format!("probe-{}.exe", run.probe.name());
    let source = builds.join(probes::build_dir(run.arch)).join(&file);
    if !source.is_file() {
        return Err(format!(
            "{} is missing: run docker compose --profile build run --rm probes",
            source.display()
        ));
    }
    let _ = fs::remove_dir_all(folder);
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    fs::copy(&source, folder.join(&file)).map_err(|error| error.to_string())?;
    Ok(())
}

fn print_step(step: &StepResult) {
    let outcome = match step.outcome {
        Outcome::Pass => "pass",
        Outcome::Skip => "skip",
        Outcome::Console => "console",
        Outcome::Warn => "warn",
        Outcome::Fail => "fail",
    };
    println!(
        "  {:>6.0} s  {outcome:<7} {}  {}",
        step.at,
        report::step_title(step, Lang::En),
        step.detail
    );
}

fn console_files(argument: Option<&Path>, config_path: &Path) -> Result<PathBuf, String> {
    let local_file = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(local::LOCAL_FILE);
    let settings = LocalSettings::load(&local_file)?;
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    local::find_keys(argument, &settings, appdata.as_deref())
        .map(|(path, _)| path)
        .ok_or_else(|| {
            "no console files to pack with: choose them in the window's Emulator screen or in Carafe, or pass --keys"
                .to_owned()
        })
}

fn open_in_browser(url: &str) {
    let opened = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if opened.is_err() {
        println!("open {url} in a browser to watch the run");
    }
}

fn hacbrewpack_name() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "hacbrewpack-aarch64-pc-windows-msvc.exe"
    } else {
        "hacbrewpack-x86_64-pc-windows-msvc.exe"
    }
}

fn gpu_name(gpu: Gpu) -> &'static str {
    match gpu {
        Gpu::Wsl => "GPU through WSL2",
        Gpu::Software => "software rendering",
    }
}

fn now() -> Utc {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    Utc::from_unix(seconds)
}

fn settings_label(settings: &AutorunSettings) -> String {
    let mode = match settings.input.mode {
        InputMode::Controller => "controller",
        InputMode::KeyboardMouse => "keyboard",
    };
    format!("{} {mode}", matrix::describe(settings))
}
