//! Sets of graphics and system settings for the probe runs: a pairwise set and random combinations.
//!
//! Each setting is reduced to the values that change how the runtime behaves: frame limits are none, 30, 60
//! and 120, FSR sharpness is 0, 40 and 100.

use carafe_core::settings::{AutorunSettings, DxvkHud, LsfgFlow, Upscaling, WindowOutput};

const WINDOWS: [WindowOutput; 3] = [
    WindowOutput::Auto,
    WindowOutput::Framebuffer,
    WindowOutput::Compositor,
];
const UPSCALING: [Upscaling; 3] = [Upscaling::Fsr, Upscaling::Off, Upscaling::Integer];
const HUD: [DxvkHud; 4] = [DxvkHud::Off, DxvkHud::Fps, DxvkHud::Compact, DxvkHud::Full];
const FRAME_LIMIT: [u16; 4] = [0, 30, 60, 120];
const SHARPNESS: [u8; 3] = [40, 0, 100];
const LSFG_FLOW: [LsfgFlow; 3] = [LsfgFlow::Quarter, LsfgFlow::Eighth, LsfgFlow::Half];
const FLAG: [bool; 2] = [true, false];

const SIZES: [usize; 9] = [
    WINDOWS.len(),
    UPSCALING.len(),
    SHARPNESS.len(),
    HUD.len(),
    FRAME_LIMIT.len(),
    FLAG.len(),
    FLAG.len(),
    FLAG.len(),
    LSFG_FLOW.len(),
];

type Row = [usize; SIZES.len()];

/// One combination of settings and what it was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combination {
    /// The settings; input settings stay at their defaults.
    pub settings: AutorunSettings,
    /// A short label for the report, such as `compositor fsr40 hud-off unlimited vsync no-lsfg`.
    pub label: String,
}

/// Returns the number of combinations of all reduced values.
#[must_use]
pub fn full_count() -> u64 {
    SIZES.iter().map(|&size| size as u64).product()
}

/// Returns a set in which every pair of values of any two settings occurs at least once.
///
/// The set is the same on every call. Every combination in it passes [`AutorunSettings::validate`].
#[must_use]
pub fn pairwise() -> Vec<Combination> {
    let mut uncovered = required_pairs();
    let mut rows = Vec::new();
    while let Some(&(first, a, second, b)) = uncovered.first() {
        let mut row: [Option<usize>; SIZES.len()] = [None; SIZES.len()];
        row[first] = Some(a);
        row[second] = Some(b);
        for parameter in 0..SIZES.len() {
            if row[parameter].is_some() {
                continue;
            }
            let best = (0..SIZES[parameter])
                .max_by_key(|&value| {
                    let gain = row
                        .iter()
                        .enumerate()
                        .filter_map(|(other, chosen)| chosen.map(|chosen| (other, chosen)))
                        .filter(|&(other, chosen)| {
                            uncovered.contains(&ordered(parameter, value, other, chosen))
                        })
                        .count();
                    (gain, usize::MAX - value)
                })
                .unwrap_or(0);
            row[parameter] = Some(best);
        }
        let row: Row = row.map(|value| value.unwrap_or(0));
        uncovered.retain(|&(p, a, q, b)| row[p] != a || row[q] != b);
        rows.push(row);
    }
    rows.iter().map(combination).collect()
}

/// Returns `count` combinations drawn from `seed`; the same seed gives the same combinations.
#[must_use]
pub fn random(seed: u64, count: usize) -> Vec<Combination> {
    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    let mut next = move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    (0..count)
        .map(|_| combination(&SIZES.map(&mut next)))
        .collect()
}

fn required_pairs() -> Vec<(usize, usize, usize, usize)> {
    let mut pairs = Vec::new();
    for (p, &size_p) in SIZES.iter().enumerate() {
        for (q, &size_q) in SIZES.iter().enumerate().skip(p + 1) {
            for a in 0..size_p {
                for b in 0..size_q {
                    pairs.push((p, a, q, b));
                }
            }
        }
    }
    pairs
}

fn ordered(p: usize, a: usize, q: usize, b: usize) -> (usize, usize, usize, usize) {
    if p < q { (p, a, q, b) } else { (q, b, p, a) }
}

fn combination(row: &Row) -> Combination {
    let mut settings = AutorunSettings::default();
    settings.system.windows = WINDOWS[row[0]];
    let graphics = &mut settings.graphics;
    graphics.upscaling = UPSCALING[row[1]];
    graphics.sharpness = SHARPNESS[row[2]];
    graphics.hud = HUD[row[3]];
    graphics.frame_limit = FRAME_LIMIT[row[4]];
    graphics.vsync = FLAG[row[5]];
    graphics.lsfg = FLAG[row[6]];
    graphics.lsfg_performance = FLAG[row[7]];
    graphics.lsfg_flow = LSFG_FLOW[row[8]];
    let label = describe(&settings);
    Combination { settings, label }
}

/// Returns the graphics and system settings in short, such as `auto fsr40 hud-off unlimited vsync …`.
#[must_use]
pub fn describe(settings: &AutorunSettings) -> String {
    let graphics = &settings.graphics;
    let windows = match settings.system.windows {
        WindowOutput::Auto => "auto",
        WindowOutput::Framebuffer => "framebuffer",
        WindowOutput::Compositor => "compositor",
    };
    let upscaling = match graphics.upscaling {
        Upscaling::Off => "bilinear".to_owned(),
        Upscaling::Fsr => format!("fsr{}", graphics.sharpness),
        Upscaling::Integer => "integer".to_owned(),
    };
    let hud = match graphics.hud {
        DxvkHud::Off => "hud-off",
        DxvkHud::Fps => "hud-fps",
        DxvkHud::Compact => "hud-compact",
        DxvkHud::Full => "hud-full",
    };
    let limit = match graphics.frame_limit {
        0 => "unlimited".to_owned(),
        fps => format!("{fps}fps"),
    };
    let vsync = if graphics.vsync { "vsync" } else { "no-vsync" };
    let lsfg = if graphics.lsfg {
        let mode = if graphics.lsfg_performance {
            "perf"
        } else {
            "quality"
        };
        let flow = match graphics.lsfg_flow {
            LsfgFlow::Eighth => "1/8",
            LsfgFlow::Quarter => "1/4",
            LsfgFlow::Half => "1/2",
        };
        format!("lsfg-{mode}-{flow}")
    } else {
        "no-lsfg".to_owned()
    };
    [windows, &upscaling, hud, &limit, vsync, &lsfg].join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_of(settings: &AutorunSettings) -> Row {
        let graphics = &settings.graphics;
        let at = |found: Option<usize>| found.expect("value from the table");
        [
            at(WINDOWS.iter().position(|v| *v == settings.system.windows)),
            at(UPSCALING.iter().position(|v| *v == graphics.upscaling)),
            at(SHARPNESS.iter().position(|v| *v == graphics.sharpness)),
            at(HUD.iter().position(|v| *v == graphics.hud)),
            at(FRAME_LIMIT.iter().position(|v| *v == graphics.frame_limit)),
            at(FLAG.iter().position(|v| *v == graphics.vsync)),
            at(FLAG.iter().position(|v| *v == graphics.lsfg)),
            at(FLAG.iter().position(|v| *v == graphics.lsfg_performance)),
            at(LSFG_FLOW.iter().position(|v| *v == graphics.lsfg_flow)),
        ]
    }

    #[test]
    fn pairwise_covers_every_pair() {
        let rows: Vec<Row> = pairwise().iter().map(|c| row_of(&c.settings)).collect();
        for (p, a, q, b) in required_pairs() {
            assert!(
                rows.iter().any(|row| row[p] == a && row[q] == b),
                "pair ({p}={a}, {q}={b}) is not covered"
            );
        }
    }

    #[test]
    fn pairwise_holds_only_valid_settings() {
        for combination in pairwise() {
            assert_eq!(
                combination.settings.validate(),
                Ok(()),
                "{}",
                combination.label
            );
        }
    }

    #[test]
    fn pairwise_stays_small_and_stable() {
        let set = pairwise();
        assert!(set.len() <= 30, "{} combinations", set.len());
        assert!(set.len() >= HUD.len() * FRAME_LIMIT.len());
        assert_eq!(set, pairwise());
    }

    #[test]
    fn random_is_valid_and_repeatable() {
        let first = random(7, 50);
        assert_eq!(first.len(), 50);
        assert_eq!(first, random(7, 50));
        assert_ne!(first, random(8, 50));
        for combination in &first {
            assert_eq!(combination.settings.validate(), Ok(()));
        }
    }

    #[test]
    fn the_first_row_is_the_default_where_it_can_be() {
        let first = &pairwise()[0];
        assert_eq!(first.settings.input, AutorunSettings::default().input);
    }

    #[test]
    fn labels_name_the_settings() {
        let label = &combination(&[0; SIZES.len()]).label;
        assert_eq!(label, "auto fsr40 hud-off unlimited vsync lsfg-perf-1/4");
    }

    #[test]
    fn full_count_multiplies_every_setting() {
        assert_eq!(full_count(), 3 * 3 * 3 * 4 * 4 * 2 * 2 * 2 * 3);
    }
}
