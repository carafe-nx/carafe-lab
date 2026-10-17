//! What the window knows about the lab: where its files are, and the slow answers it keeps for a while.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lab_core::config::{self, Config};
use lab_core::local;
use lab_run::queue::Queue;
use serde::Serialize;
use ts_rs::TS;

const REPOSITORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");
const DOCKER_FOR: Duration = Duration::from_secs(15);
const COMMIT_FOR: Duration = Duration::from_secs(30);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Whether the emulator can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DockerState {
    /// Docker is not installed or not on the path.
    Missing,
    /// Docker is installed but its engine is not running.
    Stopped,
    /// Docker runs, but the emulator's image is not built.
    NoImage,
    /// The emulator's image is there.
    Ready,
}

/// The lab's files and the answers kept for a while.
pub struct Lab {
    repository: PathBuf,
    docker: Mutex<Option<(Instant, String, DockerState)>>,
    commit: Mutex<Option<(Instant, PathBuf, Option<String>)>>,
}

impl Lab {
    /// Creates the lab of this repository.
    #[must_use]
    pub fn new() -> Self {
        Self {
            repository: config::tidy(Path::new(REPOSITORY)),
            docker: Mutex::new(None),
            commit: Mutex::new(None),
        }
    }

    /// Returns the repository's folder.
    #[must_use]
    pub fn repository(&self) -> &Path {
        &self.repository
    }

    /// Returns `lab.json`.
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.repository.join(config::CONFIG_FILE)
    }

    /// Returns `lab.local.json`.
    #[must_use]
    pub fn local_path(&self) -> PathBuf {
        self.repository.join(local::LOCAL_FILE)
    }

    /// Reads `lab.json`.
    ///
    /// # Errors
    ///
    /// The message of the [`config::ConfigError`] when it is missing or broken.
    pub fn config(&self) -> Result<Config, String> {
        Config::load(&self.config_path()).map_err(|error| error.to_string())
    }

    /// Returns the queue of the lab whose work folder is `work`.
    #[must_use]
    pub fn queue(work: &Path) -> Queue {
        Queue::new(work.join("queue"))
    }

    /// Returns whether the emulator's `image` can start, asking Docker at most every 15 seconds.
    pub fn docker(&self, image: &str) -> DockerState {
        let mut kept = self
            .docker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((at, kept_image, state)) = kept.as_ref()
            && at.elapsed() < DOCKER_FOR
            && kept_image == image
        {
            return *state;
        }
        let state = ask_docker(image);
        *kept = Some((Instant::now(), image.to_owned(), state));
        state
    }

    /// Returns the short commit of `checkout`, asking git at most every 30 seconds.
    pub fn commit(&self, checkout: &Path) -> Option<String> {
        let mut kept = self
            .commit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((at, kept_checkout, commit)) = kept.as_ref()
            && at.elapsed() < COMMIT_FOR
            && kept_checkout == checkout
        {
            return commit.clone();
        }
        let commit = output(Command::new("git").arg("-C").arg(checkout).args([
            "rev-parse",
            "--short",
            "HEAD",
        ]))
        .ok()
        .map(|text| text.trim().to_owned());
        *kept = Some((Instant::now(), checkout.to_path_buf(), commit.clone()));
        commit
    }

    /// Returns the command line program: next to the window's program, or the release build of the repository.
    #[must_use]
    pub fn cli(&self) -> PathBuf {
        let name = format!("carafe-lab{}", std::env::consts::EXE_SUFFIX);
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| {
                self.repository
                    .join("app")
                    .join("target")
                    .join("release")
                    .join(&name)
            })
    }
}

impl Default for Lab {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `command` set to start without a console window.
pub fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn ask_docker(image: &str) -> DockerState {
    let result = quiet(
        Command::new("docker")
            .args(["image", "ls", "--quiet"])
            .arg(image),
    )
    .output();
    match result {
        Err(_) => DockerState::Missing,
        Ok(done) if !done.status.success() => DockerState::Stopped,
        Ok(done) if done.stdout.iter().all(u8::is_ascii_whitespace) => DockerState::NoImage,
        Ok(_) => DockerState::Ready,
    }
}

fn output(command: &mut Command) -> Result<String, String> {
    let done = quiet(command).output().map_err(|error| error.to_string())?;
    if done.status.success() {
        Ok(String::from_utf8_lossy(&done.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&done.stderr).into_owned())
    }
}
