//! The time of every frame the emulator presented, as Eden records it with `record_frame_times`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Name of the file in a launch's log folder with its frame times.
pub const FILE: &str = "frametimes.csv";

/// A frame this long or longer is a stutter, in milliseconds: three frames at 60 per second.
pub const STUTTER_MS: f64 = 50.0;

/// A launch's frame times in short.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FrameSummary {
    /// Frames recorded.
    pub frames: u32,
    /// The mean time of a frame, in milliseconds, without the waits for the display.
    pub mean_ms: f64,
    /// The time 99 frames in 100 stay under, in milliseconds.
    pub p99_ms: f64,
    /// The longest frame, in milliseconds.
    pub worst_ms: f64,
    /// Frames of [`STUTTER_MS`] or longer.
    pub stutters: u32,
}

/// Reads the frame times in milliseconds, one a line; lines that are not numbers are skipped.
#[must_use]
pub fn parse(text: &str) -> Vec<f64> {
    text.lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .filter(|ms| ms.is_finite() && *ms >= 0.0)
        .collect()
}

/// Returns the summary of `times`; `None` when there are none.
#[must_use]
pub fn summarize(times: &[f64]) -> Option<FrameSummary> {
    if times.is_empty() {
        return None;
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p99_index = ((sorted.len() as f64) * 0.99).ceil() as usize;
    Some(FrameSummary {
        frames: u32::try_from(times.len()).unwrap_or(u32::MAX),
        mean_ms: times.iter().sum::<f64>() / times.len() as f64,
        p99_ms: sorted[p99_index.clamp(1, sorted.len()) - 1],
        worst_ms: sorted[sorted.len() - 1],
        stutters: u32::try_from(times.iter().filter(|&&ms| ms >= STUTTER_MS).count())
            .unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_long_frames_show_as_stutters_and_in_the_worst_hundredth() {
        let mut text = String::new();
        for _ in 0..98 {
            text.push_str("10\n");
        }
        text.push_str("60\nnot a frame\n120\n");
        let times = parse(&text);
        assert_eq!(times.len(), 100);
        let summary = summarize(&times).unwrap();
        assert_eq!(summary.frames, 100);
        assert_eq!(summary.stutters, 2);
        assert!((summary.worst_ms - 120.0).abs() < 1e-9);
        assert!((summary.p99_ms - 60.0).abs() < 1e-9);
        assert!((summary.mean_ms - 11.6).abs() < 1e-9);
        assert!(summarize(&[]).is_none());
    }
}
