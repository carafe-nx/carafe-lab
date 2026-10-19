//! `carafe-lab`: builds games and the probe programs, runs them in Eden and writes the report.

mod cli;
mod releases;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cli::{Args, Command};
use carafe_core::cancel::CancelToken;
use carafe_core::metadata::Metadata;
use carafe_core::ports::FolderInspector;
use carafe_core::record::GameSource;
use carafe_core::settings::{AutorunSettings, InputMode, SwitchButton};
use carafe_core::wizard::BuildRequest;
use carafe_folder::DiskFolderInspector;
use carafe_pack::HacBrewPacker;
use lab_core::bisect;
use lab_core::config::{self, Config, Game};
use lab_core::diagnostics::Diagnostic;
use lab_core::emulator::{EdenCpu, Gpu};
use lab_core::lang::Lang;
use lab_core::local::{self, LocalSettings};
use lab_core::matrix;
use lab_core::milestones::Launch;
use lab_core::perf;
use lab_core::probes::{self, ProbeRun};
use lab_core::problems;
use lab_core::scenario::{Milestone, Scenario, Step};
use lab_core::time::Utc;
use lab_core::versions;
use lab_core::view::LabView;
use lab_eden::Eden;
use lab_run::engine::{
    Alternate, Engine, Event, NspSource, Planned, READY, safe_name, with_diagnostics,
};
use lab_run::history::{self, VARIANT_A, VARIANT_B};
use lab_run::queue::{Holding, Queue, QueueEntry, QueueState, Turn, unix_now};
use lab_run::ready::{self, ReadyNsp};
use lab_run::report::{self, RunStamp};
use lab_run::run::{Clock, Emulator, Outcome, StepResult, stage_name};
use lab_run::tidy;
use serde::Serialize;

const REPOSITORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");
const DESKTOP: &str = "app/crates/carafe-desktop";
const RUNTIME_SYMBOLS: &str = "resources/runtime/symbols/wine-nx-runtime.elf";
const QUEUE_POLL: Duration = Duration::from_secs(2);
const PERF_WINDOW_MS: u64 = 10_000;
const LOG_PROBLEMS_SHOWN: usize = 15;
const KEPT_IMAGES: &str = "carafe-lab-eden-kept";
const REFERENCES_DIR: &str = "references";
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
    runtime: PathBuf,
    packer: HacBrewPacker,
    eden: Eden,
    scenario: Option<Scenario>,
    alternate: Option<(HacBrewPacker, PathBuf, String)>,
    references: PathBuf,
}

struct KeptParts {
    runtime: PathBuf,
    image: String,
    games: Vec<String>,
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
        Command::Perf => return show_perf(&config, args.json),
        Command::Tidy => return tidy_runs(&config, args.apply, args.json),
        Command::Bisect { game } => {
            let game = game
                .as_deref()
                .ok_or("bisect needs a game's name from lab.json")?;
            return bisect_game(&config, &args, game);
        }
        Command::Release { tag } => {
            let tag = tag
                .as_deref()
                .ok_or("release needs a tag, such as v0.2.3 or latest")?;
            return build_release(&config, tag);
        }
        Command::Nsp { file } => {
            let file = file.as_deref().ok_or("nsp needs an NSP file")?;
            return show_nsp(&config, file, args.json);
        }
        _ => {}
    }
    let keys = console_files(args.keys.as_deref(), &config_path)?;
    let gpu = args.gpu.unwrap_or(config.gpu);
    let desktop = carafe_checkout(&config).join(DESKTOP);
    let like = args
        .like
        .as_deref()
        .map(|dir| kept_parts(&config, dir))
        .transpose()?;
    let runtime = match (&args.runtime, &like) {
        (Some(file), _) => file.clone(),
        (None, Some(kept)) => kept.runtime.clone(),
        (None, None) => desktop.join("resources").join("carafe-runtime.bin"),
    };
    let packer = HacBrewPacker::new(
        runtime.clone(),
        desktop.join("binaries").join(hacbrewpack_name()),
    );
    let mut eden = Eden::new(
        config.work.join("eden").join("user"),
        &keys,
        gpu,
        config.emulator_cpu,
    )
    .with_settings(config.eden_settings.clone())
    .with_one_eden(config.one_eden)
    .with_fresh_eden(args.clean)
    .with_video(args.video)
    .with_firmware(local_settings(&config_path)?.firmware)
    .with_symbols(
        (args.runtime.is_none() && like.is_none()).then(|| desktop.join(RUNTIME_SYMBOLS)),
    );
    let image = match &like {
        Some(kept) => kept.image.clone(),
        None => config
            .eden_image
            .clone()
            .unwrap_or_else(|| lab_eden::IMAGE.to_owned()),
    };
    eden = eden.with_image(image.clone());
    let mut args = args;
    if let Some(kept) = &like {
        args.games.clone_from(&kept.games);
    }
    let timeout = args.timeout.unwrap_or(config.timeout);
    let scenario = args
        .scenario
        .as_deref()
        .map(|choice| plan_scenario(choice, &config.work, timeout))
        .transpose()?;
    let alternate = args
        .ab_runtime
        .as_deref()
        .map(|spec| releases::runtime(spec, &config.work, &carafe_checkout(&config)))
        .transpose()?
        .map(|(file, label)| {
            let packer = HacBrewPacker::new(
                file.clone(),
                desktop.join("binaries").join(hacbrewpack_name()),
            );
            (packer, file, label)
        });
    let lab = Lab {
        references: config.work.join(REFERENCES_DIR),
        alternate,
        work: config.work.clone(),
        gpu,
        cpu: config.emulator_cpu,
        lang: args.lang.unwrap_or(config.lang),
        timeout,
        scenario,
        args,
        keys,
        carafe: carafe_checkout(&config),
        image,
        desktop,
        runtime,
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
        Command::Play { game } => {
            let game = game.ok_or("play needs a game's name from lab.json")?;
            lab.play(&config, &game)
        }
        Command::Press { buttons } => {
            let buttons = buttons.ok_or("press needs buttons, such as A or A+ZR")?;
            lab.press(&config, &buttons)
        }
        Command::Tap { x, y } => {
            let (x, y) = x
                .zip(y)
                .ok_or("tap needs a point: x and y on the 1280×720 screen")?;
            lab.tap(&config, x, y)
        }
        Command::Screen { file } => {
            let file = file.ok_or("screen needs a file to save the picture to")?;
            lab.screen(&config, &file)
        }
        Command::Logs => lab.show_logs(&config),
        Command::Help
        | Command::Status
        | Command::Games
        | Command::Runs
        | Command::Stop { .. }
        | Command::Perf
        | Command::Nsp { .. }
        | Command::Release { .. }
        | Command::Bisect { .. }
        | Command::Tidy => Ok(ExitCode::SUCCESS),
    }
}

fn bisect_game(config: &Config, args: &Args, game: &str) -> Result<ExitCode, String> {
    config
        .pick(&[game.to_owned()])
        .map_err(|error| error.to_string())?;
    let carafe = carafe_checkout(config);
    let short = |revision: &str| git_out(&carafe, &["rev-parse", "--short=12", revision]);
    let good = short(
        args.good
            .as_deref()
            .ok_or("bisect needs --good <commit or tag>")?,
    )?;
    let bad = short(
        args.bad
            .as_deref()
            .ok_or("bisect needs --bad <commit or tag>")?,
    )?;
    let range = format!("{good}..{bad}");
    let between = git_out(
        &carafe,
        &[
            "log",
            "--reverse",
            "--first-parent",
            "--format=%h",
            "--abbrev=12",
            &range,
        ],
    )?;
    let mut commits = vec![good.clone()];
    commits.extend(between.lines().map(str::to_owned));
    if commits.last() != Some(&bad) || commits.len() < 2 {
        return Err(format!(
            "{bad} does not come after {good} on Carafe's main line"
        ));
    }
    let log =
        config
            .work
            .join("bisect")
            .join(format!("{}-{}.json", now().folder(), safe_name(game)));
    let mut verdicts: Vec<(String, bisect::Verdict)> = Vec::new();
    loop {
        let next = bisect::next(&commits, &verdicts);
        let record = serde_json::json!({
            "game": game,
            "good": good,
            "bad": bad,
            "commits": commits,
            "verdicts": verdicts,
            "next": next,
        });
        if let Some(dir) = log.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(
            &log,
            serde_json::to_string_pretty(&record).unwrap_or_default(),
        );
        match next {
            bisect::Next::Test { commit, left } => {
                println!("{left} commits in question; trying {commit}");
                let verdict = try_commit(config, args, game, &carafe, &commit);
                println!("{commit}: {verdict:?}");
                verdicts.push((commit, verdict));
            }
            bisect::Next::Found { commit } => {
                let subject = git_out(&carafe, &["log", "-1", "--format=%h %s", &commit])?;
                println!("first bad commit: {subject}");
                return Ok(ExitCode::SUCCESS);
            }
            bisect::Next::Unsure { commits } => {
                println!(
                    "one of these, which could not be tried: {}",
                    commits.join(", ")
                );
                return Ok(ExitCode::from(3));
            }
        }
    }
}

fn try_commit(
    config: &Config,
    args: &Args,
    game: &str,
    carafe: &Path,
    commit: &str,
) -> bisect::Verdict {
    let runtime = match releases::build_commit(carafe, &config.work, commit, &mut |line| {
        println!("  {line}")
    }) {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("  {error}");
            return bisect::Verdict::Skip;
        }
    };
    let Ok(exe) = std::env::current_exe() else {
        return bisect::Verdict::Skip;
    };
    let mut command = std::process::Command::new(exe);
    if let Some(file) = &args.config {
        command.arg("--config").arg(file);
    }
    command
        .args([
            "run",
            "--games",
            game,
            "--repeat",
            &args.repeat.to_string(),
            "--runtime",
        ])
        .arg(&runtime)
        .args(["--no-retry", "--json", "--who", "bisect"]);
    let Ok(output) = command.output() else {
        return bisect::Verdict::Skip;
    };
    let finished = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|event| event["event"] == "runFinished");
    match finished.and_then(|event| event["failed"].as_u64()) {
        Some(0) => bisect::Verdict::Good,
        Some(_) => bisect::Verdict::Bad,
        None => bisect::Verdict::Skip,
    }
}

fn git_out(checkout: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn build_release(config: &Config, name: &str) -> Result<ExitCode, String> {
    let carafe = carafe_checkout(config);
    let tag = releases::tag(&carafe, name)?;
    let kept = releases::build(&carafe, &config.work, &tag, &mut |line| println!("{line}"))?;
    println!("{}", kept.display());
    Ok(ExitCode::SUCCESS)
}

fn show_nsp(config: &Config, file: &Path, json: bool) -> Result<ExitCode, String> {
    let nsp = ready::read(file)?;
    let record = &nsp.record;
    let game = config
        .games
        .iter()
        .find(|game| same_folder(&game.folder, &record.source.folder))
        .map(|game| game.name.clone());
    let folder_found = Path::new(&record.source.folder).is_dir();
    if json {
        print_json(&serde_json::json!({
            "record": record,
            "game": game,
            "folderFound": folder_found,
        }))?;
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{} ({}), build {}",
        record.metadata.title, record.title_id, record.build_number
    );
    println!("runtime {}", record.runtime_version);
    println!("settings: {}", settings_label(&record.settings));
    println!(
        "debug: {}",
        serde_json::to_string(&record.settings.debug).map_err(|error| error.to_string())?
    );
    println!(
        "game folder: {} ({})",
        record.source.folder,
        if folder_found {
            "found: --rebuild can build it again"
        } else {
            "gone: it runs only as it is"
        }
    );
    if let Some(game) = game {
        println!("in lab.json as {game}");
    }
    Ok(ExitCode::SUCCESS)
}

fn tidy_runs(config: &Config, apply: bool, json: bool) -> Result<ExitCode, String> {
    let runs = config.work.join("runs");
    let nsp = config.work.join("nsp");
    let dumps = std::env::temp_dir().join("wsl-crashes");
    let places = tidy::Places {
        runs: &runs,
        nsp: &nsp,
        dumps: Some(&dumps),
    };
    let plan = tidy::plan(places, tidy::KEPT_WHOLE);
    if json && !apply {
        print_json(&plan)?;
        return Ok(ExitCode::SUCCESS);
    }
    let containers = stopped_containers();
    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    println!(
        "{} runs; the {} newest stay whole; compress {} log files of failed runs ({:.1} MB), remove {} files of \
         passing runs ({:.1} MB)",
        plan.runs,
        tidy::KEPT_WHOLE,
        plan.compress.len(),
        mib(plan.compress_bytes),
        plan.remove.len(),
        mib(plan.remove_bytes)
    );
    println!(
        "remove {} old NSPs and Eden crash dumps ({:.1} MB) and {} stopped lab containers",
        plan.stale.len(),
        mib(plan.stale_bytes),
        containers.len()
    );
    if !apply {
        println!("nothing changed; run with --apply to do it");
        return Ok(ExitCode::SUCCESS);
    }
    let freed = tidy::apply(&plan).map_err(|error| error.to_string())?;
    if !containers.is_empty() {
        let _ = std::process::Command::new("docker")
            .arg("rm")
            .args(&containers)
            .output();
    }
    println!("freed {:.1} MB", mib(freed));
    Ok(ExitCode::SUCCESS)
}

fn stopped_containers() -> Vec<String> {
    std::process::Command::new("docker")
        .args([
            "ps",
            "--all",
            "--quiet",
            "--filter",
            "name=carafe-lab",
            "--filter",
            "status=exited",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn show_perf(config: &Config, json: bool) -> Result<ExitCode, String> {
    let text = fs::read_to_string(
        config
            .work
            .join("eden")
            .join("user")
            .join("control")
            .join("stats"),
    )
    .unwrap_or_default();
    let samples = perf::parse_stats(&text);
    let newest = samples.last().map_or(0, |sample| sample.at);
    let recent: Vec<_> = samples
        .into_iter()
        .filter(|sample| sample.at + PERF_WINDOW_MS >= newest)
        .collect();
    if json {
        print_json(&recent)?;
        return Ok(ExitCode::SUCCESS);
    }
    match perf::summarize(&recent) {
        Some(summary) => println!(
            "{:.0} fps median, {:.0} fps worst tenth, speed {:.0}% over {} readings",
            summary.median_fps,
            summary.low_fps,
            summary.median_speed * 100.0,
            summary.samples
        ),
        None => println!("no game is drawing"),
    }
    Ok(ExitCode::SUCCESS)
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
    let roadmap = fs::read_to_string(repository().join("ROADMAP.md")).unwrap_or_default();
    let view = LabView::of(config, &roadmap);
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

fn kept_runtime(work: &Path, hash: &str) -> PathBuf {
    work.join("runtimes").join(format!("{hash}.bin"))
}

fn kept_image(id: &str) -> String {
    format!("{KEPT_IMAGES}:{id}")
}

fn keep_parts(work: &Path, runtime: &Path, image: &str, stamp: &RunStamp) {
    if let Some(hash) = &stamp.runtime_hash {
        let kept = kept_runtime(work, hash);
        if !kept.is_file()
            && kept
                .parent()
                .is_some_and(|parent| fs::create_dir_all(parent).is_ok())
        {
            let staged = kept.with_extension("part");
            if fs::copy(runtime, &staged).is_ok() {
                let _ = fs::rename(&staged, &kept);
            }
        }
    }
    if let Some(id) = &stamp.eden_image {
        let tag = kept_image(id);
        if tag != image {
            let _ = std::process::Command::new("docker")
                .args(["tag", image, &tag])
                .output();
        }
    }
}

fn kept_parts(config: &Config, dir: &str) -> Result<KeptParts, String> {
    let run = history::read(&config.work.join("runs"), dir)
        .ok_or_else(|| format!("{dir}: no such run"))?;
    if run.verdict.is_some() || run.planned.iter().any(|name| name.ends_with(VARIANT_A)) {
        return Err(format!(
            "{dir} is an A/B run; start it again from the Plans screen"
        ));
    }
    let hash = run
        .stamp
        .runtime_hash
        .ok_or_else(|| format!("{dir} does not say which runtime it packed with"))?;
    let runtime = kept_runtime(&config.work, &hash);
    if !runtime.is_file() {
        return Err(format!(
            "{dir} packed with runtime {hash}, which was not kept; only runs from now on keep theirs"
        ));
    }
    let id = run
        .stamp
        .eden_image
        .ok_or_else(|| format!("{dir} does not say which Eden image it ran"))?;
    let image = kept_image(&id);
    if image_id(&image).is_none() {
        return Err(format!(
            "{dir} ran Eden image {id}, which was not kept; only runs from now on keep theirs"
        ));
    }
    Ok(KeptParts {
        runtime,
        image,
        games: run.planned,
    })
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
    fn run_ready(mut self, config: &Config) -> Result<ExitCode, String> {
        if self.args.ab.is_some() {
            return Err("--ab does not take ready NSPs".to_owned());
        }
        let repeat = self.args.repeat as usize;
        let mut planned = Vec::new();
        let mut names = Vec::new();
        for path in &self.args.nsp {
            let nsp = ready::read(path)?;
            let game = config
                .games
                .iter()
                .find(|game| same_folder(&game.folder, &nsp.record.source.folder));
            let plan = self.plan_ready(game, nsp);
            names.push(plan.name.clone());
            planned.extend(std::iter::repeat_n(plan, repeat));
        }
        let mut what = names.join(", ");
        if self.args.rebuild {
            what = format!("rebuild {what}");
        }
        if repeat > 1 {
            what = format!("{what} ×{repeat}");
        }
        self.execute(planned, &what)
    }

    fn plan_ready(&self, game: Option<&Game>, nsp: ReadyNsp) -> Planned {
        let record = nsp.record.clone();
        let name = game.map_or_else(|| record.metadata.title.clone(), |game| game.name.clone());
        let scenario = self.scenario.clone().unwrap_or_else(|| {
            game.map_or_else(
                || Scenario::standard(self.timeout),
                |game| game.scenario(self.timeout),
            )
        });
        let threshold = game.map_or(versions::DEFAULT_THRESHOLD, |game| game.threshold);
        let folder = PathBuf::from(&record.source.folder);
        if self.args.rebuild {
            let planned = Planned {
                entropy: 0,
                name,
                folder,
                request: ready::rebuild_request(&nsp),
                label: settings_label(&record.settings),
                scenario,
                threshold,
                nsp: NspSource::Rebuild(Box::new(record)),
                diagnostics: Vec::new(),
            };
            return with_diagnostics(planned, &self.args.diagnostics);
        }
        let of_emulator: Vec<Diagnostic> = self
            .args
            .diagnostics
            .iter()
            .filter(|diagnostic| !diagnostic.in_the_nsp())
            .copied()
            .collect();
        let planned = Planned {
            entropy: 0,
            name: format!("{name}{READY}"),
            folder,
            request: Err("a ready NSP runs as it is".to_owned()),
            label: format!(
                "runtime {} · {}",
                record.runtime_version,
                settings_label(&record.settings)
            ),
            scenario,
            threshold,
            nsp: NspSource::Ready(Box::new(nsp)),
            diagnostics: Vec::new(),
        };
        with_diagnostics(planned, &of_emulator)
    }

    fn run_games(mut self, config: &Config, set: Option<&str>) -> Result<ExitCode, String> {
        if !self.args.nsp.is_empty() {
            return self.run_ready(config);
        }
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
        } else if let Some((_, _, label)) = &self.alternate {
            let [game] = games.as_slice() else {
                return Err(
                    "an A/B run takes exactly one game: name it with --games or --only".to_owned(),
                );
            };
            what = format!("A/B {} against runtime {label}", game.name);
            let variant_b = |lab: &Self| {
                let mut planned =
                    lab.plan(game, format!("{}{VARIANT_B}", game.name), &game.settings);
                planned.nsp = NspSource::Alternate;
                planned.label = format!("runtime {label} · {}", planned.label);
                planned
            };
            (0..repeat)
                .flat_map(|_| {
                    [
                        self.plan(game, format!("{}{VARIANT_A}", game.name), &game.settings),
                        variant_b(&self),
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
        let planned = Planned {
            entropy: config::name_entropy(&name),
            name,
            folder: game.folder.clone(),
            request,
            label: settings_label(settings),
            scenario: self
                .scenario
                .clone()
                .unwrap_or_else(|| game.scenario(self.timeout)),
            threshold: game.threshold,
            nsp: NspSource::Build,
            diagnostics: Vec::new(),
        };
        with_diagnostics(planned, &self.args.diagnostics)
    }

    fn play(mut self, config: &Config, name: &str) -> Result<ExitCode, String> {
        let game = config
            .pick(&[name.to_owned()])
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
            .ok_or("no such game")?;
        let mut planned = self.plan(game, game.name.clone(), &game.settings);
        planned.scenario = Scenario {
            steps: vec![
                Step::Launch,
                Step::WaitFor {
                    milestone: Milestone::FirstFrame,
                    timeout: self.timeout,
                },
                Step::Wait {
                    seconds: self.args.minutes as f32 * 60.0,
                },
            ],
        };
        let what = format!("play {}", game.name);
        self.execute(vec![planned], &what)
    }

    fn require_running(&self, config: &Config) -> Result<(), String> {
        if queue_of(config).state(unix_now()).holder.is_none() {
            return Err("no game is running: start one with carafe-lab play <game>".to_owned());
        }
        Ok(())
    }

    fn press(mut self, config: &Config, buttons: &str) -> Result<ExitCode, String> {
        self.require_running(config)?;
        let buttons: Vec<SwitchButton> = buttons
            .split('+')
            .map(|name| {
                serde_json::from_value(serde_json::Value::String(name.trim().to_uppercase()))
                    .map_err(|_| format!("{name} is not a Switch button"))
            })
            .collect::<Result<_, _>>()?;
        self.eden.join_running();
        self.eden
            .press(
                &buttons,
                Duration::from_millis(u64::from(self.args.hold_ms)),
            )
            .map_err(|error| error.to_string())?;
        Ok(ExitCode::SUCCESS)
    }

    fn show_logs(mut self, config: &Config) -> Result<ExitCode, String> {
        self.require_running(config)?;
        if !self.eden.join_running() {
            return Err(
                "no Eden with a control channel runs: logs reads the one Eden of a run".to_owned(),
            );
        }
        let title = self
            .eden
            .title()
            .ok_or("the running Eden has booted no game yet")?;
        let logs = self.eden.logs(title);
        let launch = Launch::read(&logs.overlay, &logs.runtime);
        let mut found = problems::problems("eden", &self.eden.emulator_log());
        found.extend(problems::problems("carafe", &logs.overlay));
        found.extend(problems::problems("runtime", &logs.runtime));
        if self.args.json {
            print_json(&serde_json::json!({
                "titleId": title.to_string(),
                "stage": launch.stage(),
                "launch": launch,
                "problems": found,
            }))?;
            return Ok(ExitCode::SUCCESS);
        }
        println!("{title}: got as far as {}", stage_name(launch.stage()));
        if let Some(at) = launch.loading_drawn_at {
            println!("  loading screen at {at} s");
        }
        if let Some((at, reason)) = &launch.loading_hidden {
            println!("  loading screen gone at {at} s: {}", reason.text());
        }
        if launch.stalls > 0 {
            println!("  {} stall reports", launch.stalls);
        }
        let counts = problems::count(&found);
        println!(
            "{} error lines, {} warning lines",
            counts.errors, counts.warnings
        );
        found.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.count.cmp(&a.count)));
        for problem in found.iter().take(LOG_PROBLEMS_SHOWN) {
            println!(
                "{:>6} {:<7} {}",
                problem.count, problem.source, problem.text
            );
        }
        Ok(ExitCode::SUCCESS)
    }

    fn tap(mut self, config: &Config, x: u32, y: u32) -> Result<ExitCode, String> {
        self.require_running(config)?;
        self.eden.join_running();
        self.eden.tap(x, y).map_err(|error| error.to_string())?;
        Ok(ExitCode::SUCCESS)
    }

    fn screen(mut self, config: &Config, file: &Path) -> Result<ExitCode, String> {
        self.require_running(config)?;
        self.eden.join_running();
        self.eden
            .screenshot(file)
            .map_err(|error| error.to_string())?;
        println!("{}", file.display());
        Ok(ExitCode::SUCCESS)
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
        runs.extend(probes::screen_runs(self.timeout));
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
        let stamp = RunStamp {
            carafe_commit: git_commit_with_changes(&self.carafe),
            runtime_hash: file_hash(&self.runtime),
            lab_commit: git_commit_with_changes(&repository()),
            eden_image: if self.args.build_only {
                None
            } else {
                image_id(&self.image)
            },
        };
        keep_parts(&self.work, &self.runtime, &self.image, &stamp);
        let mut engine = Engine {
            emulator: &mut self.eden,
            packer: &self.packer,
            keys: &self.keys,
            build_inputs: vec![
                self.runtime.clone(),
                self.desktop.join("binaries").join(hacbrewpack_name()),
            ],
            work: &self.work,
            lang,
            runtime_version: RUNTIME_VERSION,
            emulator_label,
            stamp,
            build_only: self.args.build_only,
            alternate: self
                .alternate
                .as_ref()
                .map(|(packer, file, label)| Alternate {
                    packer,
                    build_inputs: vec![
                        file.clone(),
                        self.desktop.join("binaries").join(hacbrewpack_name()),
                    ],
                    runtime_version: label.clone(),
                }),
            retry_failed: !self.args.no_retry
                && self.args.ab.is_none()
                && self.alternate.is_none()
                && what != "probes",
            references: self.references.clone(),
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
        };
        let report = engine.run(
            planned,
            &started_at.folder(),
            &started_at.minutes(),
            &clock,
            &mut on_event,
        )?;
        drop(holding);
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
        Event::BuildStage {
            stage,
            at: Some(at),
        } => println!("  {stage}  at {at:.1} s"),
        Event::BuildStage { stage, at: None } => println!("  {stage}"),
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
        Event::Stopped { left } => println!("stopped; {left} games left out"),
        Event::RunFinished { report, .. } => println!("report: {report}"),
        Event::RunStarted { .. } | Event::LaunchLogs { .. } | Event::GameFinished { .. } => {}
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
        threshold: versions::DEFAULT_THRESHOLD,
        nsp: NspSource::Build,
        diagnostics: Vec::new(),
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

fn plan_scenario(choice: &str, work: &Path, timeout: u32) -> Result<Scenario, String> {
    if choice == "standard" {
        return Ok(Scenario::standard(timeout));
    }
    let file = work.join(choice);
    let text = fs::read_to_string(&file).map_err(|error| format!("{choice}: {error}"))?;
    let scenario = Scenario::parse(&text).map_err(|error| format!("{choice}: {error}"))?;
    scenario
        .check()
        .map_err(|error| format!("{choice}: {error}"))?;
    Ok(scenario)
}

fn local_settings(config_path: &Path) -> Result<LocalSettings, String> {
    let local_file = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(local::LOCAL_FILE);
    LocalSettings::load(&local_file)
}

fn console_files(argument: Option<&Path>, config_path: &Path) -> Result<PathBuf, String> {
    let settings = local_settings(config_path)?;
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    local::find_keys(argument, &settings, appdata.as_deref())
        .map(|(path, _)| path)
        .ok_or_else(|| {
            "no console files to pack with: choose them in the window's Emulator screen or in Carafe, or pass --keys"
                .to_owned()
        })
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

fn same_folder(folder: &Path, recorded: &str) -> bool {
    let plain = |text: &str| {
        text.trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    plain(&folder.to_string_lossy()) == plain(recorded)
}

fn settings_label(settings: &AutorunSettings) -> String {
    let mode = match settings.input.mode {
        InputMode::Controller => "controller",
        InputMode::KeyboardMouse => "keyboard",
    };
    format!("{} {mode}", matrix::describe(settings))
}
