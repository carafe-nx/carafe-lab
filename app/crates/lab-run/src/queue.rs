//! The queue for the emulator: one run holds it, the others wait in the order they came.
//!
//! The queue is a folder of small JSON files, so the command line, the window and every other session see the same
//! queue without a server. Each process refreshes its entry every few seconds; an entry that has not been refreshed
//! for [`STALE_SECONDS`] belongs to a process that died and is ignored.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use carafe_core::cancel::CancelToken;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Seconds after which an entry nobody refreshed is taken for a dead process.
pub const STALE_SECONDS: u64 = 30;

const HOLDER_FILE: &str = "holder.json";
const WAITING_DIR: &str = "waiting";
const BEAT: Duration = Duration::from_secs(5);

/// A run in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueEntry {
    /// Unique among the entries: the time it joined and the process.
    pub id: String,
    /// Who started the run: `window`, `Claude`, `command line`…
    pub who: String,
    /// What it runs, such as the set's name.
    pub what: String,
    /// The process.
    pub pid: u32,
    /// When it joined the queue, in Unix seconds.
    #[ts(type = "number")]
    pub since: u64,
    /// When the process last refreshed the entry, in Unix seconds.
    #[ts(type = "number")]
    pub beat: u64,
    /// The run's folder, once the run has started.
    #[serde(default)]
    pub run_dir: Option<String>,
}

/// Who holds the emulator and who waits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueState {
    /// The run that holds the emulator.
    pub holder: Option<QueueEntry>,
    /// The runs waiting, first in line first.
    pub waiting: Vec<QueueEntry>,
}

/// What [`Queue::try_take`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Turn {
    /// The emulator is this run's now.
    Taken,
    /// Not yet.
    Wait {
        /// Who holds the emulator; empty when it is free but an earlier run is first in line.
        holder: String,
        /// Runs ahead of this one, the holder's included.
        ahead: usize,
    },
}

/// The queue in a folder.
#[derive(Debug, Clone)]
pub struct Queue {
    dir: PathBuf,
}

impl Queue {
    /// Returns the queue kept in `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Returns who holds the emulator and who waits at `now`, without the entries of dead processes.
    #[must_use]
    pub fn state(&self, now: u64) -> QueueState {
        let holder = read_entry(&self.dir.join(HOLDER_FILE)).filter(|entry| alive(entry, now));
        let mut waiting: Vec<QueueEntry> = self
            .waiting_files()
            .iter()
            .filter_map(|path| read_entry(path))
            .filter(|entry| alive(entry, now))
            .collect();
        waiting.sort_by(|a, b| (a.since, &a.id).cmp(&(b.since, &b.id)));
        QueueState { holder, waiting }
    }

    /// Puts a run at the end of the queue.
    ///
    /// # Errors
    ///
    /// A message when the queue's folder cannot be written.
    pub fn join(&self, who: &str, what: &str, pid: u32, now: u64) -> Result<QueueEntry, String> {
        let entry = QueueEntry {
            id: format!("{now}-{pid}"),
            who: who.to_owned(),
            what: what.to_owned(),
            pid,
            since: now,
            beat: now,
            run_dir: None,
        };
        write_entry(&self.waiting_file(&entry.id), &entry)?;
        Ok(entry)
    }

    /// Takes the emulator for `entry` if it is free and `entry` is first in line.
    ///
    /// # Errors
    ///
    /// A message when the queue's files cannot be written.
    pub fn try_take(&self, entry: &mut QueueEntry, now: u64) -> Result<Turn, String> {
        entry.beat = now;
        let state = self.state(now);
        let place = state
            .waiting
            .iter()
            .position(|waiting| waiting.id == entry.id);
        if let Some(holder) = state.holder {
            return Ok(Turn::Wait {
                holder: holder.who,
                ahead: place.unwrap_or(state.waiting.len()) + 1,
            });
        }
        if let Some(place) = place.filter(|place| *place > 0) {
            return Ok(Turn::Wait {
                holder: String::new(),
                ahead: place,
            });
        }
        let holder_file = self.dir.join(HOLDER_FILE);
        if read_entry(&holder_file).is_some() || broken(&holder_file, now) {
            let _ = fs::remove_file(&holder_file);
        }
        for path in self.waiting_files() {
            if broken(&path, now) {
                let _ = fs::remove_file(path);
            }
        }
        match create_entry(&holder_file, entry) {
            Ok(()) => {
                let _ = fs::remove_file(self.waiting_file(&entry.id));
                Ok(Turn::Taken)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(Turn::Wait {
                holder: String::new(),
                ahead: 1,
            }),
            Err(error) => Err(format!("{}: {error}", holder_file.display())),
        }
    }

    /// Refreshes a waiting entry so it is not taken for a dead process.
    ///
    /// # Errors
    ///
    /// A message when the file cannot be written.
    pub fn beat_waiting(&self, entry: &mut QueueEntry, now: u64) -> Result<(), String> {
        entry.beat = now;
        write_entry(&self.waiting_file(&entry.id), entry)
    }

    /// Refreshes the holder's entry, if `entry` still holds the emulator.
    ///
    /// # Errors
    ///
    /// A message when the file cannot be written.
    pub fn beat_holder(&self, entry: &mut QueueEntry, now: u64) -> Result<(), String> {
        entry.beat = now;
        let file = self.dir.join(HOLDER_FILE);
        match read_entry(&file) {
            Some(holder) if holder.id == entry.id => write_entry(&file, entry),
            _ => Ok(()),
        }
    }

    /// Takes `entry` out of the queue, whether it waits or holds the emulator.
    pub fn leave(&self, entry: &QueueEntry) {
        let _ = fs::remove_file(self.waiting_file(&entry.id));
        let file = self.dir.join(HOLDER_FILE);
        if read_entry(&file).is_some_and(|holder| holder.id == entry.id) {
            let _ = fs::remove_file(file);
        }
        let _ = fs::remove_file(self.stop_file(&entry.id));
    }

    /// Asks the run `id` to stop: a waiting run leaves the queue, a running one stops between steps.
    ///
    /// # Errors
    ///
    /// A message when the request cannot be written.
    pub fn request_stop(&self, id: &str) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        fs::write(self.stop_file(id), b"").map_err(|error| error.to_string())
    }

    /// Returns whether someone asked the run `id` to stop.
    #[must_use]
    pub fn stop_requested(&self, id: &str) -> bool {
        self.stop_file(id).is_file()
    }

    fn waiting_file(&self, id: &str) -> PathBuf {
        self.dir.join(WAITING_DIR).join(format!("{id}.json"))
    }

    fn waiting_files(&self) -> Vec<PathBuf> {
        fs::read_dir(self.dir.join(WAITING_DIR))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect()
    }

    fn stop_file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("stop-{id}"))
    }
}

/// The emulator held by a run: the entry is refreshed in the background, a stop request cancels `stop`, and the
/// emulator is released when the value is dropped.
pub struct Holding {
    queue: Queue,
    entry: QueueEntry,
    done: Option<Sender<Option<String>>>,
    beater: Option<JoinHandle<()>>,
}

impl Holding {
    /// Starts refreshing `entry`, which holds the emulator, and watching for a stop request.
    #[must_use]
    pub fn start(queue: Queue, entry: QueueEntry, stop: CancelToken) -> Self {
        let (done, wake) = mpsc::channel::<Option<String>>();
        let beater = {
            let queue = queue.clone();
            let mut entry = entry.clone();
            thread::spawn(move || {
                loop {
                    if queue.stop_requested(&entry.id) {
                        stop.cancel();
                    }
                    let _ = queue.beat_holder(&mut entry, unix_now());
                    match wake.recv_timeout(BEAT) {
                        Ok(Some(dir)) => entry.run_dir = Some(dir),
                        Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                }
            })
        };
        Self {
            queue,
            entry,
            done: Some(done),
            beater: Some(beater),
        }
    }

    /// Records the run's folder in the entry, so the window can follow the run.
    pub fn set_run_dir(&self, dir: &str) {
        if let Some(done) = &self.done {
            let _ = done.send(Some(dir.to_owned()));
        }
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            let _ = done.send(None);
        }
        if let Some(beater) = self.beater.take() {
            let _ = beater.join();
        }
        self.queue.leave(&self.entry);
    }
}

/// Returns the time now in Unix seconds.
#[must_use]
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn alive(entry: &QueueEntry, now: u64) -> bool {
    now.saturating_sub(entry.beat) <= STALE_SECONDS
}

fn read_entry(path: &Path) -> Option<QueueEntry> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn broken(path: &Path, now: u64) -> bool {
    let written = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok());
    written.is_some_and(|written| now.saturating_sub(written.as_secs()) > STALE_SECONDS)
        && read_entry(path).is_none()
}

fn write_entry(path: &Path, entry: &QueueEntry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let temp = write_temp(path, entry).map_err(|error| format!("{}: {error}", path.display()))?;
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("{}: {error}", path.display())
    })
}

fn create_entry(path: &Path, entry: &QueueEntry) -> io::Result<()> {
    let temp = write_temp(path, entry)?;
    let created = fs::hard_link(&temp, path);
    let _ = fs::remove_file(&temp);
    created
}

fn write_temp(path: &Path, entry: &QueueEntry) -> io::Result<PathBuf> {
    let text = serde_json::to_string_pretty(entry).map_err(io::Error::other)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = File::create(&temp).and_then(|mut file| {
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
    });
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(temp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(name: &str) -> Queue {
        let dir =
            std::env::temp_dir().join(format!("carafe-lab-queue-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Queue::new(dir)
    }

    #[test]
    fn the_first_run_takes_a_free_emulator() {
        let queue = queue("free");
        let mut first = queue.join("window", "nfsu", 1, 100).unwrap();
        assert_eq!(queue.try_take(&mut first, 100).unwrap(), Turn::Taken);
        let state = queue.state(100);
        assert_eq!(
            state.holder.map(|holder| holder.who),
            Some("window".to_owned())
        );
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn a_second_run_waits_behind_the_holder() {
        let queue = queue("second");
        let mut first = queue.join("window", "nfsu", 1, 100).unwrap();
        let mut second = queue.join("Claude", "quick", 2, 101).unwrap();
        assert_eq!(queue.try_take(&mut first, 101).unwrap(), Turn::Taken);
        let turn = queue.try_take(&mut second, 102).unwrap();
        assert_eq!(
            turn,
            Turn::Wait {
                holder: "window".to_owned(),
                ahead: 1
            }
        );
        queue.leave(&first);
        assert_eq!(queue.try_take(&mut second, 103).unwrap(), Turn::Taken);
    }

    #[test]
    fn a_later_run_does_not_jump_the_line() {
        let queue = queue("line");
        let mut first = queue.join("window", "nfsu", 1, 100).unwrap();
        let mut second = queue.join("Claude", "quick", 2, 101).unwrap();
        assert_eq!(
            queue.try_take(&mut second, 101).unwrap(),
            Turn::Wait {
                holder: String::new(),
                ahead: 1
            }
        );
        assert_eq!(queue.try_take(&mut first, 101).unwrap(), Turn::Taken);
    }

    #[test]
    fn a_dead_holder_frees_the_emulator() {
        let queue = queue("dead");
        let mut first = queue.join("window", "nfsu", 1, 100).unwrap();
        let mut second = queue.join("Claude", "quick", 2, 100).unwrap();
        assert_eq!(queue.try_take(&mut first, 100).unwrap(), Turn::Taken);
        let later = 100 + STALE_SECONDS + 1;
        queue.beat_waiting(&mut second, later).unwrap();
        assert_eq!(queue.try_take(&mut second, later).unwrap(), Turn::Taken);
    }

    #[test]
    fn a_broken_holder_file_frees_the_emulator() {
        let queue = queue("broken-holder");
        fs::create_dir_all(&queue.dir).unwrap();
        fs::write(queue.dir.join(HOLDER_FILE), [0_u8; 230]).unwrap();
        let later = unix_now() + STALE_SECONDS + 1;
        let mut first = queue.join("window", "nfsu", 1, later).unwrap();
        assert_eq!(queue.try_take(&mut first, later).unwrap(), Turn::Taken);
        assert_eq!(queue.state(later).holder, Some(first));
    }

    #[test]
    fn a_fresh_unreadable_holder_file_is_left_alone() {
        let queue = queue("fresh-holder");
        fs::create_dir_all(&queue.dir).unwrap();
        fs::write(queue.dir.join(HOLDER_FILE), b"").unwrap();
        let now = unix_now();
        let mut first = queue.join("window", "nfsu", 1, now).unwrap();
        assert_eq!(
            queue.try_take(&mut first, now).unwrap(),
            Turn::Wait {
                holder: String::new(),
                ahead: 1
            }
        );
    }

    #[test]
    fn a_broken_waiting_file_is_removed() {
        let queue = queue("broken-waiting");
        let broken_file = queue.waiting_file("1-1");
        fs::create_dir_all(broken_file.parent().unwrap()).unwrap();
        fs::write(&broken_file, [0_u8; 153]).unwrap();
        let later = unix_now() + STALE_SECONDS + 1;
        let mut first = queue.join("window", "nfsu", 2, later).unwrap();
        assert_eq!(queue.try_take(&mut first, later).unwrap(), Turn::Taken);
        assert!(!broken_file.exists());
    }

    #[test]
    fn writing_entries_leaves_only_whole_files() {
        let queue = queue("whole");
        let mut first = queue.join("window", "nfsu", 1, 100).unwrap();
        let mut second = queue.join("Claude", "quick", 2, 100).unwrap();
        assert_eq!(queue.try_take(&mut first, 100).unwrap(), Turn::Taken);
        queue.beat_holder(&mut first, 101).unwrap();
        queue.beat_waiting(&mut second, 101).unwrap();
        let mut names: Vec<String> = [queue.dir.clone(), queue.dir.join(WAITING_DIR)]
            .iter()
            .flat_map(|dir| fs::read_dir(dir).unwrap())
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != WAITING_DIR)
            .collect();
        names.sort();
        assert_eq!(names, vec!["100-2.json".to_owned(), HOLDER_FILE.to_owned()]);
        assert_eq!(queue.state(101).holder.map(|holder| holder.beat), Some(101));
    }

    #[test]
    fn a_stop_request_is_seen_and_cleared_on_leaving() {
        let queue = queue("stop");
        let first = queue.join("window", "nfsu", 1, 100).unwrap();
        queue.request_stop(&first.id).unwrap();
        assert!(queue.stop_requested(&first.id));
        queue.leave(&first);
        assert!(!queue.stop_requested(&first.id));
    }
}
