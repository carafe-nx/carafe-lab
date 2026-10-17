//! One timeline of a launch: what the lab did, what Eden, the Carafe overlay and the runtime logged and how much memory
//! and processor the container used, on one clock, seconds since the lab launched the game.
//!
//! Each source has a clock of its own, so each is moved onto the launch's: Eden's log counts from Eden's start and is
//! anchored at its `Control: booting` line; the overlay and the runtime count from the game's start, which is Eden's
//! `Booting game` line; the container's readings are Unix milliseconds. A line without a time of its own keeps the time
//! of the line before it.

use crate::resources::ResourceSample;

/// The file in a launch's folder with its timeline.
pub const FILE: &str = "timeline.txt";

const RESOURCES_EVERY_MS: u64 = 5000;

/// A line of the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Seconds since the lab launched the game.
    pub at: f64,
    /// Where the line comes from: `lab`, `eden`, `carafe`, `runtime` or `container`.
    pub source: &'static str,
    /// The line.
    pub text: String,
}

/// The logs and readings of one launch.
#[derive(Debug, Clone, Default)]
pub struct Sources<'a> {
    /// What the lab did, as seconds since the launch and a line.
    pub lab: Vec<(f64, String)>,
    /// Eden's log of the launch.
    pub eden: &'a str,
    /// `carafe-overlay.log`.
    pub overlay: &'a str,
    /// `autorun_runtime.log`.
    pub runtime: &'a str,
    /// The container's readings.
    pub resources: &'a [ResourceSample],
    /// When the lab launched the game, in Unix milliseconds; `None` leaves the readings out.
    pub launched_ms: Option<u64>,
}

/// Returns every line of `sources` on the launch's clock, in time order; lines of one source keep their order.
#[must_use]
pub fn merge(sources: &Sources<'_>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = sources
        .lab
        .iter()
        .map(|(at, text)| Entry {
            at: *at,
            source: "lab",
            text: text.clone(),
        })
        .collect();
    let eden = eden_lines(sources.eden);
    let boot = eden
        .iter()
        .find(|(_, line)| line.contains("Control: booting"))
        .or_else(|| eden.first())
        .map_or(0.0, |(at, _)| *at);
    let game_start = eden
        .iter()
        .find(|(_, line)| line.contains("Booting game"))
        .map_or(boot, |(at, _)| *at)
        - boot;
    entries.extend(eden.into_iter().map(|(at, text)| Entry {
        at: at - boot,
        source: "eden",
        text,
    }));
    entries.extend(
        game_lines(sources.overlay, overlay_time)
            .into_iter()
            .map(|(at, text)| Entry {
                at: game_start + at,
                source: "carafe",
                text,
            }),
    );
    entries.extend(
        game_lines(sources.runtime, runtime_time)
            .into_iter()
            .map(|(at, text)| Entry {
                at: game_start + at,
                source: "runtime",
                text,
            }),
    );
    if let Some(launched) = sources.launched_ms {
        entries.extend(resource_lines(sources.resources, launched));
    }
    entries.sort_by(|a, b| a.at.total_cmp(&b.at));
    entries
}

/// Returns `entries` as text, one line each: the time, the source and the line.
#[must_use]
pub fn render(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|entry| format!("{:>9.3} s  {:<9} {}\n", entry.at, entry.source, entry.text))
        .collect()
}

fn eden_lines(log: &str) -> Vec<(f64, String)> {
    let mut at = 0.0;
    log.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let text = match line
                .strip_prefix('[')
                .and_then(|rest| rest.split_once(']'))
                .and_then(|(time, rest)| Some((time.trim().parse::<f64>().ok()?, rest)))
            {
                Some((time, rest)) => {
                    at = time;
                    rest.trim().to_owned()
                }
                None => line.trim_end().to_owned(),
            };
            (at, text)
        })
        .collect()
}

fn game_lines(log: &str, time_of: fn(&str) -> Option<f64>) -> Vec<(f64, String)> {
    let mut at = 0.0;
    log.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            if let Some(time) = time_of(line) {
                at = time;
            }
            (at, line.trim_end().to_owned())
        })
        .collect()
}

fn overlay_time(line: &str) -> Option<f64> {
    let (_, after) = line.split_once(" at ")?;
    let number = after.split_whitespace().next()?;
    let unit = after.split_whitespace().nth(1)?;
    unit.trim_end_matches([',', ':'])
        .eq("s")
        .then(|| number.parse().ok())
        .flatten()
}

fn runtime_time(line: &str) -> Option<f64> {
    let rest = line.strip_prefix("[PROGRESS] ")?;
    rest.split_whitespace()
        .next()?
        .strip_suffix('s')?
        .parse()
        .ok()
}

fn resource_lines(samples: &[ResourceSample], launched: u64) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut last: Option<&ResourceSample> = None;
    for sample in samples.iter().filter(|sample| sample.at >= launched) {
        if let Some(before) = last
            && sample.at < before.at + RESOURCES_EVERY_MS
        {
            continue;
        }
        let cpu = last.map_or(String::new(), |before| {
            let wall_us = sample.at.saturating_sub(before.at) * 1000;
            let used = sample.cpu_usec.saturating_sub(before.cpu_usec);
            let percent = (used * 100).checked_div(wall_us).unwrap_or(0);
            format!(", processor {percent}%")
        });
        entries.push(Entry {
            at: (sample.at - launched) as f64 / 1000.0,
            source: "container",
            text: format!("memory {} MB{cpu}", sample.memory / (1024 * 1024)),
        });
        last = Some(sample);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDEN: &str = "\
[  40.000000] Frontend <Info> ControlReply: Control: booting /nsp/0500.nsp
[  41.000000] Core <Info> Load: Loading OpenTTD
[  42.000000] Frontend <Info> BootGame: Booting game: 0500 | OpenTTD
[  70.000000] Core.ARM <Critical> RunThread: Unhandled read
R00=0000003FFF8DCD00
";

    const OVERLAY: &str = "\
[carafe-overlay] started 2026-10-17 17:43:20
[carafe-overlay] [CARAFE] loading screen drawn at 1 s, 1280x720
[carafe-overlay] commit after 10 s of writes at 11.240 s: rc=0x00000000
";

    const RUNTIME: &str = "\
[BUILD] nx-amd64-fex-2640
[PROGRESS] 5s reads=183
[THREADS] 8 threads use 0.56 cores
";

    #[test]
    fn every_source_lands_on_the_launch_clock() {
        let resources = [
            ResourceSample {
                at: 1_000_000,
                memory: 100 * 1024 * 1024,
                cpu_usec: 0,
            },
            ResourceSample {
                at: 1_005_000,
                memory: 2048 * 1024 * 1024,
                cpu_usec: 10_000_000,
            },
        ];
        let sources = Sources {
            lab: vec![(0.0, "Launch: launch 1 started".to_owned())],
            eden: EDEN,
            overlay: OVERLAY,
            runtime: RUNTIME,
            resources: &resources,
            launched_ms: Some(1_000_000),
        };
        let entries = merge(&sources);
        let find = |text: &str| {
            entries
                .iter()
                .find(|entry| entry.text.contains(text))
                .unwrap()
        };
        assert_eq!(find("Control: booting").at, 0.0);
        assert_eq!(find("Booting game").at, 2.0);
        assert_eq!(find("loading screen drawn").at, 3.0);
        assert!((find("commit after").at - 13.24).abs() < 1e-9);
        assert_eq!(find("[PROGRESS] 5s").at, 7.0);
        assert_eq!(find("[THREADS]").at, 7.0);
        assert_eq!(find("[BUILD]").at, 2.0);
        assert_eq!(find("R00=").at, 30.0);
        assert_eq!(find("memory 2048 MB, processor 200%").at, 5.0);
        assert!(entries.windows(2).all(|pair| pair[0].at <= pair[1].at));
        assert!(render(&entries).contains("    0.000 s  lab       Launch: launch 1 started\n"));
    }
}
