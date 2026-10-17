//! The noVNC client library the Run screen draws the live video with, served to the window under the `novnc`
//! scheme from a copy of the files in the Eden image, so the client always matches the server in the container.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use tauri::http::{Response, StatusCode, header};

use crate::lab::{Lab, quiet};

/// The scheme the window loads the library from.
pub const SCHEME: &str = "novnc";

const FOLDERS: [&str; 2] = ["core", "vendor"];
const IN_IMAGE: &str = "/usr/share/novnc";

static COPYING: Mutex<()> = Mutex::new(());

/// Returns the library file at `path`, such as `/core/rfb.js`, copying the library out of the Eden image the first
/// time; a path outside the library or a file that is not there gets 404.
pub fn respond(lab: &Lab, path: &str) -> Response<Vec<u8>> {
    match read(lab, path) {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "text/javascript")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(bytes),
        Err(error) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(error.into_bytes()),
    }
    .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn read(lab: &Lab, path: &str) -> Result<Vec<u8>, String> {
    let relative = Path::new(path.trim_start_matches('/'));
    let plain = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    let inside = relative
        .components()
        .next()
        .is_some_and(|first| FOLDERS.iter().any(|folder| first.as_os_str() == *folder));
    let script = relative
        .extension()
        .is_some_and(|extension| extension == "js");
    if !plain || !inside || !script {
        return Err(format!("{path} is not a file of the noVNC library"));
    }
    let library = library(lab)?;
    fs::read(library.join(relative)).map_err(|error| format!("{path}: {error}"))
}

fn library(lab: &Lab) -> Result<PathBuf, String> {
    let config = lab.config()?;
    let folder = config.work.join("novnc");
    if folder.join("core").join("rfb.js").is_file() {
        return Ok(folder);
    }
    let _copying = COPYING
        .lock()
        .map_err(|_| "the copy of noVNC failed".to_owned())?;
    if folder.join("core").join("rfb.js").is_file() {
        return Ok(folder);
    }
    let image = config
        .eden_image
        .unwrap_or_else(|| lab_eden::IMAGE.to_owned());
    let partial = config.work.join("novnc.part");
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|error| error.to_string())?;
    let container = docker(&["create", &image])?.trim().to_owned();
    let copied = FOLDERS.iter().try_for_each(|name| {
        let from = format!("{container}:{IN_IMAGE}/{name}");
        let to = partial.join(name);
        docker(&["cp", &from, &to.to_string_lossy()]).map(|_| ())
    });
    let _ = docker(&["rm", &container]);
    copied?;
    let _ = fs::remove_dir_all(&folder);
    fs::rename(&partial, &folder).map_err(|error| error.to_string())?;
    Ok(folder)
}

fn docker(args: &[&str]) -> Result<String, String> {
    let done = quiet(Command::new("docker").args(args))
        .output()
        .map_err(|error| error.to_string())?;
    if done.status.success() {
        Ok(String::from_utf8_lossy(&done.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&done.stderr).trim().to_owned())
    }
}
