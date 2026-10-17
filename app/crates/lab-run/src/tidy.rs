//! Keeping the runs folder small: the newest runs and the failed ones keep their logs, compressed once they are old;
//! older runs where nothing failed keep only what History and the reports read.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;

use crate::engine::ALTERNATE_DIR;
use crate::history::{self, RunSummary};
use crate::ready::READY_DIR;
use crate::run::{Outcome, PROBLEMS_FILE, TIMELINE_DIR};

/// How many of the newest runs keep everything as it is.
pub const KEPT_WHOLE: usize = 10;

const KEPT_FILES: [&str; 3] = [PROBLEMS_FILE, "session.log", "container-end.log"];
const RAW_EXTENSIONS: [&str; 3] = ["log", "txt", "csv"];

/// What tidying would do to the runs folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyPlan {
    /// Runs looked at.
    pub runs: usize,
    /// Log files of failed or unfinished runs to compress into `.gz` next to them.
    pub compress: Vec<PathBuf>,
    /// Log files and timeline pictures of older runs where nothing failed, to remove.
    pub remove: Vec<PathBuf>,
    /// Bytes the files to compress take now.
    pub compress_bytes: u64,
    /// Bytes the files to remove take.
    pub remove_bytes: u64,
    /// NSPs no game of the newest runs uses, with their build keys, NSPs older runs built for the console and Eden's
    /// crash dumps, to remove.
    pub stale: Vec<PathBuf>,
    /// Bytes the stale files take.
    pub stale_bytes: u64,
}

/// Where tidying looks.
#[derive(Debug, Clone, Copy)]
pub struct Places<'a> {
    /// The runs folder.
    pub runs: &'a Path,
    /// The folder of built NSPs.
    pub nsp: &'a Path,
    /// The folder WSL writes crash dumps to; `None` leaves dumps alone.
    pub dumps: Option<&'a Path>,
}

/// Returns what tidying would do, keeping the `keep` newest runs whole and the NSPs of their games.
#[must_use]
pub fn plan(places: Places<'_>, keep: usize) -> TidyPlan {
    let runs = places.runs;
    let list = history::list(runs);
    let mut plan = TidyPlan {
        runs: list.len(),
        ..TidyPlan::default()
    };
    let used: Vec<String> = list
        .iter()
        .take(keep)
        .flat_map(|run| {
            run.games
                .iter()
                .map(|game| game.title_id.to_ascii_lowercase())
        })
        .collect();
    let mut stale = stale_nsps(places.nsp, &used);
    stale.extend(stale_nsps(&places.nsp.join(READY_DIR), &used));
    stale.extend(stale_nsps(&places.nsp.join(ALTERNATE_DIR), &used));
    if let Some(dumps) = places.dumps {
        stale.extend(eden_dumps(dumps));
    }
    for run in list.iter().skip(keep) {
        for game in run.games.iter().filter(|game| !game.dir.is_empty()) {
            let for_console = runs
                .join(&run.dir)
                .join(&game.dir)
                .join(format!("{}.nsp", game.title_id));
            if for_console.is_file() {
                stale.push(for_console);
            }
        }
    }
    plan.stale_bytes = stale
        .iter()
        .map(|file| fs::metadata(file).map_or(0, |metadata| metadata.len()))
        .sum();
    plan.stale = stale;
    for run in list.iter().skip(keep) {
        let failed = !run.finished || run.games.iter().any(|game| game.outcome == Outcome::Fail);
        for file in raw_files(&runs.join(&run.dir), run) {
            let size = fs::metadata(&file).map_or(0, |metadata| metadata.len());
            let picture = file
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == TIMELINE_DIR);
            if failed && picture {
                continue;
            }
            if failed {
                plan.compress_bytes += size;
                plan.compress.push(file);
            } else {
                plan.remove_bytes += size;
                plan.remove.push(file);
            }
        }
    }
    plan
}

/// Carries out `plan`: compresses and removes its files.
///
/// # Returns
///
/// The bytes freed.
///
/// # Errors
///
/// The first file that cannot be read, written or removed.
pub fn apply(plan: &TidyPlan) -> io::Result<u64> {
    let mut freed = 0;
    for file in &plan.compress {
        let before = fs::metadata(file)?.len();
        let packed = compress(file)?;
        freed += before.saturating_sub(fs::metadata(&packed)?.len());
    }
    for file in plan.remove.iter().chain(&plan.stale) {
        freed += fs::metadata(file).map_or(0, |metadata| metadata.len());
        fs::remove_file(file)?;
    }
    for dir in plan.remove.iter().filter_map(|file| file.parent()) {
        if dir.file_name().is_some_and(|name| name == TIMELINE_DIR) {
            let _ = fs::remove_dir(dir);
        }
    }
    Ok(freed)
}

fn stale_nsps(nsp: &Path, used: &[String]) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(nsp) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let is_build = path
                .extension()
                .is_some_and(|extension| extension == "nsp" || extension == "build-key");
            let title = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            is_build && !used.contains(&title)
        })
        .collect();
    files.sort();
    files
}

fn eden_dumps(dumps: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dumps) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            name.ends_with(".dmp") && name.contains("eden")
        })
        .collect();
    files.sort();
    files
}

fn compress(file: &Path) -> io::Result<PathBuf> {
    let mut name = file.as_os_str().to_owned();
    name.push(".gz");
    let packed = PathBuf::from(name);
    let mut reader = BufReader::new(File::open(file)?);
    let mut encoder = GzEncoder::new(
        BufWriter::new(File::create(&packed)?),
        Compression::default(),
    );
    io::copy(&mut reader, &mut encoder)?;
    encoder.finish()?;
    fs::remove_file(file)?;
    Ok(packed)
}

fn raw_files(run_dir: &Path, run: &RunSummary) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for game in run.games.iter().filter(|game| !game.dir.is_empty()) {
        let Ok(entries) = fs::read_dir(run_dir.join(&game.dir)) else {
            continue;
        };
        for launch in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let is_launch = launch
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("launch-"));
            if is_launch && launch.is_dir() {
                collect_raw(&launch, &mut files);
            }
        }
    }
    files.sort();
    files
}

fn collect_raw(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() {
            collect_raw(&path, files);
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let in_timeline = dir.file_name().is_some_and(|name| name == TIMELINE_DIR);
        let raw = path
            .extension()
            .is_some_and(|extension| RAW_EXTENSIONS.iter().any(|raw| extension == *raw));
        if in_timeline || (raw && !KEPT_FILES.contains(&name.as_str())) {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn run(runs: &Path, dir: &str, outcome: &str) {
        write(
            &runs.join(dir).join(history::SUMMARY_FILE),
            &format!(
                r#"{{"started":"x","seconds":1,"emulator":"e","runtimeVersion":"r","finished":true,"planned":["G"],
                "games":[{{"name":"G","titleId":"t","settings":"s","outcome":"{outcome}","dir":"G"}}]}}"#
            ),
        );
        let launch = runs.join(dir).join("G").join("launch-1");
        write(&launch.join("eden_log.txt"), &"line\n".repeat(200));
        write(&launch.join(PROBLEMS_FILE), "[]");
        write(&launch.join(TIMELINE_DIR).join("0010s.jpg"), "jpg");
        write(&runs.join(dir).join("G").join("first-frame.png"), "png");
    }

    #[test]
    fn old_passing_runs_lose_their_raw_logs_and_old_failures_are_compressed() {
        let runs = std::env::temp_dir().join(format!("lab-tidy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&runs);
        run(&runs, "2026-01-01_00-00-00", "pass");
        run(&runs, "2026-01-02_00-00-00", "fail");
        run(&runs, "2026-01-03_00-00-00", "pass");
        let nsp = runs.join("nsp");
        let dumps = runs.join("dumps");
        write(&nsp.join("t.nsp"), "kept");
        write(&nsp.join("t.build-key"), "kept");
        write(&nsp.join("0100aaaa.nsp"), "old");
        write(&nsp.join("0100aaaa.build-key"), "old");
        write(&dumps.join("wsl-crash-1-12-_opt_eden_bin_eden.dmp"), "dump");
        write(
            &dumps.join("wsl-crash-1-67-_usr_bin_weston.dmp"),
            "not ours",
        );
        let plan = plan(
            Places {
                runs: &runs,
                nsp: &nsp,
                dumps: Some(&dumps),
            },
            1,
        );
        assert_eq!(plan.runs, 3);
        assert_eq!(plan.compress.len(), 1);
        assert_eq!(plan.remove.len(), 2);
        assert!(plan.compress[0].ends_with("eden_log.txt"));
        assert_eq!(plan.stale.len(), 3);
        apply(&plan).unwrap();
        assert!(nsp.join("t.nsp").exists() && !nsp.join("0100aaaa.nsp").exists());
        assert!(dumps.join("wsl-crash-1-67-_usr_bin_weston.dmp").exists());
        let old = runs.join("2026-01-01_00-00-00").join("G");
        assert!(!old.join("launch-1").join("eden_log.txt").exists());
        assert!(!old.join("launch-1").join(TIMELINE_DIR).exists());
        assert!(old.join("launch-1").join(PROBLEMS_FILE).exists());
        assert!(old.join("first-frame.png").exists());
        let failed = runs.join("2026-01-02_00-00-00").join("G").join("launch-1");
        assert!(failed.join("eden_log.txt.gz").exists());
        assert!(!failed.join("eden_log.txt").exists());
        assert!(failed.join(TIMELINE_DIR).join("0010s.jpg").exists());
        let newest = runs.join("2026-01-03_00-00-00").join("G").join("launch-1");
        assert!(newest.join("eden_log.txt").exists());
        let _ = fs::remove_dir_all(&runs);
    }
}
