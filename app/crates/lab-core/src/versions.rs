//! Performance by Carafe version: what a game measured at each commit over its repeats, and which changes between
//! versions are larger than the spread of the repeats.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The percentage a game may get worse by before [`slower_than`] calls it a regression, unless `lab.json` sets its own.
pub const DEFAULT_THRESHOLD: u32 = 20;

/// What one launch of a game measured.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Seconds to the first frame.
    pub first_frame: Option<f64>,
    /// The median FPS while the game drew.
    pub fps: Option<f64>,
    /// The time 99 frames in 100 stay under, in milliseconds.
    pub p99_ms: Option<f64>,
    /// The container's peak memory, in mebibytes.
    pub peak_mb: Option<f64>,
}

/// The middle value of repeats and how far apart they are.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Spread {
    /// The median of the repeats.
    pub median: f64,
    /// The largest repeat minus the smallest.
    pub spread: f64,
    /// How many repeats there were.
    pub count: u32,
}

/// A game's performance at one Carafe commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VersionPerf {
    /// The game's name.
    pub game: String,
    /// The Carafe commit.
    pub commit: String,
    /// Seconds to the first frame.
    pub first_frame: Option<Spread>,
    /// The median FPS.
    pub fps: Option<Spread>,
    /// The worst hundredth of frames, in milliseconds.
    pub p99_ms: Option<Spread>,
    /// The peak memory, in mebibytes.
    pub peak_mb: Option<Spread>,
}

/// Whether a measure changed between two versions by more than the spread of their repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Trend {
    /// Better by more than the spread.
    Better,
    /// Worse by more than the spread.
    Worse,
    /// The difference is within the spread.
    Same,
    /// Either version has fewer than two repeats, so the spread is unknown.
    TooFew,
}

/// Returns the median and spread of `values`; `None` when there are none.
#[must_use]
pub fn spread(values: &[f64]) -> Option<Spread> {
    let mut sorted: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    let median = if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    };
    Some(Spread {
        median,
        spread: sorted[sorted.len() - 1] - sorted[0],
        count: u32::try_from(sorted.len()).unwrap_or(u32::MAX),
    })
}

/// Returns the performance of every game at every commit, games and commits in the order `samples` first names them.
#[must_use]
pub fn by_version<'a>(
    samples: impl IntoIterator<Item = (&'a str, &'a str, Sample)>,
) -> Vec<VersionPerf> {
    let mut groups: Vec<(String, String, Vec<Sample>)> = Vec::new();
    for (game, commit, sample) in samples {
        match groups
            .iter_mut()
            .find(|(known_game, known_commit, _)| known_game == game && known_commit == commit)
        {
            Some((_, _, list)) => list.push(sample),
            None => groups.push((game.to_owned(), commit.to_owned(), vec![sample])),
        }
    }
    groups
        .into_iter()
        .map(|(game, commit, list)| {
            let pick = |take: fn(&Sample) -> Option<f64>| {
                spread(&list.iter().filter_map(take).collect::<Vec<_>>())
            };
            VersionPerf {
                game,
                commit,
                first_frame: pick(|sample| sample.first_frame),
                fps: pick(|sample| sample.fps),
                p99_ms: pick(|sample| sample.p99_ms),
                peak_mb: pick(|sample| sample.peak_mb),
            }
        })
        .collect()
}

/// Returns how a measure went from `before` to `after`, flagging only a change larger than both spreads.
///
/// # Arguments
///
/// * `higher_is_better` — `true` for FPS, `false` for times and memory.
#[must_use]
pub fn trend(before: &Spread, after: &Spread, higher_is_better: bool) -> Trend {
    if before.count < 2 || after.count < 2 {
        return Trend::TooFew;
    }
    let change = after.median - before.median;
    if change.abs() <= before.spread.max(after.spread) {
        return Trend::Same;
    }
    if (change > 0.0) == higher_is_better {
        Trend::Better
    } else {
        Trend::Worse
    }
}

/// A game's performance at one commit with how each measure went since the game's commit before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VersionStep {
    /// The game's commit before, the one this step is compared with; `None` at its first commit.
    pub previous: Option<String>,
    /// The performance at this commit.
    pub perf: VersionPerf,
    /// How the time to the first frame went; `None` at the game's first commit or when either side lacks it.
    pub first_frame: Option<Trend>,
    /// How the median FPS went.
    pub fps: Option<Trend>,
    /// How the worst hundredth of frames went.
    pub p99_ms: Option<Trend>,
    /// How the peak memory went.
    pub peak_mb: Option<Trend>,
}

/// Returns `perf`, oldest commit first, with each game's measures compared with its commit before.
#[must_use]
pub fn steps(perf: &[VersionPerf]) -> Vec<VersionStep> {
    let compare = |before: Option<Spread>, after: Option<Spread>, higher_is_better: bool| {
        Some(trend(&before?, &after?, higher_is_better))
    };
    perf.iter()
        .enumerate()
        .map(|(index, now)| {
            let before = perf[..index]
                .iter()
                .rev()
                .find(|earlier| earlier.game == now.game);
            VersionStep {
                previous: before.map(|before| before.commit.clone()),
                perf: now.clone(),
                first_frame: before
                    .and_then(|before| compare(before.first_frame, now.first_frame, false)),
                fps: before.and_then(|before| compare(before.fps, now.fps, true)),
                p99_ms: before.and_then(|before| compare(before.p99_ms, now.p99_ms, false)),
                peak_mb: before.and_then(|before| compare(before.peak_mb, now.peak_mb, false)),
            }
        })
        .collect()
}

/// Returns how many percent worse `now` is than `baseline`, when it is worse by more than `threshold` percent.
///
/// # Arguments
///
/// * `higher_is_better` — `true` for FPS, `false` for times.
#[must_use]
pub fn slower_than(baseline: f64, now: f64, higher_is_better: bool, threshold: u32) -> Option<f64> {
    if baseline <= 0.0 {
        return None;
    }
    let worse = if higher_is_better {
        (baseline - now) / baseline * 100.0
    } else {
        (now - baseline) / baseline * 100.0
    };
    (worse > f64::from(threshold)).then_some(worse)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(first_frame: f64, fps: f64) -> Sample {
        Sample {
            first_frame: Some(first_frame),
            fps: Some(fps),
            ..Sample::default()
        }
    }

    #[test]
    fn repeats_of_a_version_give_a_median_and_a_spread() {
        let perf = by_version([
            ("OpenTTD", "aaa", sample(30.0, 40.0)),
            ("OpenTTD", "aaa", sample(34.0, 44.0)),
            ("OpenTTD", "bbb", sample(50.0, 20.0)),
            ("NFSU", "aaa", sample(90.0, 25.0)),
        ]);
        assert_eq!(perf.len(), 3);
        assert_eq!(perf[0].first_frame.unwrap().median, 32.0);
        assert_eq!(perf[0].first_frame.unwrap().spread, 4.0);
        assert_eq!(perf[0].fps.unwrap().count, 2);
        assert_eq!(perf[1].commit, "bbb");
        assert!(perf[0].p99_ms.is_none());
    }

    #[test]
    fn only_a_change_beyond_the_spread_counts() {
        let before = Spread {
            median: 30.0,
            spread: 4.0,
            count: 3,
        };
        let close = Spread {
            median: 33.0,
            spread: 2.0,
            count: 3,
        };
        let far = Spread {
            median: 40.0,
            spread: 2.0,
            count: 3,
        };
        let single = Spread {
            median: 40.0,
            spread: 0.0,
            count: 1,
        };
        assert_eq!(trend(&before, &close, false), Trend::Same);
        assert_eq!(trend(&before, &far, false), Trend::Worse);
        assert_eq!(trend(&before, &far, true), Trend::Better);
        assert_eq!(trend(&before, &single, false), Trend::TooFew);
    }

    #[test]
    fn each_commit_is_compared_with_the_games_commit_before() {
        let perf = by_version([
            ("OpenTTD", "aaa", sample(30.0, 40.0)),
            ("OpenTTD", "aaa", sample(31.0, 41.0)),
            ("NFSU", "aaa", sample(90.0, 25.0)),
            ("OpenTTD", "bbb", sample(50.0, 20.0)),
            ("OpenTTD", "bbb", sample(51.0, 21.0)),
        ]);
        let steps = steps(&perf);
        assert_eq!(steps[0].first_frame, None);
        assert_eq!(steps[1].fps, None);
        assert_eq!(steps[2].first_frame, Some(Trend::Worse));
        assert_eq!(steps[2].fps, Some(Trend::Worse));
        assert_eq!(steps[2].p99_ms, None);
    }

    #[test]
    fn a_game_fails_only_beyond_its_threshold() {
        assert_eq!(slower_than(30.0, 35.0, false, 20), None);
        assert!((slower_than(30.0, 45.0, false, 20).unwrap() - 50.0).abs() < 1e-9);
        assert!((slower_than(40.0, 20.0, true, 20).unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(slower_than(40.0, 50.0, true, 20), None);
        assert_eq!(slower_than(0.0, 50.0, false, 20), None);
    }
}
