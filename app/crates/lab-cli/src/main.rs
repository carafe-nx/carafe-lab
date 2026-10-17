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
use lab_core::matrix;
use lab_core::probes::{self, ProbeRun};
use lab_core::time::Utc;
use lab_eden::Eden;
use lab_run::engine::{Engine, Event, Planned};
use lab_run::report;
use lab_run::run::{Clock, Outcome, StepResult};

const REPOSITORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");
const DESKTOP: &str = "app/crates/carafe-desktop";
const PREFERENCES: &str = "app.carafe.builder/preferences.json";
const PAGE_IDLE: Duration = Duration::from_secs(60);
const PAGE_LONGEST: Duration = Duration::from_secs(30 * 60);
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
        .unwrap_or_else(|| Path::new(REPOSITORY).join(config::CONFIG_FILE));
    let config = Config::load(&config_path).map_err(|error| error.to_string())?;
    let keys = match &args.keys {
        Some(keys) => keys.clone(),
        None => keys_from_carafe()?,
    };
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
    .with_settings(config.eden_settings.clone());
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
        Command::Help => Ok(ExitCode::SUCCESS),
    }
}

impl Lab {
    fn run_games(mut self, config: &Config, set: Option<&str>) -> Result<ExitCode, String> {
        let games: Vec<&Game> = config
            .select(set)
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|game| self.wanted(&game.name))
            .collect();
        if games.is_empty() {
            return Err("no games to run: enable some in lab.json or name a set".to_owned());
        }
        let inspector = DiskFolderInspector;
        let planned = games
            .iter()
            .map(|game| {
                let request = inspector
                    .inspect(&game.folder.to_string_lossy())
                    .map_err(|error| error.to_string())
                    .and_then(|report| {
                        game.request(&report).ok_or_else(|| {
                            "the folder has no game .exe; name one with \"executable\"".to_owned()
                        })
                    });
                Planned {
                    name: game.name.clone(),
                    folder: game.folder.clone(),
                    request,
                    entropy: game.entropy(),
                    label: settings_label(&game.settings),
                    scenario: game.scenario(self.timeout),
                }
            })
            .collect();
        self.execute(planned)
    }

    fn run_probes(mut self) -> Result<ExitCode, String> {
        let builds = self.args.probes_dir.clone().unwrap_or_else(|| {
            Path::new(REPOSITORY)
                .join("tools")
                .join("probes")
                .join("build")
        });
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
        self.execute(planned)
    }

    fn wanted(&self, name: &str) -> bool {
        self.args
            .only
            .as_ref()
            .is_none_or(|only| name.eq_ignore_ascii_case(only))
    }

    fn execute(&mut self, planned: Vec<Planned>) -> Result<ExitCode, String> {
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
        let json = self.args.json;
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
        let stop = live
            .as_ref()
            .map_or_else(CancelToken::new, Live::stop_token);
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
            build_only: self.args.build_only,
            stop,
        };
        let mut on_event = |event: &Event| {
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

fn print_event(event: &Event, total: usize) {
    match event {
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

/// Returns the Carafe checkout the lab packs with: `carafe` in `lab.json`, relative to the repository, or the
/// `external/carafe` submodule.
fn carafe_checkout(config: &Config) -> PathBuf {
    let repository = Path::new(REPOSITORY);
    match &config.carafe {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => repository.join(path),
        None => repository.join("external").join("carafe"),
    }
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

fn keys_from_carafe() -> Result<PathBuf, String> {
    let appdata = std::env::var_os("APPDATA").ok_or("no APPDATA; pass --keys")?;
    let file = PathBuf::from(appdata).join(PREFERENCES);
    let text = fs::read_to_string(&file)
        .map_err(|_| "Carafe's settings were not found; pass --keys".to_owned())?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    value
        .get("keysPath")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "Carafe's settings name no file to pack with; pass --keys".to_owned())
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
