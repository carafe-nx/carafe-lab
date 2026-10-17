//! `carafe-lab`: builds games and the probe programs, runs them in Eden and writes the report.

mod cli;
mod live;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cli::{Args, Command};
use crate::live::Live;
use carafe_core::BuildRecord;
use carafe_core::cancel::CancelToken;
use carafe_core::metadata::Metadata;
use carafe_core::ports::{BuildProgress, FolderInspector, PackTarget, Packer};
use carafe_core::record::GameSource;
use carafe_core::settings::{AutorunSettings, InputMode};
use carafe_core::wizard::{self, BuildRequest};
use carafe_folder::DiskFolderInspector;
use carafe_pack::HacBrewPacker;
use lab_core::config::{self, Config, Game};
use lab_core::emulator::{EdenCpu, Gpu};
use lab_core::lang::Lang;
use lab_core::matrix;
use lab_core::probes::{self, ProbeRun};
use lab_core::scenario::Scenario;
use lab_core::time::Utc;
use lab_eden::{Eden, redact};
use lab_run::report::{self, BuildOutcome, GameReport, RunReport};
use lab_run::run::{Clock, LaunchRecord, Outcome, Runner, StepResult};

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

struct Planned {
    name: String,
    folder: PathBuf,
    request: Result<BuildRequest, String>,
    entropy: u64,
    label: String,
    scenario: Scenario,
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
        desktop: desktop.clone(),
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
        println!(
            "probes: {} runs, {} of them pairwise over {} combinations",
            runs.len(),
            matrix::pairwise().len(),
            matrix::full_count()
        );
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
        let run_dir = self.work.join("runs").join(started_at.folder());
        let nsp_dir = self.work.join("nsp");
        fs::create_dir_all(&run_dir).map_err(|error| format!("{}: {error}", run_dir.display()))?;
        fs::create_dir_all(&nsp_dir).map_err(|error| format!("{}: {error}", nsp_dir.display()))?;
        let emulator = if self.args.build_only {
            "—".to_owned()
        } else {
            let version = self.eden.version().map_err(|error| error.to_string())?;
            format!(
                "Eden {version}, {}, {}",
                gpu_name(self.gpu),
                self.cpu.name()
            )
        };
        let live = Live::start(
            self.lang,
            &started_at.minutes(),
            planned.len(),
            !self.args.build_only,
        )?;
        live.set_run_dir(&run_dir);
        live.watch_presses(self.eden.press_log());
        println!("live page: {}", live.url());
        if !self.args.no_open {
            open_in_browser(&live.url());
        }
        let mut report = RunReport {
            started: started_at.minutes(),
            runtime_version: RUNTIME_VERSION.to_owned(),
            emulator,
            seconds: 0.0,
            games: Vec::new(),
        };
        let total = planned.len();
        for (index, game) in planned.into_iter().enumerate() {
            if live.stop_token().is_cancelled() {
                println!(
                    "stopped from the live page; {} games left out",
                    total - index
                );
                break;
            }
            println!("[{}/{total}] {}", index + 1, game.name);
            live.begin_game(index + 1, &game.name);
            let entry = self.one_game(&game, &run_dir, &nsp_dir, &clock, &live);
            live.end_game(entry.outcome());
            report.games.push(entry);
            report.seconds = clock.now().as_secs_f64();
            write_report(&run_dir, &report, self.lang)?;
        }
        live.finish();
        println!("report: {}", run_dir.join("report.html").display());
        println!("the live page stays up; close it with its button or the tab, or press Ctrl+C");
        let closed = live.wait_for_close(PAGE_IDLE, PAGE_LONGEST);
        println!("live page closed: {closed:?}");
        let [_, _, _, failed] = report.counts();
        Ok(if failed == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(3)
        })
    }

    fn one_game(
        &mut self,
        game: &Planned,
        run_dir: &Path,
        nsp_dir: &Path,
        clock: &RealClock,
        live: &Live,
    ) -> GameReport {
        let dir_name = safe_name(&game.name);
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
        };
        let (record, seconds, size_bytes) = match self.build(game, nsp_dir) {
            Ok(built) => built,
            Err(error) => {
                println!("  build failed: {error}");
                entry.build = BuildOutcome::Failed { error };
                return entry;
            }
        };
        println!("  built {} in {seconds:.0} s", record.title_id);
        entry.title_id = record.title_id.to_string();
        entry.build = BuildOutcome::Built {
            seconds,
            size_bytes,
        };
        if self.args.build_only {
            return entry;
        }
        live.begin_run(&self.eden.logs_dir(record.title_id));
        let nsp = nsp_dir.join(format!("{}.nsp", record.title_id));
        let lang = self.lang;
        let runner = Runner::new(&mut self.eden, clock, record.title_id, nsp, &game_dir)
            .with_input(record.settings.input.clone())
            .with_stop(live.stop_token());
        let result = runner.run(&game.scenario, |step| {
            print_step(step);
            live.step(step, lang);
        });
        entry.log_excerpt = excerpt(&game_dir, &result.launches);
        entry.scenario = Some(result);
        entry
    }

    fn build(&self, game: &Planned, nsp_dir: &Path) -> Result<(BuildRecord, f64, u64), String> {
        let request = game.request.clone()?;
        let record = wizard::plan(request, None, game.entropy, RUNTIME_VERSION)
            .map_err(|error| error.to_string())?;
        let key = build_key(&self.desktop, &record, &game.folder);
        let key_path = nsp_dir.join(format!("{}.build-key", record.title_id));
        let nsp_path = nsp_dir.join(format!("{}.nsp", record.title_id));
        if let (Some(key), Ok(kept), Ok(nsp)) =
            (&key, fs::read_to_string(&key_path), fs::metadata(&nsp_path))
            && kept.trim() == key
        {
            println!("  reused the NSP built from the same game, settings and runtime");
            return Ok((record, 0.0, nsp.len()));
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
                println!("  {:?}", update.stage);
            }
        };
        let stored = self
            .packer
            .pack(&record, None, &target, &CancelToken::new(), &mut progress)
            .map_err(|error| redact(&format!("{error} ({})", game.folder.display()), &self.keys))?;
        if let Some(key) = key {
            let _ = fs::write(&key_path, key);
        }
        Ok((record, started.elapsed().as_secs_f64(), stored.size_bytes))
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

/// Returns a digest of everything an NSP is built from: the build record, the runtime archive, hacBrewPack and
/// the size and time of every file in the game folder.
///
/// Returns `None` when any of them cannot be read, so the NSP is built again.
fn build_key(desktop: &Path, record: &BuildRecord, folder: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(record).ok()?);
    for input in [
        desktop.join("resources").join("carafe-runtime.bin"),
        desktop.join("binaries").join(hacbrewpack_name()),
    ] {
        hasher.update(file_stamp(&input)?.as_bytes());
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

/// Returns the size and modification time of `path` as text, or `None` when it cannot be read.
fn file_stamp(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}:{}", metadata.len(), modified.as_nanos()))
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
            "{} is missing: run docker compose run --rm probes",
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

fn write_report(run_dir: &Path, report: &RunReport, lang: Lang) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|error| error.to_string())?;
    fs::write(run_dir.join("report.json"), json).map_err(|error| error.to_string())?;
    fs::write(run_dir.join("report.html"), report.html(lang)).map_err(|error| error.to_string())
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

fn safe_name(name: &str) -> String {
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

fn settings_label(settings: &AutorunSettings) -> String {
    let mode = match settings.input.mode {
        InputMode::Controller => "controller",
        InputMode::KeyboardMouse => "keyboard",
    };
    format!("{} {mode}", matrix::describe(settings))
}
