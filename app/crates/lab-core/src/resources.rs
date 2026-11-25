//! The emulator container's memory and processor time, read once a second.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One reading of the container.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
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
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The cgroup's total charged memory in bytes, including file pages."]
    pub container_memory: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "File-backed cache in bytes, excluding shared memory."]
    pub file_cache: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The cgroup's hard memory limit in bytes; absent when unlimited or unavailable."]
    pub memory_limit: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The cgroup's cumulative OOM-kill count."]
    pub oom_kills: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "Guest physical bytes reserved against the application process's resource limit."]
    pub guest_memory: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The guest application process's physical memory limit in bytes."]
    pub guest_limit: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The emulated console's total physical memory in bytes."]
    pub guest_total: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The host graphics driver's reported available video memory in bytes."]
    pub gpu_available: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The host graphics driver's reported total video memory in bytes."]
    pub gpu_capacity: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "The renderer's configured cache pressure target in bytes."]
    pub gpu_cache_budget: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "Bytes allocated in the host upload staging pool."]
    pub upload_reserved: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "Bytes allocated in the host download staging pool."]
    pub download_reserved: Option<u64>,
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
    #[serde(default)]
    #[doc = "Peak total cgroup charge in MiB, including file pages."]
    pub peak_container_memory_mb: Option<u32>,
    #[serde(default)]
    #[doc = "Peak file cache excluding shared memory, in MiB."]
    pub peak_file_cache_mb: Option<u32>,
    #[serde(default)]
    #[doc = "The sampled cgroup hard memory limit in MiB."]
    pub memory_limit_mb: Option<u32>,
    #[serde(default)]
    #[doc = "Peak guest application physical reservations in MiB."]
    pub peak_guest_memory_mb: Option<u32>,
    #[serde(default)]
    #[doc = "The guest application's physical memory limit in MiB."]
    pub guest_limit_mb: Option<u32>,
    #[serde(default)]
    #[doc = "The emulated console's physical memory in MiB."]
    pub guest_total_mb: Option<u32>,
    #[serde(default)]
    #[doc = "Minimum sampled available host video memory in MiB."]
    pub min_gpu_available_mb: Option<u32>,
    #[serde(default)]
    #[doc = "Maximum reported host video memory capacity in MiB."]
    pub gpu_capacity_mb: Option<u32>,
    #[serde(default)]
    #[doc = "The renderer's cache pressure target in MiB."]
    pub gpu_cache_budget_mb: Option<u32>,
    #[serde(default)]
    #[doc = "Peak bytes allocated in the upload staging pool, in MiB."]
    pub peak_upload_reserved_mb: Option<u32>,
    #[serde(default)]
    #[doc = "Peak bytes allocated in the download staging pool, in MiB."]
    pub peak_download_reserved_mb: Option<u32>,
    #[serde(default)]
    #[ts(type = "number | null")]
    #[doc = "OOM kills observed between the first and last cgroup samples."]
    pub oom_kills: Option<u64>,
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
                container_memory: words.next().and_then(|word| word.parse().ok()),
                file_cache: words.next().and_then(|word| word.parse().ok()),
                memory_limit: words.next().and_then(|word| word.parse().ok()),
                oom_kills: words.next().and_then(|word| word.parse().ok()),
                ..ResourceSample::default()
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
        peak_container_memory_mb: peak_mib(samples, |sample| sample.container_memory),
        peak_file_cache_mb: peak_mib(samples, |sample| sample.file_cache),
        memory_limit_mb: peak_mib(samples, |sample| sample.memory_limit),
        peak_guest_memory_mb: peak_mib(samples, |sample| sample.guest_memory),
        guest_limit_mb: peak_mib(samples, |sample| sample.guest_limit),
        guest_total_mb: peak_mib(samples, |sample| sample.guest_total),
        min_gpu_available_mb: samples
            .iter()
            .filter_map(|sample| sample.gpu_available)
            .min()
            .map(mib),
        gpu_capacity_mb: peak_mib(samples, |sample| sample.gpu_capacity),
        gpu_cache_budget_mb: peak_mib(samples, |sample| sample.gpu_cache_budget),
        peak_upload_reserved_mb: peak_mib(samples, |sample| sample.upload_reserved),
        peak_download_reserved_mb: peak_mib(samples, |sample| sample.download_reserved),
        oom_kills: match (first.oom_kills, last.oom_kills) {
            (Some(first), Some(last)) => Some(last.saturating_sub(first)),
            _ => None,
        },
    })
}

fn mib(bytes: u64) -> u32 {
    u32::try_from(bytes / (1024 * 1024)).unwrap_or(u32::MAX)
}

fn peak_mib(
    samples: &[ResourceSample],
    value: impl Fn(&ResourceSample) -> Option<u64>,
) -> Option<u32> {
    samples.iter().filter_map(value).max().map(mib)
}

#[doc = "Attaches the latest preceding renderer and guest readings, expiring readings after five seconds."]
pub fn attach_emulator_memory(samples: &mut [ResourceSample], log: &str) {
    let mut readings = Vec::new();
    for line in log.lines() {
        let kind = if line.contains("[LAB_GPU_MEMORY]") {
            0
        } else if line.contains("[LAB_GUEST_MEMORY]") {
            1
        } else if line.contains("[LAB_STAGING_MEMORY]") {
            2
        } else {
            continue;
        };
        let fields: std::collections::BTreeMap<_, _> = line
            .split_whitespace()
            .filter_map(|word| word.split_once('='))
            .collect();
        let Some(at) = fields
            .get("unix_ms")
            .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        readings.push((at, kind, fields));
    }
    readings.sort_by_key(|reading| reading.0);
    let mut next = 0;
    let mut latest = ResourceSample::default();
    let mut last_at = [None; 4];
    for sample in samples {
        while let Some((at, kind, fields)) =
            readings.get(next).filter(|reading| reading.0 <= sample.at)
        {
            let number = |key| fields.get(key).and_then(|value| value.parse::<u64>().ok());
            match kind {
                0 => {
                    latest.gpu_available = number("available");
                    latest.gpu_capacity = number("capacity");
                    latest.gpu_cache_budget = number("cache_budget");
                    last_at[0] = Some(*at);
                }
                1 => {
                    latest.guest_memory = number("committed");
                    latest.guest_limit = number("limit");
                    latest.guest_total = number("physical");
                    last_at[1] = Some(*at);
                }
                _ if fields.get("kind") == Some(&"upload") => {
                    latest.upload_reserved = number("reserved");
                    last_at[2] = Some(*at);
                }
                _ if fields.get("kind") == Some(&"download") => {
                    latest.download_reserved = number("reserved");
                    last_at[3] = Some(*at);
                }
                _ => {}
            }
            next += 1;
        }
        if last_at[0].is_some_and(|at| sample.at.saturating_sub(at) <= 5000) {
            sample.gpu_available = latest.gpu_available;
            sample.gpu_capacity = latest.gpu_capacity;
            sample.gpu_cache_budget = latest.gpu_cache_budget;
        }
        if last_at[1].is_some_and(|at| sample.at.saturating_sub(at) <= 5000) {
            sample.guest_memory = latest.guest_memory;
            sample.guest_limit = latest.guest_limit;
            sample.guest_total = latest.guest_total;
        }
        if last_at[2].is_some_and(|at| sample.at.saturating_sub(at) <= 5000) {
            sample.upload_reserved = latest.upload_reserved;
        }
        if last_at[3].is_some_and(|at| sample.at.saturating_sub(at) <= 5000) {
            sample.download_reserved = latest.download_reserved;
        }
    }
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

    #[test]
    fn old_readings_and_reports_keep_unknown_layers_unknown() {
        let samples = parse("1000 1048576 0\n2000 2097152 1000000");
        let summary = summarize(&samples).unwrap();
        assert_eq!(summary.peak_memory_mb, 2);
        assert_eq!(summary.peak_container_memory_mb, None);
        assert_eq!(summary.peak_guest_memory_mb, None);
        assert_eq!(summary.oom_kills, None);
        let old: ResourceSummary =
            serde_json::from_str(r#"{"peakMemoryMb":2,"cpuPercent":100.0}"#).unwrap();
        assert_eq!(old, summary);
        let old: ResourceSample =
            serde_json::from_str(r#"{"at":1000,"memory":1048576,"cpuUsec":0}"#).unwrap();
        assert_eq!(old, samples[0]);
    }

    #[test]
    fn layers_are_independent_and_never_added_together() {
        let mut samples = parse(
            "1000 1048576 0 4194304 2097152 6442450944 7\n2000 2097152 1000000 6291456 3145728 6442450944 8",
        );
        attach_emulator_memory(
            &mut samples,
            "[LAB_GPU_MEMORY] unix_ms=1900 capacity=16777216 available=8388608 cache_budget=4194304\n[LAB_GUEST_MEMORY] unix_ms=900 committed=3145728 limit=3444572160 physical=4294967296\n[LAB_GPU_MEMORY] unix_ms=900 capacity=16777216 available=12582912 cache_budget=4194304\n[LAB_STAGING_MEMORY] unix_ms=900 kind=upload reserved=1048576 deferred=0 fenced=0\n[LAB_STAGING_MEMORY] unix_ms=900 kind=download reserved=0 deferred=0 fenced=0",
        );
        let summary = summarize(&samples).unwrap();
        assert_eq!(summary.peak_memory_mb, 2);
        assert_eq!(summary.peak_container_memory_mb, Some(6));
        assert_eq!(summary.peak_file_cache_mb, Some(3));
        assert_eq!(summary.memory_limit_mb, Some(6144));
        assert_eq!(summary.peak_guest_memory_mb, Some(3));
        assert_eq!(summary.guest_total_mb, Some(4096));
        assert_eq!(summary.min_gpu_available_mb, Some(8));
        assert_eq!(summary.gpu_capacity_mb, Some(16));
        assert_eq!(summary.gpu_cache_budget_mb, Some(4));
        assert_eq!(summary.peak_upload_reserved_mb, Some(1));
        assert_eq!(summary.peak_download_reserved_mb, Some(0));
        assert_eq!(summary.oom_kills, Some(1));
    }

    #[test]
    fn readings_cannot_predict_the_future_or_hide_missing_telemetry() {
        let mut samples = parse("1000 1 0\n2000 1 0\n7001 1 0");
        attach_emulator_memory(
            &mut samples,
            "[LAB_GPU_MEMORY] unix_ms=2000 capacity=99 available=0 cache_budget=42\n[LAB_GPU_MEMORY] unix_ms=broken capacity=999 available=999 cache_budget=999",
        );
        assert_eq!(samples[0].gpu_available, None);
        assert_eq!(samples[1].gpu_available, Some(0));
        assert_eq!(samples[2].gpu_available, None);
        assert_eq!(samples[1].guest_memory, None);
    }
}
