//! The emulator's performance numbers, as Eden writes them to the control channel's `stats` file
//! (`patches/eden/0047`), and their summary over a launch.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One reading of the emulator's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerfSample {
    /// When it was taken, in Unix milliseconds.
    #[ts(type = "number")]
    pub at: u64,
    /// The game's average frames per second since the reading before.
    pub fps: f64,
    /// Emulation speed: 1.0 is the console's own speed.
    pub speed: f64,
    /// Milliseconds the emulator spent on a frame.
    pub frametime: f64,
    /// Shaders being built.
    pub shaders: u32,
}

/// A launch's performance in short.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerfSummary {
    /// Readings the summary is made of: those with the game drawing.
    pub samples: usize,
    /// The median FPS.
    pub median_fps: f64,
    /// The FPS the worst tenth of the readings stay under.
    pub low_fps: f64,
    /// The median emulation speed.
    pub median_speed: f64,
}

/// Returns the readings in the text of a `stats` file; lines that are not readings are left out.
#[must_use]
pub fn parse_stats(text: &str) -> Vec<PerfSample> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some(PerfSample {
                at: parts.next()?.parse().ok()?,
                fps: parts.next()?.parse().ok()?,
                speed: parts.next()?.parse().ok()?,
                frametime: parts.next()?.parse().ok()?,
                shaders: parts.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// Returns the summary of `samples`, counting only those with the game drawing; `None` when there are none.
#[must_use]
pub fn summarize(samples: &[PerfSample]) -> Option<PerfSummary> {
    let drawing: Vec<&PerfSample> = samples.iter().filter(|sample| sample.fps > 0.0).collect();
    if drawing.is_empty() {
        return None;
    }
    let mut fps: Vec<f64> = drawing.iter().map(|sample| sample.fps).collect();
    fps.sort_by(f64::total_cmp);
    let mut speed: Vec<f64> = drawing.iter().map(|sample| sample.speed).collect();
    speed.sort_by(f64::total_cmp);
    Some(PerfSummary {
        samples: drawing.len(),
        median_fps: median(&fps),
        low_fps: fps[fps.len() / 10],
        median_speed: median(&speed),
    })
}

/// Returns the readings from `seconds` after the first reading on: the game's own frames when `seconds` is the
/// time from the launch to its first frame, without the loading screen's.
#[must_use]
pub fn after(samples: &[PerfSample], seconds: u32) -> Vec<PerfSample> {
    let Some(first) = samples.first() else {
        return Vec::new();
    };
    let from = first.at + u64::from(seconds) * 1000;
    samples
        .iter()
        .filter(|sample| sample.at >= from)
        .copied()
        .collect()
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_are_read_and_broken_lines_left_out() {
        let samples =
            parse_stats("1000 29.50 0.985 31.20 0\nnot a reading\n1500 30.00 1.000 30.10 2\n");
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[1].at, 1500);
        assert_eq!(samples[1].shaders, 2);
        assert!((samples[0].fps - 29.5).abs() < f64::EPSILON);
    }

    #[test]
    fn readings_after_the_first_frame_leave_the_loading_screen_out() {
        let samples =
            parse_stats("1000 10 1 16 0\n2000 10 1 16 0\n3000 55 1 16 0\n3500 57 1 16 0\n");
        let game = after(&samples, 2);
        assert_eq!(game.len(), 2);
        assert_eq!(game[0].at, 3000);
        assert!(after(&[], 2).is_empty());
    }

    #[test]
    fn the_summary_leaves_out_the_time_before_the_game_draws() {
        let mut samples: Vec<PerfSample> = (0..20)
            .map(|index| PerfSample {
                at: index,
                fps: f64::from(u32::try_from(index).unwrap() + 20),
                speed: 1.0,
                frametime: 16.0,
                shaders: 0,
            })
            .collect();
        samples.insert(
            0,
            PerfSample {
                at: 0,
                fps: 0.0,
                speed: 0.0,
                frametime: 0.0,
                shaders: 0,
            },
        );
        let summary = summarize(&samples).unwrap();
        assert_eq!(summary.samples, 20);
        assert!((summary.median_fps - 29.5).abs() < f64::EPSILON);
        assert!((summary.low_fps - 22.0).abs() < f64::EPSILON);
        assert_eq!(summarize(&samples[..1]), None);
    }
}
