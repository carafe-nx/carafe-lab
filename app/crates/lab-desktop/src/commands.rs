//! Commands invoked by the window.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use carafe_core::icon::data_url;
use carafe_core::ports::{KeysChecker, KeysReport};
use carafe_keys::FileKeysChecker;
use lab_core::local::{self, KeysSource, LocalSettings};
use lab_core::view::LabView;
use lab_run::history::{self, EVENTS_FILE, RunSummary};
use lab_run::queue::{QueueState, unix_now};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use ts_rs::TS;

use crate::error::CommandError;
use crate::lab::{DockerState, Lab, quiet};

const RUN_LOG: &str = "window-run.log";
const RUN_LOG_TAIL: usize = 20;

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

/// Starts `carafe-lab run` in its own process, so closing the window does not stop it.
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
) -> Result<(), CommandError> {
    let lab = Arc::clone(&lab);
    blocking(move || {
        let work = lab.config()?.work;
        let queue_dir = work.join("queue");
        fs::create_dir_all(&queue_dir).map_err(|error| error.to_string())?;
        let log_path = queue_dir.join(RUN_LOG);
        let log = File::create(&log_path).map_err(|error| error.to_string())?;
        let log_err = log.try_clone().map_err(|error| error.to_string())?;
        let mut command = Command::new(lab.cli());
        command.arg("run");
        if let Some(set) = &set {
            command.arg(set);
        }
        if let Some(only) = &only {
            command.args(["--only", only]);
        }
        command
            .args(["--json", "--who", "window"])
            .current_dir(lab.repository())
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err);
        let mut child = quiet(&mut command)
            .spawn()
            .map_err(|error| format!("{}: {error}", lab.cli().display()))?;
        std::thread::spawn(move || {
            let Ok(status) = child.wait() else {
                return;
            };
            if !status.success() && status.code() != Some(3) {
                let _ = app.emit(
                    "run://exited",
                    RunExit {
                        code: status.code(),
                        output: tail(&log_path, RUN_LOG_TAIL),
                    },
                );
            }
        });
        Ok(())
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
    let plain = !dir.is_empty()
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !plain {
        return Err("not a run folder".into());
    }
    Ok(lab.config()?.work.join("runs").join(dir))
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

fn tail(file: &Path, lines: usize) -> String {
    let text = fs::read_to_string(file).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
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
