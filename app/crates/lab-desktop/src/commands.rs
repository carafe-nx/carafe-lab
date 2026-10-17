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
use lab_core::local::{self, KeysSource, LocalSettings};
use lab_core::scenario::Scenario;
use lab_core::view::LabView;
use lab_run::history::{self, EVENTS_FILE, RunSummary};
use lab_run::queue::{QueueState, unix_now};
use serde::Serialize;
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
    /// Who holds the emulator and who waits.
    pub queue: QueueState,
}

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
    blocking(move || Ok(LabView::of(&lab.config()?))).await
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

/// Starts `carafe-lab run` in its own process, so closing the window does not stop it: the set `set`, or the games
/// named in `games`, each `repeat` times; `only` narrows it to one game.
///
/// # Errors
///
/// [`CommandError`] when `lab.json` is broken or the program cannot start.
#[tauri::command]
pub async fn start_run(
    app: AppHandle,
    lab: State<'_, Arc<Lab>>,
    set: Option<String>,
    only: Option<String>,
    games: Option<Vec<String>>,
    repeat: Option<u32>,
) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
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
                queue: QueueState::default(),
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
        queue: Lab::queue(&config.work).state(unix_now()),
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
