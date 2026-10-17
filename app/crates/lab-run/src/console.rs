//! What the console said about the games of a run: the verdicts people bring back after trying the NSPs on a
//! Switch, kept in `console.json` next to the run's summary.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The file in a run's folder with the console's verdicts.
pub const CONSOLE_FILE: &str = "console.json";

/// What the console said about one game of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConsoleVerdict {
    /// The game's folder inside the run's folder.
    pub game: String,
    /// The game worked on the console.
    pub works: bool,
    /// What was seen, in the tester's words; may be empty.
    pub note: String,
    /// When the verdict was given, `YYYY-MM-DD HH:MM` in UTC.
    pub at: String,
}

/// Returns the console's verdicts on the run in `run_dir`; empty when there are none or the file does not read.
#[must_use]
pub fn read(run_dir: &Path) -> Vec<ConsoleVerdict> {
    fs::read_to_string(run_dir.join(CONSOLE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Keeps `verdict` for the run in `run_dir`, in place of an earlier verdict on the same game.
///
/// # Errors
///
/// The file cannot be written.
pub fn record(run_dir: &Path, verdict: ConsoleVerdict) -> io::Result<()> {
    let mut verdicts = read(run_dir);
    verdicts.retain(|kept| kept.game != verdict.game);
    verdicts.push(verdict);
    let json = serde_json::to_string_pretty(&verdicts).map_err(io::Error::other)?;
    fs::write(run_dir.join(CONSOLE_FILE), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(game: &str, works: bool) -> ConsoleVerdict {
        ConsoleVerdict {
            game: game.to_owned(),
            works,
            note: String::new(),
            at: "2026-10-17 20:00".to_owned(),
        }
    }

    #[test]
    fn a_later_verdict_on_a_game_takes_the_place_of_the_earlier_one() {
        let dir = std::env::temp_dir().join(format!("lab-console-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(read(&dir).is_empty());
        record(&dir, verdict("OpenTTD", false)).unwrap();
        record(&dir, verdict("NFSU", true)).unwrap();
        record(&dir, verdict("OpenTTD", true)).unwrap();
        let kept = read(&dir);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|verdict| verdict.works));
        let _ = fs::remove_dir_all(&dir);
    }
}
