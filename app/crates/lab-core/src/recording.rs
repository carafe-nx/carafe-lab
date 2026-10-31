//! A recorded pass: what a player did with a running game through `press`, `screen`, `pause` and `resume`, and the
//! scenario that replays it.

use carafe_core::settings::SwitchButton;
use serde::{Deserialize, Serialize};

use crate::scenario::{Milestone, Scenario, Step};

/// The shortest stretch of the game running between two presses that the replay waits out.
const SHORTEST_WAIT_MS: u64 = 200;

/// Seconds a replay waits for each recorded picture at least.
const SCREEN_TIMEOUT_MIN: u32 = 60;

/// One thing the player did, with when it was done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    /// The game was launched for the pass.
    #[serde(rename_all = "camelCase")]
    Start {
        /// Unix milliseconds.
        at: u64,
        /// The game's name in `lab.json`.
        game: String,
    },
    /// The game drew its first frame.
    FirstFrame {
        /// Unix milliseconds.
        at: u64,
    },
    /// The game stood still.
    Pause {
        /// Unix milliseconds.
        at: u64,
    },
    /// The game went on.
    Resume {
        /// Unix milliseconds.
        at: u64,
    },
    /// Buttons were held together for `hold` seconds; a paused game went on for the press and `after` seconds more.
    #[serde(rename_all = "camelCase")]
    Press {
        /// Unix milliseconds when the press began.
        at: u64,
        /// The buttons.
        buttons: Vec<SwitchButton>,
        /// Seconds held.
        hold: f32,
        /// Seconds the paused game went on after the press; `0` when it was not paused.
        after: f32,
    },
    /// The screen was touched at a point of 1280×720; a paused game went on for the touch and `after` seconds more.
    #[serde(rename_all = "camelCase")]
    Tap {
        /// Unix milliseconds.
        at: u64,
        /// The point across the screen.
        x: u32,
        /// The point down the screen.
        y: u32,
        /// Seconds the paused game went on after the touch; `0` when it was not paused.
        after: f32,
    },
    /// The screen was saved to `file`, which becomes a point of the pass.
    #[serde(rename_all = "camelCase")]
    Screen {
        /// Unix milliseconds.
        at: u64,
        /// The picture.
        file: String,
    },
}

/// A point of the pass: a picture that becomes a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    /// The picture the player saved.
    pub file: String,
    /// Its path in the references folder, such as `nfsu-race/point-1.png`.
    pub reference: String,
}

/// Returns the scenario that replays `events` and the pictures to copy into the references folder under `name`.
///
/// The replay launches the game and waits for its first frame, then waits for each recorded picture until the
/// screen matches it, presses what the player pressed and waits out the time the game ran between two presses;
/// time it stood paused does not count. It ends by closing the game.
#[must_use]
pub fn scenario(events: &[Event], name: &str, first_frame_timeout: u32) -> (Scenario, Vec<Point>) {
    let mut steps = vec![
        Step::Launch,
        Step::WaitFor {
            milestone: Milestone::FirstFrame,
            timeout: first_frame_timeout,
        },
    ];
    let mut points = Vec::new();
    let mut paused = false;
    let mut since: Option<u64> = None;
    let mut ran_ms: u64 = 0;
    let mut seen_point = false;
    let mut first_frame = false;
    for event in events {
        match event {
            Event::Start { at, .. } => {
                since = Some(*at);
                ran_ms = 0;
                paused = false;
                first_frame = false;
            }
            Event::FirstFrame { at } => {
                since = Some(*at);
                ran_ms = 0;
                first_frame = true;
            }
            Event::Pause { at } => {
                run_until(*at, &mut since, &mut ran_ms, paused);
                paused = true;
            }
            Event::Resume { at } => {
                run_until(*at, &mut since, &mut ran_ms, paused);
                paused = false;
            }
            Event::Press {
                at,
                buttons,
                hold,
                after,
            } => {
                run_until(*at, &mut since, &mut ran_ms, paused);
                if seen_point && ran_ms >= SHORTEST_WAIT_MS {
                    steps.push(Step::Wait {
                        seconds: ran_ms as f32 / 1000.0,
                    });
                }
                steps.push(Step::Press {
                    buttons: buttons.clone(),
                    hold: *hold,
                });
                seen_point = true;
                if paused {
                    ran_ms = (after * 1000.0) as u64;
                } else {
                    ran_ms = 0;
                    since = Some(*at + (hold * 1000.0) as u64);
                }
            }
            Event::Tap { at, x, y, after } => {
                run_until(*at, &mut since, &mut ran_ms, paused);
                if seen_point && ran_ms >= SHORTEST_WAIT_MS {
                    steps.push(Step::Wait {
                        seconds: ran_ms as f32 / 1000.0,
                    });
                }
                steps.push(Step::Tap { x: *x, y: *y });
                seen_point = true;
                ran_ms = if paused { (after * 1000.0) as u64 } else { 0 };
            }
            Event::Screen { at, file } => {
                run_until(*at, &mut since, &mut ran_ms, paused);
                if first_frame && !seen_point && ran_ms >= SHORTEST_WAIT_MS {
                    steps.push(Step::Wait {
                        seconds: ran_ms as f32 / 1000.0,
                    });
                }
                let number = points.len() + 1;
                let reference = format!("{name}/point-{number}.png");
                let timeout = SCREEN_TIMEOUT_MIN.max((ran_ms / 1000) as u32 * 3);
                steps.push(Step::WaitForScreen {
                    name: format!("point-{number}"),
                    reference: reference.clone(),
                    tolerance: 12.0,
                    timeout,
                });
                points.push(Point {
                    file: file.clone(),
                    reference,
                });
                seen_point = true;
                ran_ms = 0;
            }
        }
    }
    if seen_point && ran_ms >= SHORTEST_WAIT_MS {
        steps.push(Step::Wait {
            seconds: ran_ms as f32 / 1000.0,
        });
    }
    steps.push(Step::Close { timeout: 60 });
    (Scenario { steps }, points)
}

fn run_until(at: u64, since: &mut Option<u64>, ran_ms: &mut u64, paused: bool) {
    if let Some(from) = since.take()
        && !paused
    {
        *ran_ms += at.saturating_sub(from);
    }
    *since = Some(at);
}

/// Returns the events in the text of a recording, one JSON object a line; lines that do not read are left out.
#[must_use]
pub fn parse(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(at: u64, after: f32) -> Event {
        Event::Press {
            at,
            buttons: vec![SwitchButton::A],
            hold: 0.2,
            after,
        }
    }

    #[test]
    fn paused_time_does_not_count_and_each_picture_is_waited_for() {
        let events = vec![
            Event::Start {
                at: 0,
                game: "G".to_owned(),
            },
            Event::Pause { at: 40_000 },
            Event::Screen {
                at: 41_000,
                file: "a.png".to_owned(),
            },
            press(90_000, 0.5),
            press(200_000, 0.5),
            Event::Resume { at: 300_000 },
            Event::Pause { at: 310_000 },
            Event::Screen {
                at: 400_000,
                file: "b.png".to_owned(),
            },
        ];
        let (scenario, points) = scenario(&events, "g-menu", 300);
        assert_eq!(
            scenario.steps,
            vec![
                Step::Launch,
                Step::WaitFor {
                    milestone: Milestone::FirstFrame,
                    timeout: 300
                },
                Step::WaitForScreen {
                    name: "point-1".to_owned(),
                    reference: "g-menu/point-1.png".to_owned(),
                    tolerance: 12.0,
                    timeout: 120
                },
                Step::Press {
                    buttons: vec![SwitchButton::A],
                    hold: 0.2
                },
                Step::Wait { seconds: 0.5 },
                Step::Press {
                    buttons: vec![SwitchButton::A],
                    hold: 0.2
                },
                Step::WaitForScreen {
                    name: "point-2".to_owned(),
                    reference: "g-menu/point-2.png".to_owned(),
                    tolerance: 12.0,
                    timeout: 60
                },
                Step::Close { timeout: 60 },
            ]
        );
        assert_eq!(points[1].file, "b.png");
        assert!(scenario.check().is_ok());
    }

    #[test]
    fn a_running_game_waits_out_the_time_between_presses() {
        let events = vec![
            Event::Start {
                at: 0,
                game: "G".to_owned(),
            },
            Event::Screen {
                at: 30_000,
                file: "a.png".to_owned(),
            },
            press(31_000, 0.0),
            press(33_200, 0.0),
        ];
        let (scenario, _) = scenario(&events, "g", 300);
        assert_eq!(scenario.steps[3], Step::Wait { seconds: 1.0 });
        assert_eq!(scenario.steps[5], Step::Wait { seconds: 2.0 });
    }

    #[test]
    fn a_paused_touch_waits_out_its_after_time() {
        let events = vec![
            Event::Start {
                at: 0,
                game: "G".to_owned(),
            },
            Event::Pause { at: 10_000 },
            Event::Screen {
                at: 11_000,
                file: "a.png".to_owned(),
            },
            Event::Tap {
                at: 20_000,
                x: 640,
                y: 360,
                after: 1.5,
            },
            press(60_000, 0.5),
        ];
        let (scenario, _) = scenario(&events, "g", 300);
        assert_eq!(scenario.steps[3], Step::Tap { x: 640, y: 360 });
        assert_eq!(scenario.steps[4], Step::Wait { seconds: 1.5 });
    }

    #[test]
    fn the_time_before_the_first_picture_and_after_the_last_press_is_kept() {
        let events = vec![
            Event::Start {
                at: 0,
                game: "G".to_owned(),
            },
            Event::FirstFrame { at: 30_000 },
            Event::Pause { at: 42_000 },
            Event::Screen {
                at: 43_000,
                file: "a.png".to_owned(),
            },
            press(50_000, 3.0),
        ];
        let (scenario, _) = scenario(&events, "g", 300);
        assert_eq!(scenario.steps[2], Step::Wait { seconds: 12.0 });
        assert!(matches!(scenario.steps[3], Step::WaitForScreen { .. }));
        assert_eq!(scenario.steps[5], Step::Wait { seconds: 3.0 });
        assert_eq!(scenario.steps[6], Step::Close { timeout: 60 });
    }

    #[test]
    fn lines_that_do_not_read_are_left_out() {
        let text = "{\"event\":\"pause\",\"at\":5}\nnot json\n{\"event\":\"resume\",\"at\":9}\n";
        assert_eq!(
            parse(text),
            vec![Event::Pause { at: 5 }, Event::Resume { at: 9 }]
        );
    }
}
