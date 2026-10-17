//! Checking the input probe's log against the bindings of a build: what each Switch button turned into.

use std::collections::BTreeSet;

use carafe_core::settings::{InputMode, InputSettings, MOUSE_CODE_FIRST, SwitchButton};
use serde::Serialize;
use ts_rs::TS;

const GAP_SECONDS: f64 = 0.6;
const AXIS_THRESHOLD: i32 = 8000;
const TRIGGER_THRESHOLD: u32 = 64;

/// One thing the game can receive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind", content = "name")]
#[ts(export)]
pub enum Signal {
    /// An Xbox button, such as `A` or `Start`.
    Pad(String),
    /// A trigger pulled: `LT` or `RT`.
    Trigger(String),
    /// A stick pushed: `LX+`, `LY-`…
    Stick(String),
    /// A key, by its Windows virtual key code.
    Key(u16),
    /// A mouse button: `left`, `right`, `middle`, `x1`, `x2`.
    Mouse(String),
    /// A wheel turned: `up`, `down`, `left`, `right`.
    Wheel(String),
    /// The cursor moved: `up`, `down`, `left`, `right`.
    Move(String),
}

/// What one button press was expected to give and what the probe saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PressCheck {
    /// The Switch button.
    pub button: SwitchButton,
    /// What the bindings say the game gets.
    pub expected: BTreeSet<Signal>,
    /// What the probe saw.
    pub observed: BTreeSet<Signal>,
}

impl PressCheck {
    /// Returns whether the probe saw exactly what was expected.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.expected == self.observed
    }
}

/// Returns what pressing `button` must give a game under `input`.
#[must_use]
pub fn expected(input: &InputSettings, button: SwitchButton) -> BTreeSet<Signal> {
    let (layout, controller) = match input.mode {
        InputMode::Controller => (&input.pad, true),
        InputMode::KeyboardMouse => (&input.keys, false),
    };
    let codes = layout
        .iter()
        .find(|binding| binding.button == button)
        .map(|binding| binding.codes.as_slice())
        .unwrap_or_default();
    codes
        .iter()
        .filter_map(|&code| {
            if controller {
                pad_target(code)
            } else {
                key_target(code)
            }
        })
        .collect()
}

fn pad_target(target: u16) -> Option<Signal> {
    let pad = |name: &str| Some(Signal::Pad(name.to_owned()));
    let stick = |name: &str| Some(Signal::Stick(name.to_owned()));
    match target {
        1 => pad("B"),
        2 => pad("A"),
        3 => pad("Y"),
        4 => pad("X"),
        5 => pad("LB"),
        6 => pad("RB"),
        7 => Some(Signal::Trigger("LT".to_owned())),
        8 => Some(Signal::Trigger("RT".to_owned())),
        9 => pad("Start"),
        10 => pad("Back"),
        11 => pad("LS"),
        12 => pad("RS"),
        13 => pad("Up"),
        14 => pad("Down"),
        15 => pad("Left"),
        16 => pad("Right"),
        17 => stick("LY+"),
        18 => stick("LY-"),
        19 => stick("LX-"),
        20 => stick("LX+"),
        21 => stick("RY+"),
        22 => stick("RY-"),
        23 => stick("RX-"),
        24 => stick("RX+"),
        _ => None,
    }
}

fn key_target(code: u16) -> Option<Signal> {
    if code < MOUSE_CODE_FIRST {
        return Some(Signal::Key(code));
    }
    let named = |kind: fn(String) -> Signal, name: &str| Some(kind(name.to_owned()));
    match code - MOUSE_CODE_FIRST {
        0 => named(Signal::Mouse, "left"),
        1 => named(Signal::Mouse, "right"),
        2 => named(Signal::Mouse, "middle"),
        3 => named(Signal::Mouse, "x1"),
        4 => named(Signal::Mouse, "x2"),
        5 => named(Signal::Wheel, "up"),
        6 => named(Signal::Wheel, "down"),
        7 => named(Signal::Wheel, "left"),
        8 => named(Signal::Wheel, "right"),
        9 => named(Signal::Move, "up"),
        10 => named(Signal::Move, "down"),
        11 => named(Signal::Move, "left"),
        12 => named(Signal::Move, "right"),
        _ => None,
    }
}

const PAD_BITS: [(u32, &str); 14] = [
    (0x0001, "Up"),
    (0x0002, "Down"),
    (0x0004, "Left"),
    (0x0008, "Right"),
    (0x0010, "Start"),
    (0x0020, "Back"),
    (0x0040, "LS"),
    (0x0080, "RS"),
    (0x0100, "LB"),
    (0x0200, "RB"),
    (0x1000, "A"),
    (0x2000, "B"),
    (0x4000, "X"),
    (0x8000, "Y"),
];

/// Reads the probe's log into groups of signals: a new group starts after 0.6 s without input.
///
/// Key and button releases and a centred pad end nothing by themselves; they are not signals.
#[must_use]
pub fn groups(log: &str) -> Vec<BTreeSet<Signal>> {
    let mut groups: Vec<BTreeSet<Signal>> = Vec::new();
    let mut last_at: Option<f64> = None;
    for line in log.lines() {
        let Some((at, event)) = line.split_once(' ') else {
            continue;
        };
        let Ok(at) = at.parse::<f64>() else {
            continue;
        };
        let signals = signals(event);
        if signals.is_empty() {
            continue;
        }
        let fresh = last_at.is_none_or(|last| at - last > GAP_SECONDS);
        if fresh || groups.is_empty() {
            groups.push(BTreeSet::new());
        }
        if let Some(group) = groups.last_mut() {
            group.extend(signals);
        }
        last_at = Some(at);
    }
    groups
}

fn signals(event: &str) -> Vec<Signal> {
    let words: Vec<&str> = event.split_whitespace().collect();
    match words.as_slice() {
        ["key", "down", code] => parse_hex(code).map(Signal::Key).into_iter().collect(),
        ["mouse", "down", name] => vec![Signal::Mouse((*name).to_owned())],
        ["wheel", name] => vec![Signal::Wheel((*name).to_owned())],
        ["move", dx, dy] => {
            let (Ok(dx), Ok(dy)) = (dx.parse::<i64>(), dy.parse::<i64>()) else {
                return Vec::new();
            };
            let mut found = Vec::new();
            if dx.abs() >= dy.abs() && dx != 0 {
                found.push(Signal::Move(
                    if dx > 0 { "right" } else { "left" }.to_owned(),
                ));
            } else if dy != 0 {
                found.push(Signal::Move(if dy > 0 { "down" } else { "up" }.to_owned()));
            }
            found
        }
        ["pad", fields @ ..] if fields.first().is_some_and(|f| f.starts_with("buttons=")) => {
            pad_signals(fields)
        }
        _ => Vec::new(),
    }
}

fn pad_signals(fields: &[&str]) -> Vec<Signal> {
    let value = |name: &str| {
        fields
            .iter()
            .find_map(|field| field.strip_prefix(name)?.strip_prefix('='))
    };
    let mut found = Vec::new();
    if let Some(buttons) = value("buttons").and_then(|text| parse_hex(text).map(u32::from)) {
        for (bit, name) in PAD_BITS {
            if buttons & bit != 0 {
                found.push(Signal::Pad(name.to_owned()));
            }
        }
    }
    for (field, name) in [("lt", "LT"), ("rt", "RT")] {
        if value(field)
            .and_then(|text| text.parse::<u32>().ok())
            .is_some_and(|pulled| pulled >= TRIGGER_THRESHOLD)
        {
            found.push(Signal::Trigger(name.to_owned()));
        }
    }
    for (field, axis) in [("lx", "LX"), ("ly", "LY"), ("rx", "RX"), ("ry", "RY")] {
        if let Some(position) = value(field).and_then(|text| text.parse::<i32>().ok()) {
            if position > AXIS_THRESHOLD {
                found.push(Signal::Stick(format!("{axis}+")));
            } else if position < -AXIS_THRESHOLD {
                found.push(Signal::Stick(format!("{axis}-")));
            }
        }
    }
    found
}

fn parse_hex(text: &str) -> Option<u16> {
    u16::from_str_radix(text.strip_prefix("0x").unwrap_or(text), 16).ok()
}

/// Pairs the presses with the probe's groups in order and returns what each press gave.
///
/// A press whose button is bound to nothing expects nothing and takes no group, so the next press keeps its
/// own group.
#[must_use]
pub fn check(input: &InputSettings, presses: &[SwitchButton], log: &str) -> Vec<PressCheck> {
    let mut groups = groups(log).into_iter();
    presses
        .iter()
        .map(|&button| {
            let expected = expected(input, button);
            let observed = if expected.is_empty() {
                BTreeSet::new()
            } else {
                groups.next().unwrap_or_default()
            };
            PressCheck {
                button,
                expected,
                observed,
            }
        })
        .collect()
}

/// Returns a signal set as text, such as `pad A + key 0x41`.
#[must_use]
pub fn describe(signals: &BTreeSet<Signal>) -> String {
    if signals.is_empty() {
        return "nothing".to_owned();
    }
    signals
        .iter()
        .map(|signal| match signal {
            Signal::Pad(name) => format!("pad {name}"),
            Signal::Trigger(name) => format!("trigger {name}"),
            Signal::Stick(name) => format!("stick {name}"),
            Signal::Key(code) => format!("key 0x{code:02x}"),
            Signal::Mouse(name) => format!("mouse {name}"),
            Signal::Wheel(name) => format!("wheel {name}"),
            Signal::Move(name) => format!("cursor {name}"),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

#[cfg(test)]
mod tests {
    use carafe_core::settings::Binding;

    use super::*;

    fn set(signals: &[Signal]) -> BTreeSet<Signal> {
        signals.iter().cloned().collect()
    }

    #[test]
    fn default_controller_maps_by_position() {
        let input = InputSettings::default();
        assert_eq!(
            expected(&input, SwitchButton::A),
            set(&[Signal::Pad("B".to_owned())])
        );
        assert_eq!(
            expected(&input, SwitchButton::B),
            set(&[Signal::Pad("A".to_owned())])
        );
        assert_eq!(
            expected(&input, SwitchButton::Zr),
            set(&[Signal::Trigger("RT".to_owned())])
        );
        assert_eq!(
            expected(&input, SwitchButton::Plus),
            set(&[Signal::Pad("Start".to_owned())])
        );
        assert_eq!(
            expected(&input, SwitchButton::LUp),
            set(&[Signal::Stick("LY+".to_owned())])
        );
        assert_eq!(
            expected(&input, SwitchButton::RLeft),
            set(&[Signal::Stick("RX-".to_owned())])
        );
    }

    #[test]
    fn default_keyboard_uses_keys_and_mouse() {
        let input = InputSettings {
            mode: InputMode::KeyboardMouse,
            ..InputSettings::default()
        };
        assert_eq!(
            expected(&input, SwitchButton::A),
            set(&[Signal::Mouse("left".to_owned())])
        );
        assert_eq!(expected(&input, SwitchButton::X), set(&[Signal::Key(0x20)]));
        assert_eq!(
            expected(&input, SwitchButton::RUp),
            set(&[Signal::Move("up".to_owned())])
        );
    }

    #[test]
    fn chords_expect_every_key() {
        let mut input = InputSettings {
            mode: InputMode::KeyboardMouse,
            ..InputSettings::default()
        };
        input.keys[0] = Binding {
            button: SwitchButton::A,
            codes: vec![0x11, 0x53],
        };
        assert_eq!(
            expected(&input, SwitchButton::A),
            set(&[Signal::Key(0x11), Signal::Key(0x53)])
        );
    }

    #[test]
    fn groups_split_on_quiet_gaps() {
        let log = "\
0.001 probe input ready, 64-bit, log C:\\probe-input.log
0.200 pad connected
0.210 pad buttons=0x0000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
2.000 pad buttons=0x2000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
2.150 pad buttons=0x0000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
3.500 pad buttons=0x0000 lt=0 rt=255 lx=0 ly=0 rx=0 ry=0
3.650 pad buttons=0x0000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
5.000 pad buttons=0x0000 lt=0 rt=0 lx=0 ly=32767 rx=0 ry=0
";
        assert_eq!(
            groups(log),
            vec![
                set(&[Signal::Pad("B".to_owned())]),
                set(&[Signal::Trigger("RT".to_owned())]),
                set(&[Signal::Stick("LY+".to_owned())]),
            ]
        );
    }

    #[test]
    fn keyboard_groups_hold_chords_and_moves() {
        let log = "\
1.000 key down 0x11
1.010 key down 0x53
1.200 key up 0x53
1.210 key up 0x11
3.000 move 0 -40
3.100 move 0 -40
5.000 mouse down left
5.100 mouse up left
";
        assert_eq!(
            groups(log),
            vec![
                set(&[Signal::Key(0x11), Signal::Key(0x53)]),
                set(&[Signal::Move("up".to_owned())]),
                set(&[Signal::Mouse("left".to_owned())]),
            ]
        );
    }

    #[test]
    fn check_pairs_presses_with_groups() {
        let input = InputSettings::default();
        let log = "\
2.000 pad buttons=0x2000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
2.150 pad buttons=0x0000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
4.000 pad buttons=0x2000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0
";
        let checks = check(&input, &[SwitchButton::A, SwitchButton::B], log);
        assert!(checks[0].ok());
        assert!(!checks[1].ok());
        assert_eq!(describe(&checks[1].observed), "pad B");
        assert_eq!(describe(&checks[1].expected), "pad A");
    }

    #[test]
    fn unbound_buttons_expect_nothing_and_take_no_group() {
        let mut input = InputSettings::default();
        input.pad[0].codes.clear();
        let log = "2.000 pad buttons=0x1000 lt=0 rt=0 lx=0 ly=0 rx=0 ry=0\n";
        let checks = check(&input, &[SwitchButton::A, SwitchButton::B], log);
        assert!(checks[0].ok());
        assert!(checks[1].ok(), "{:?}", checks[1]);
    }

    #[test]
    fn a_missing_group_is_nothing_observed() {
        let checks = check(&InputSettings::default(), &[SwitchButton::A], "");
        assert!(!checks[0].ok());
        assert_eq!(describe(&checks[0].observed), "nothing");
    }
}
