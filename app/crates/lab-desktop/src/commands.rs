//! Commands invoked by the window.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use carafe_core::icon::data_url;
use carafe_core::ports::{FolderInspector, KeysChecker, KeysReport};
use carafe_folder::DiskFolderInspector;
use carafe_keys::FileKeysChecker;
use lab_core::config;
use lab_core::emulator::Video;
use lab_core::firmware;
use lab_core::local::{self, KeysSource, LocalSettings};
use lab_core::matrix;
use lab_core::perf::{self, PerfSample};
use lab_core::scenario::Scenario;
use lab_core::time::Utc;
use lab_core::versions::{self, VersionStep};
use lab_core::view::LabView;
use lab_run::console::{self, ConsoleVerdict};
use lab_run::history::{self, EVENTS_FILE, GameProblems, RunSummary};
use lab_run::queue::{QueueState, unix_now};
use lab_run::ready;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use ts_rs::TS;

use crate::detached;
use crate::error::CommandError;
use crate::lab::{DockerState, Lab};

const RUN_LOGS: &str = "window-runs";
const KEPT_RUN_LOGS: usize = 20;
const RUN_LOG_TAIL: usize = 20;
const EXIT_POLL: Duration = Duration::from_secs(2);

/// What the title bar and the Emulator screen show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabStatus {
    /// `lab.json`.
    pub config: String,
    /// Why `lab.json` cannot be read; `None` when it reads.
    pub problem: Option<String>,
    /// The lab's work folder; empty when `lab.json` cannot be read.
    pub work: String,
    /// The Carafe checkout the lab packs with.
    pub carafe: String,
    /// Its short commit, when git knows it.
    pub carafe_commit: Option<String>,
    /// The checkout has a built runtime to pack with.
    pub runtime_built: bool,
    /// The emulator's Docker image.
    pub image: String,
    /// Whether the emulator can start.
    pub docker: DockerState,
    /// The console files Lab packs with.
    pub keys: KeysState,
    /// The Switch firmware Eden runs with.
    pub firmware: FirmwareState,
    /// Who holds the emulator and who waits.
    pub queue: QueueState,
    /// Free space on the drives the lab fills.
    pub disk: DiskState,
}

/// Free space on the drives the lab fills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiskState {
    /// Gigabytes free on the drive of the work folder; `None` when unknown.
    pub work_free_gb: Option<u32>,
    /// Gigabytes free on the drive of the temporary folder, where WSL writes Eden's crash dumps.
    pub temp_free_gb: Option<u32>,
    /// One of the drives has less than [`LOW_SPACE_GB`] left.
    pub low: bool,
}

/// Free gigabytes under which the title bar warns: about one Eden crash dump and a few NSPs.
pub const LOW_SPACE_GB: u32 = 15;

/// The console files Lab packs with, without their location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeysState {
    /// Where the choice comes from; `None` when nothing names the files.
    pub source: Option<KeysSource>,
    /// The file's name, without its folder.
    pub file_name: Option<String>,
    /// The file has what packing needs; `None` when it cannot be read.
    pub complete: Option<bool>,
    /// Why `lab.local.json` cannot be read, if it cannot.
    pub problem: Option<String>,
}

/// The Switch firmware Eden runs with, without its location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FirmwareState {
    /// The chosen folder's name, without its parent; `None` when Eden runs without firmware.
    pub folder_name: Option<String>,
    /// How many `.nca` files the folder has.
    pub files: u32,
    /// Why the choice cannot be used, if it cannot.
    pub problem: Option<String>,
}

/// The emulator's screen as the lab last saved it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Screen {
    /// The picture as a `data:` URL.
    pub url: String,
    /// When it was saved, in Unix milliseconds.
    #[ts(type = "number")]
    pub modified: u64,
}

/// A run the window started that ended with an error before or during its work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunExit {
    /// The exit code of `carafe-lab`; `None` when it was killed.
    pub code: Option<i32>,
    /// The last lines it printed.
    pub output: String,
}

/// Returns what the title bar and the Emulator screen show.
///
/// # Errors
///
/// [`CommandError`] if the background work fails.
#[tauri::command]
pub async fn get_status(lab: State<'_, Arc<Lab>>) -> Result<LabStatus, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || Ok(status(&lab))).await
}

/// Returns the games and sets of `lab.json`.
///
/// # Errors
///
/// [`CommandError`] with the reason when `lab.json` is missing or broken.
#[tauri::command]
pub async fn get_lab(lab: State<'_, Arc<Lab>>) -> Result<LabView, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let roadmap = fs::read_to_string(lab.repository().join("ROADMAP.md")).unwrap_or_default();
        Ok(LabView::of(&lab.config()?, &roadmap))
    })
    .await
}

/// Returns every game's performance at every Carafe commit, oldest first, with how each measure went since the
/// game's commit before.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is missing or broken.
#[tauri::command]
pub async fn version_steps(lab: State<'_, Arc<Lab>>) -> Result<Vec<VersionStep>, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let runs = history::list(&lab.config()?.work.join("runs"));
        Ok(versions::steps(&history::version_performance(&runs)))
    })
    .await
}

/// Returns the runs, newest first.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is missing or broken.
#[tauri::command]
pub async fn list_runs(lab: State<'_, Arc<Lab>>) -> Result<Vec<RunSummary>, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || Ok(history::list(&lab.config()?.work.join("runs")))).await
}

/// Returns the events of the run `dir` from the `from`-th on.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken or `dir` is not a run folder's name.
#[tauri::command]
pub async fn run_events(
    lab: State<'_, Arc<Lab>>,
    dir: String,
    from: usize,
) -> Result<Vec<Value>, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let file = run_dir(&lab, &dir)?.join(EVENTS_FILE);
        let Ok(opened) = File::open(file) else {
            return Ok(Vec::new());
        };
        Ok(BufReader::new(opened)
            .lines()
            .map_while(Result::ok)
            .skip(from)
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect())
    })
    .await
}

/// What the window asks `carafe-lab run` to do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunRequest {
    /// The set to run; `None` with no `games` runs every enabled game.
    pub set: Option<String>,
    /// Only the game with this name.
    pub only: Option<String>,
    /// These games of `lab.json`, in this order, instead of a set.
    pub games: Option<Vec<String>>,
    /// How many times every game runs in a row.
    pub repeat: Option<u32>,
    /// A/B: the settings of variant B, as JSON.
    pub ab: Option<String>,
    /// A new Eden for every launch instead of one for the whole run.
    pub clean: Option<bool>,
    /// The scenario every game runs instead of its own: `standard` or a file relative to `lab.json`.
    pub scenario: Option<String>,
    /// The live video: `full`, `low` or `off`; `None` means full.
    pub video: Option<String>,
    /// The folder of a past run to repeat with the runtime and Eden image it ran with.
    pub like: Option<String>,
    /// A ready NSP to run instead of the games of `lab.json`.
    pub nsp: Option<String>,
    /// Build the ready NSP again with the lab's runtime instead of running it as it is.
    pub rebuild: Option<bool>,
    /// Only build the NSPs, for the console.
    pub build_only: Option<bool>,
}

/// What a ready NSP was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NspInfo {
    /// The NSP file.
    pub path: String,
    /// The game's title.
    pub title: String,
    /// Its Title ID.
    pub title_id: String,
    /// Its build number.
    pub build_number: u32,
    /// The runtime version inside it.
    pub runtime_version: String,
    /// Its settings in a few words.
    pub settings: String,
    /// The runtime's verbose traces are on.
    pub verbose: bool,
    /// The runtime's profiler is on.
    pub profile: bool,
    /// The game folder it was built from.
    pub folder: String,
    /// The folder is still there, so the NSP can be built again.
    pub folder_found: bool,
    /// The game of `lab.json` with the same folder, if there is one.
    pub game: Option<String>,
}

/// Reads what the ready NSP at `path` was built from.
///
/// # Errors
///
/// [`CommandError`] when the file is not an NSP made by Carafe.
#[tauri::command]
pub async fn read_nsp(lab: State<'_, Arc<Lab>>, path: String) -> Result<NspInfo, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let nsp = ready::read(Path::new(&path))?;
        let record = nsp.record;
        let folder = PathBuf::from(&record.source.folder);
        let game = lab.config().ok().and_then(|config| {
            config
                .games
                .iter()
                .find(|game| same_folder(&game.folder, &folder))
                .map(|game| game.name.clone())
        });
        Ok(NspInfo {
            path,
            title: record.metadata.title.clone(),
            title_id: record.title_id.to_string(),
            build_number: record.build_number,
            runtime_version: record.runtime_version.clone(),
            settings: matrix::describe(&record.settings),
            verbose: record.settings.debug.verbose,
            profile: record.settings.debug.profile,
            folder_found: folder.is_dir(),
            folder: record.source.folder,
            game,
        })
    })
    .await
}

fn same_folder(one: &Path, other: &Path) -> bool {
    let plain = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    plain(one) == plain(other)
}

/// Starts `carafe-lab run` in its own process, so closing the window does not stop it, as `request` asks.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken or the program cannot start.
#[tauri::command]
pub async fn start_run(
    app: AppHandle,
    lab: State<'_, Arc<Lab>>,
    request: RunRequest,
) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
    let RunRequest {
        set,
        only,
        games,
        repeat,
        ab,
        clean,
        scenario,
        video,
        like,
        nsp,
        rebuild,
        build_only,
    } = request;
    blocking(move || {
        let work = lab.config()?.work;
        let logs = work.join("queue").join(RUN_LOGS);
        fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        keep_newest(&logs, KEPT_RUN_LOGS);
        let log_path = logs.join(format!("{}.log", unix_now()));
        let mut args = vec!["run".to_owned()];
        args.extend(set);
        if let Some(only) = only {
            args.extend(["--only".to_owned(), only]);
        }
        if let Some(games) = games.filter(|games| !games.is_empty()) {
            args.extend(["--games".to_owned(), games.join(",")]);
        }
        if let Some(repeat) = repeat.filter(|repeat| *repeat > 1) {
            args.extend(["--repeat".to_owned(), repeat.to_string()]);
        }
        if let Some(ab) = ab {
            let compact: serde_json::Value =
                serde_json::from_str(&ab).map_err(|error| format!("B: {error}"))?;
            args.extend(["--ab".to_owned(), compact.to_string()]);
        }
        if clean == Some(true) {
            args.push("--clean".to_owned());
        }
        if let Some(scenario) = scenario.filter(|scenario| !scenario.is_empty()) {
            args.extend(["--scenario".to_owned(), scenario]);
        }
        if let Some(video) = video.filter(|video| Video::parse(video).is_some()) {
            args.extend(["--video".to_owned(), video]);
        }
        if let Some(like) = like {
            if !plain_name(&like) {
                return Err("not a run folder".into());
            }
            args.extend(["--like".to_owned(), like]);
        }
        if let Some(nsp) = nsp {
            args.extend(["--nsp".to_owned(), nsp]);
            if rebuild == Some(true) {
                args.push("--rebuild".to_owned());
            }
        }
        if build_only == Some(true) {
            args.push("--build-only".to_owned());
        }
        args.extend(["--json", "--who", "window"].map(str::to_owned));
        detached::spawn(&lab.cli(), &args, lab.repository(), &log_path)?;
        std::thread::spawn(move || watch_exit(&app, &log_path));
        Ok(())
    })
    .await
}

/// Saves the scenario of the game `game` to its scenario file; a game whose scenario was standard or written in
/// `lab.json` gets `scenarios/<name>.json`, and `lab.json` points to it.
///
/// # Returns
///
/// The scenario file, relative to `lab.json`.
///
/// # Errors
///
/// [`CommandError`] when the scenario does not make sense, the game is unknown or a file cannot be written.
#[tauri::command]
pub async fn save_scenario(
    lab: State<'_, Arc<Lab>>,
    game: String,
    scenario: Scenario,
) -> Result<String, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        scenario.check().map_err(|error| error.to_string())?;
        let config = lab.config()?;
        let known = config
            .pick(std::slice::from_ref(&game))
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
            .ok_or("no such game")?;
        let file = known
            .scenario_file
            .clone()
            .unwrap_or_else(|| config::scenario_file_for(&known.name));
        let path = lab.repository().join(&file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(&scenario).map_err(|error| error.to_string())?;
        fs::write(&path, format!("{text}\n")).map_err(|error| error.to_string())?;
        if known.scenario_file.is_none() {
            let config_path = lab.config_path();
            let lab_text = fs::read_to_string(&config_path).map_err(|error| error.to_string())?;
            let base = lab.repository().to_path_buf();
            let updated = config::set_scenario_file(&lab_text, &known.name, &file, |name| {
                fs::read_to_string(base.join(name)).map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())?;
            fs::write(&config_path, updated).map_err(|error| error.to_string())?;
        }
        Ok(file)
    })
    .await
}

/// Keeps what the console said about the game in the folder `game` of the run `dir`.
///
/// # Errors
///
/// [`CommandError`] when `dir` or `game` is not a folder's name or the verdict cannot be written.
#[tauri::command]
pub async fn record_console_verdict(
    lab: State<'_, Arc<Lab>>,
    dir: String,
    game: String,
    works: bool,
    note: String,
) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        if !plain_name(&game) {
            return Err("not a game folder".into());
        }
        let run = run_dir(&lab, &dir)?;
        console::record(
            &run,
            ConsoleVerdict {
                game,
                works,
                note: note.trim().to_owned(),
                at: Utc::from_unix(unix_now()).minutes(),
            },
        )
        .map_err(|error| CommandError::from(error.to_string()))
    })
    .await
}

/// Replaces the tags and notes of the game `game` in `lab.json`.
#[tauri::command]
pub async fn save_game_notes(
    lab: State<'_, Arc<Lab>>,
    game: String,
    tags: Vec<String>,
    notes: String,
) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let config_path = lab.config_path();
        let text = fs::read_to_string(&config_path).map_err(|error| error.to_string())?;
        let base = lab.repository().to_path_buf();
        let updated = config::set_game_notes(&text, &game, &tags, &notes, |name| {
            fs::read_to_string(base.join(name)).map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())?;
        fs::write(&config_path, updated).map_err(|error| error.to_string())?;
        Ok(())
    })
    .await
}

/// Returns the problems in the logs of the game in the folder `game` of the run `dir`, and which of them the same
/// game's run before did not have.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken or either name is not a folder name of the run.
#[tauri::command]
pub async fn game_problems(
    lab: State<'_, Arc<Lab>>,
    dir: String,
    game: String,
) -> Result<GameProblems, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        if !plain_name(&game) {
            return Err("not a game folder".into());
        }
        run_dir(&lab, &dir)?;
        Ok(history::game_problems(
            &lab.config()?.work.join("runs"),
            &dir,
            &game,
        ))
    })
    .await
}

/// Returns the screenshots a game of the run `dir` saved, as paths sorted by name.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken or either name is not a folder name of the run.
#[tauri::command]
pub async fn run_screenshots(
    lab: State<'_, Arc<Lab>>,
    dir: String,
    game: String,
) -> Result<Vec<String>, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        if !plain_name(&game) {
            return Err("not a game folder".into());
        }
        let folder = run_dir(&lab, &dir)?.join(game);
        let Ok(entries) = fs::read_dir(folder) else {
            return Ok(Vec::new());
        };
        let mut shots: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
            .map(|path| path.display().to_string())
            .collect();
        shots.sort();
        Ok(shots)
    })
    .await
}

/// Adds the game in `folder` to `lab.json` under a name made from the folder's.
///
/// # Returns
///
/// The name the game got.
///
/// # Errors
///
/// [`CommandError`] when the folder has no Windows game, `lab.json` is broken or cannot be written.
#[tauri::command]
pub async fn add_game(lab: State<'_, Arc<Lab>>, folder: String) -> Result<String, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let report = DiskFolderInspector
            .inspect(&folder)
            .map_err(|error| CommandError::from(error.to_string()))?;
        if report.executables.is_empty() {
            return Err("the folder has no Windows game: no .exe inside".into());
        }
        let config_path = lab.config_path();
        let text = fs::read_to_string(&config_path).map_err(|error| error.to_string())?;
        let taken: Vec<String> = lab
            .config()?
            .games
            .into_iter()
            .map(|game| game.name)
            .collect();
        let folder = PathBuf::from(folder);
        let name = config::game_name_for(&folder, &taken);
        let base = lab.repository().to_path_buf();
        let updated = config::add_game(&text, &name, &folder, |file| {
            fs::read_to_string(base.join(file)).map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())?;
        fs::write(&config_path, updated).map_err(|error| error.to_string())?;
        Ok(name)
    })
    .await
}

/// Chooses the console files Lab packs with, for this PC only.
///
/// # Errors
///
/// [`CommandError`] when the file lacks what packing needs or `lab.local.json` cannot be written.
#[tauri::command]
pub async fn choose_keys(
    lab: State<'_, Arc<Lab>>,
    path: String,
) -> Result<KeysState, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let report = FileKeysChecker
            .check(&path)
            .map_err(|_| CommandError::from("the file cannot be read"))?;
        if !report.is_complete() {
            return Err("the file lacks what packing needs".into());
        }
        let file = lab.local_path();
        let mut settings = LocalSettings::load(&file)?;
        settings.keys = Some(PathBuf::from(path));
        settings.save(&file)?;
        Ok(keys_state(&lab))
    })
    .await
}

/// Forgets the console files chosen in the window, so Lab takes the ones chosen in Carafe.
///
/// # Errors
///
/// [`CommandError`] when `lab.local.json` cannot be read or written.
#[tauri::command]
pub async fn forget_keys(lab: State<'_, Arc<Lab>>) -> Result<KeysState, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let file = lab.local_path();
        let mut settings = LocalSettings::load(&file)?;
        settings.keys = None;
        settings.save(&file)?;
        Ok(keys_state(&lab))
    })
    .await
}

/// Chooses the folder `path` as the Switch firmware Eden runs with.
///
/// # Errors
///
/// [`CommandError`] when the folder has no `.nca` files or `lab.local.json` cannot be written.
#[tauri::command]
pub async fn choose_firmware(
    lab: State<'_, Arc<Lab>>,
    path: String,
) -> Result<FirmwareState, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let folder = PathBuf::from(path);
        firmware::read_folder(&folder)
            .map_err(|_| CommandError::from("the folder has no .nca files"))?;
        let file = lab.local_path();
        let mut settings = LocalSettings::load(&file)?;
        settings.firmware = Some(folder);
        settings.save(&file)?;
        Ok(firmware_state(&lab))
    })
    .await
}

/// Forgets the firmware, so Eden runs without it.
///
/// # Errors
///
/// [`CommandError`] when `lab.local.json` cannot be read or written.
#[tauri::command]
pub async fn forget_firmware(lab: State<'_, Arc<Lab>>) -> Result<FirmwareState, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let file = lab.local_path();
        let mut settings = LocalSettings::load(&file)?;
        settings.firmware = None;
        settings.save(&file)?;
        Ok(firmware_state(&lab))
    })
    .await
}

/// Asks the run `id`, or the run holding the emulator, to stop.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken, no run holds the emulator or the request cannot be written.
#[tauri::command]
pub async fn stop_run(lab: State<'_, Arc<Lab>>, id: Option<String>) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let queue = Lab::queue(&lab.config()?.work);
        let id = match id {
            Some(id) => id,
            None => queue
                .state(unix_now())
                .holder
                .map(|holder| holder.id)
                .ok_or("no run holds the emulator")?,
        };
        Ok(queue.request_stop(&id)?)
    })
    .await
}

/// Opens a file or folder of the lab in the system's program for it.
///
/// # Errors
///
/// [`CommandError`] when the path is outside the lab or does not exist, or nothing can open it.
#[tauri::command]
pub async fn open_path(lab: State<'_, Arc<Lab>>, path: String) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let path = inside_lab(&lab, &path)?;
        let opener = if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        Command::new(opener)
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|error| CommandError::from(error.to_string()))
    })
    .await
}

/// Returns the emulator's performance readings of the last `seconds` seconds, oldest first; empty when the one Eden
/// is not running or tells nothing.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken.
#[tauri::command]
pub async fn live_perf(
    lab: State<'_, Arc<Lab>>,
    seconds: u64,
) -> Result<Vec<PerfSample>, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let file = lab
            .config()?
            .work
            .join("eden")
            .join("user")
            .join("control")
            .join("stats");
        let text = fs::read_to_string(file).unwrap_or_default();
        let samples = perf::parse_stats(&text);
        let newest = samples.last().map_or(0, |sample| sample.at);
        let from = newest.saturating_sub(seconds * 1000);
        Ok(samples
            .into_iter()
            .filter(|sample| sample.at >= from)
            .collect())
    })
    .await
}

/// Returns the emulator's screen as the lab last saved it, or `None` if it has not saved one.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken.
#[tauri::command]
pub async fn read_screen(lab: State<'_, Arc<Lab>>) -> Result<Option<Screen>, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let file = lab
            .config()?
            .work
            .join("eden")
            .join("user")
            .join("screenshots")
            .join("lab-screen.png");
        Ok(png_screen(&file))
    })
    .await
}

/// Returns a PNG picture of the lab's work folder as a `data:` URL.
///
/// # Errors
///
/// [`CommandError`] when the path is outside the work folder, is not a PNG or cannot be read.
#[tauri::command]
pub async fn read_image(lab: State<'_, Arc<Lab>>, path: String) -> Result<String, CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let path = inside_lab(&lab, &path)?;
        let is_png = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
        if !is_png {
            return Err("only PNG pictures are shown".into());
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        Ok(data_url("image/png", &bytes))
    })
    .await
}

fn status(lab: &Lab) -> LabStatus {
    let config_path = lab.config_path().display().to_string();
    let config = match lab.config() {
        Ok(config) => config,
        Err(problem) => {
            let carafe = lab.repository().join("external").join("carafe");
            return LabStatus {
                config: config_path,
                problem: Some(problem),
                work: String::new(),
                carafe_commit: lab.commit(&carafe),
                runtime_built: false,
                carafe: carafe.display().to_string(),
                image: lab_eden::IMAGE.to_owned(),
                docker: lab.docker(lab_eden::IMAGE),
                keys: keys_state(lab),
                firmware: firmware_state(lab),
                queue: QueueState::default(),
                disk: disk_state(None),
            };
        }
    };
    let carafe = config.carafe_checkout(lab.repository());
    let image = config
        .eden_image
        .clone()
        .unwrap_or_else(|| lab_eden::IMAGE.to_owned());
    LabStatus {
        config: config_path,
        problem: None,
        work: config.work.display().to_string(),
        carafe_commit: lab.commit(&carafe),
        runtime_built: carafe
            .join("app")
            .join("crates")
            .join("carafe-desktop")
            .join("resources")
            .join("carafe-runtime.bin")
            .is_file(),
        carafe: carafe.display().to_string(),
        docker: lab.docker(&image),
        image,
        keys: keys_state(lab),
        firmware: firmware_state(lab),
        queue: Lab::queue(&config.work).state(unix_now()),
        disk: disk_state(Some(&config.work)),
    }
}

fn disk_state(work: Option<&Path>) -> DiskState {
    let free_gb = |path: &Path| {
        let existing = path.ancestors().find(|place| place.exists())?;
        let bytes = fs4::available_space(existing).ok()?;
        u32::try_from(bytes / (1024 * 1024 * 1024)).ok()
    };
    let work_free_gb = work.and_then(free_gb);
    let temp_free_gb = free_gb(&std::env::temp_dir());
    let low = [work_free_gb, temp_free_gb]
        .into_iter()
        .flatten()
        .any(|gb| gb < LOW_SPACE_GB);
    DiskState {
        work_free_gb,
        temp_free_gb,
        low,
    }
}

fn firmware_state(lab: &Lab) -> FirmwareState {
    let folder = match LocalSettings::load(&lab.local_path()) {
        Ok(settings) => settings.firmware,
        Err(problem) => {
            return FirmwareState {
                folder_name: None,
                files: 0,
                problem: Some(problem),
            };
        }
    };
    let Some(folder) = folder else {
        return FirmwareState {
            folder_name: None,
            files: 0,
            problem: None,
        };
    };
    let folder_name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    match firmware::read_folder(&folder) {
        Ok(files) => FirmwareState {
            folder_name,
            files: u32::try_from(files.len()).unwrap_or(u32::MAX),
            problem: None,
        },
        Err(_) => FirmwareState {
            folder_name,
            files: 0,
            problem: Some("the folder cannot be read or has no .nca files".to_owned()),
        },
    }
}

fn keys_state(lab: &Lab) -> KeysState {
    let (settings, problem) = match LocalSettings::load(&lab.local_path()) {
        Ok(settings) => (settings, None),
        Err(problem) => (LocalSettings::default(), Some(problem)),
    };
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let Some((path, source)) = local::find_keys(None, &settings, appdata.as_deref()) else {
        return KeysState {
            source: None,
            file_name: None,
            complete: None,
            problem,
        };
    };
    KeysState {
        source: Some(source),
        file_name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        complete: FileKeysChecker
            .check(&path.to_string_lossy())
            .ok()
            .map(KeysReport::is_complete),
        problem,
    }
}

fn run_dir(lab: &Lab, dir: &str) -> Result<PathBuf, CommandError> {
    if !plain_name(dir) {
        return Err("not a run folder".into());
    }
    Ok(lab.config()?.work.join("runs").join(dir))
}

fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn inside_lab(lab: &Lab, path: &str) -> Result<PathBuf, CommandError> {
    let path = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("{path}: {error}"))?;
    let mut roots = vec![lab.repository().to_path_buf()];
    if let Ok(config) = lab.config() {
        roots.extend(config.games.into_iter().map(|game| game.folder));
        roots.push(config.work);
    }
    let inside = roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| path.starts_with(root));
    if inside {
        Ok(path)
    } else {
        Err("the path is outside the lab".into())
    }
}

fn png_screen(file: &Path) -> Option<Screen> {
    let modified = fs::metadata(file)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?;
    let bytes = fs::read(file).ok()?;
    Some(Screen {
        url: data_url("image/png", &bytes),
        modified: u64::try_from(modified.as_millis()).unwrap_or(u64::MAX),
    })
}

fn watch_exit(app: &AppHandle, log: &Path) {
    loop {
        std::thread::sleep(EXIT_POLL);
        let text = fs::read_to_string(log).unwrap_or_default();
        let Some(code) = detached::exit_code(&text) else {
            continue;
        };
        if !matches!(code, 0 | 3 | 4) {
            let lines: Vec<&str> = text
                .lines()
                .filter(|line| !line.trim().starts_with(detached::EXIT_MARK))
                .collect();
            let output = lines[lines.len().saturating_sub(RUN_LOG_TAIL)..].join("\n");
            let _ = app.emit(
                "run://exited",
                RunExit {
                    code: Some(code),
                    output,
                },
            );
        }
        return;
    }
}

fn keep_newest(dir: &Path, count: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "log"))
        .collect();
    logs.sort();
    let extra = logs.len().saturating_sub(count);
    for old in &logs[..extra] {
        let _ = fs::remove_file(old);
    }
}

async fn blocking<T, F>(work: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CommandError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::from(error.to_string()))?
}
