//! Keeping the runs folder small: the newest runs and the failed ones keep their logs, compressed once they are old;
//! older runs where nothing failed keep only what History and the reports read.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;

use crate::history::{self, RunSummary};
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
}

/// Returns what tidying the runs folder `runs` would do, keeping the `keep` newest runs whole.
#[must_use]
pub fn plan(runs: &Path, keep: usize) -> TidyPlan {
    let list = history::list(runs);
    let mut plan = TidyPlan {
        runs: list.len(),
        ..TidyPlan::default()
    };
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
    for file in &plan.remove {
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
        let plan = plan(&runs, 1);
        assert_eq!(plan.runs, 3);
        assert_eq!(plan.compress.len(), 1);
        assert_eq!(plan.remove.len(), 2);
        assert!(plan.compress[0].ends_with("eden_log.txt"));
        apply(&plan).unwrap();
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
