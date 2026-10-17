//! The emulator container's memory and processor time, read once a second.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One reading of the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResourceSample {
    /// When it was taken, in Unix milliseconds.
    #[ts(type = "number")]
    pub at: u64,
    /// Memory in use without the file cache, in bytes: the processes' own memory and the shared memory, such as the
    /// emulated Switch's.
    #[ts(type = "number")]
    pub memory: u64,
    /// Processor time used since the container started, in microseconds.
    #[ts(type = "number")]
    pub cpu_usec: u64,
}

/// A launch's use of the container.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResourceSummary {
    /// The most memory in use, in mebibytes.
    pub peak_memory_mb: u32,
    /// Processor time over the launch, in percent of one processor: 400 is four processors busy.
    pub cpu_percent: f64,
}

/// Reads the lines `ms memory cpu_usec` the container's sampler writes; other lines are skipped.
#[must_use]
pub fn parse(text: &str) -> Vec<ResourceSample> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some(ResourceSample {
                at: words.next()?.parse().ok()?,
                memory: words.next()?.parse().ok()?,
                cpu_usec: words.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// Returns the readings taken at `from` or later.
#[must_use]
pub fn since(samples: &[ResourceSample], from: u64) -> Vec<ResourceSample> {
    samples
        .iter()
        .copied()
        .filter(|sample| sample.at >= from)
        .collect()
}

/// Returns the peak memory and the mean processor use of `samples`; `None` with fewer than two readings.
#[must_use]
pub fn summarize(samples: &[ResourceSample]) -> Option<ResourceSummary> {
    let (first, last) = (samples.first()?, samples.last()?);
    let wall_ms = last.at.checked_sub(first.at).filter(|&ms| ms > 0)?;
    let cpu_usec = last.cpu_usec.saturating_sub(first.cpu_usec);
    let peak = samples
        .iter()
        .map(|sample| sample.memory)
        .max()
        .unwrap_or(0);
    Some(ResourceSummary {
        peak_memory_mb: u32::try_from(peak / (1024 * 1024)).unwrap_or(u32::MAX),
        cpu_percent: cpu_usec as f64 / (wall_ms as f64 * 1000.0) * 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_shows_its_peak_memory_and_processor_use() {
        let samples =
            parse("1000 1048576 0\nnot a reading\n2000 3145728 1500000\n3000 2097152 3000000\n");
        assert_eq!(samples.len(), 3);
        let summary = summarize(&since(&samples, 1500)).unwrap();
        assert_eq!(summary.peak_memory_mb, 3);
        assert!((summary.cpu_percent - 150.0).abs() < 0.01);
        assert!(summarize(&samples[..1]).is_none());
    }
}
