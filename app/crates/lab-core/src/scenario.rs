//! A scenario: the steps the lab takes with one game, read from JSON.

use carafe_core::settings::SwitchButton;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

/// A point in the launch the lab can wait for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Milestone {
    /// Wine's virtual memory is ready.
    Wine,
    /// The loading screen is drawn.
    LoadingScreen,
    /// The game reached the screen.
    FirstFrame,
    /// The device save data was committed at least once since the launch.
    Commit,
}

/// One step of a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "step", rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub enum Step {
    /// Starts the game in the emulator.
    Launch,
    /// Waits until the logs show `milestone`; fails after `timeout` seconds.
    #[serde(rename_all = "camelCase")]
    WaitFor {
        /// What to wait for.
        milestone: Milestone,
        /// Seconds to wait at most.
        timeout: u32,
    },
    /// Waits a fixed time.
    Wait {
        /// Seconds to wait.
        seconds: f32,
    },
    /// Presses and releases buttons together.
    #[serde(rename_all = "camelCase")]
    Press {
        /// The buttons, held together.
        buttons: Vec<SwitchButton>,
        /// Seconds the buttons stay down.
        #[serde(default = "default_hold")]
        hold: f32,
    },
    /// Saves a picture of the screen under `name`.
    Screenshot {
        /// Name of the picture, unique in the scenario.
        name: String,
    },
    /// Checks that the screen changed between two pictures: the input reached the game.
    ExpectChange {
        /// The picture before.
        from: String,
        /// The picture after.
        to: String,
        /// The step only warns when the screen did not change.
        #[serde(default)]
        soft: bool,
        /// Pairs of pictures taken as far apart as `from` and `to` with no input between: when the screen changed
        /// about as much on its own in any of them, the step warns that it cannot tell.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(as = "Option<Vec<[String; 2]>>", optional)]
        quiet: Vec<[String; 2]>,
    },
    /// Checks that a picture shows the graphics probes' four quadrants.
    ExpectPattern {
        /// The picture.
        name: String,
    },
    /// Checks the input probe's log: every single-button press since the launch gave what the build's bindings say.
    ExpectInput,
    /// Checks that the game has written its save data and remembers it for [`Step::ExpectSaveKept`].
    ExpectSave,
    /// Checks that the save data remembered by [`Step::ExpectSave`] is still there and unchanged.
    ExpectSaveKept,
    /// Checks the first launch's time to the first frame and median FPS against the newest other Carafe version: the
    /// step fails when either is worse by more than the threshold.
    #[serde(rename_all = "camelCase")]
    ExpectPerformance {
        /// The percentage the game may get worse by; `None` takes the game's threshold from `lab.json`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        threshold: Option<u32>,
    },
    /// Saves a picture of the screen under `name` and checks that it shows what the reference picture shows: it fails
    /// when the two differ by more than `tolerance`.
    #[serde(rename_all = "camelCase")]
    ExpectScreen {
        /// Name of the picture, unique in the scenario.
        name: String,
        /// The reference picture, a path in the `references` folder next to `lab.json`, such as `openttd/menu.png`.
        reference: String,
        /// The mean brightness difference, 0–255, the two pictures may have.
        #[serde(default = "default_tolerance")]
        tolerance: f32,
    },
    /// Presses random buttons for `seconds`, to catch a crash or a hang without a scenario; fails when the game ends.
    #[serde(rename_all = "camelCase")]
    RandomPresses {
        /// Seconds to press for.
        seconds: u32,
        /// The seed of the presses, so a run that broke the game can press the same again.
        #[serde(default = "default_seed")]
        #[ts(type = "number")]
        seed: u64,
    },
    /// Runs `steps` `times` times in a row; the screenshots taken inside get `-1`, `-2`… after their names.
    #[serde(rename_all = "camelCase")]
    Repeat {
        /// How many times.
        times: u32,
        /// The steps, which neither launch nor close the game.
        steps: Vec<Step>,
    },
    /// Looks at the screen and runs `then` when it shows what the reference picture shows, `otherwise` when it does
    /// not; neither branch fails the step.
    #[serde(rename_all = "camelCase")]
    IfScreen {
        /// The reference picture, a path in the `references` folder next to `lab.json`.
        reference: String,
        /// The mean brightness difference, 0–255, the screen may have from the reference.
        #[serde(default = "default_tolerance")]
        tolerance: f32,
        /// The steps when the screen shows it.
        #[serde(default)]
        then: Vec<Step>,
        /// The steps when it does not.
        #[serde(default)]
        otherwise: Vec<Step>,
    },
    /// Asks the game to close the way the HOME Menu does, and waits for it to end.
    #[serde(rename_all = "camelCase")]
    Close {
        /// Seconds to wait for the game to end before it is stopped by force.
        #[serde(default = "default_close_timeout")]
        timeout: u32,
    },
}

/// The longest [`Step::RandomPresses`], in seconds.
pub const MAX_RANDOM_SECONDS: u32 = 3600;

/// The most times a [`Step::Repeat`] runs its steps.
pub const MAX_REPEAT: u32 = 1000;

fn default_tolerance() -> f32 {
    12.0
}

fn default_seed() -> u64 {
    1
}

fn default_hold() -> f32 {
    0.15
}

fn default_close_timeout() -> u32 {
    60
}

/// A scenario: steps in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct Scenario {
    /// The steps.
    pub steps: Vec<Step>,
}

/// An error in a scenario.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScenarioError {
    /// The JSON does not describe a scenario.
    #[error("not a scenario: {0}")]
    Format(String),
    /// A step comes before the game is launched or after it was closed.
    #[error("step {0} needs a running game: add a launch step before it")]
    NotRunning(usize),
    /// The game is launched twice without closing it.
    #[error("step {0} launches a game that is already running")]
    AlreadyRunning(usize),
    /// Two screenshots share a name.
    #[error("step {0} reuses the screenshot name {1}")]
    DuplicateScreenshot(usize, String),
    /// A comparison names a screenshot no earlier step takes.
    #[error("step {0} compares the screenshot {1}, which no earlier step takes")]
    UnknownScreenshot(usize, String),
    /// [`Step::ExpectSaveKept`] without an earlier [`Step::ExpectSave`].
    #[error("step {0} checks kept save data, but no earlier step remembers it")]
    NothingRemembered(usize),
    /// A time is negative, not a number or zero where it must not be.
    #[error("step {0} has an invalid time")]
    Time(usize),
}

impl Scenario {
    /// Reads and checks a scenario.
    ///
    /// # Errors
    ///
    /// [`ScenarioError::Format`] if `json` does not describe a scenario; another [`ScenarioError`] if the steps
    /// do not make sense in this order.
    pub fn parse(json: &str) -> Result<Self, ScenarioError> {
        let scenario: Self =
            serde_json::from_str(json).map_err(|error| ScenarioError::Format(error.to_string()))?;
        scenario.check()?;
        Ok(scenario)
    }

    /// Checks that the steps make sense in this order.
    ///
    /// # Errors
    ///
    /// The first [`ScenarioError`] found; step numbers count from one.
    pub fn check(&self) -> Result<(), ScenarioError> {
        check_steps(&self.steps, &mut Checking::default(), false)
    }
}

#[derive(Default)]
struct Checking<'a> {
    running: bool,
    screenshots: Vec<&'a str>,
    remembered: bool,
    number: usize,
}

fn check_steps<'a>(
    steps: &'a [Step],
    state: &mut Checking<'a>,
    in_block: bool,
) -> Result<(), ScenarioError> {
    {
        for step in steps {
            state.number += 1;
            let number = state.number;
            let needs_game = matches!(
                step,
                Step::WaitFor { .. }
                    | Step::Press { .. }
                    | Step::Screenshot { .. }
                    | Step::Close { .. }
                    | Step::RandomPresses { .. }
                    | Step::ExpectScreen { .. }
                    | Step::Repeat { .. }
                    | Step::IfScreen { .. }
                    | Step::ExpectInput
            );
            if needs_game && !state.running {
                return Err(ScenarioError::NotRunning(number));
            }
            if in_block && matches!(step, Step::Launch | Step::Close { .. }) {
                return Err(ScenarioError::Format(format!(
                    "step {number} launches or closes the game inside a block"
                )));
            }
            let running = &mut state.running;
            let screenshots = &mut state.screenshots;
            let remembered = &mut state.remembered;
            match step {
                Step::Repeat { times, steps } => {
                    if *times == 0 || *times > MAX_REPEAT {
                        return Err(ScenarioError::Format(format!(
                            "step {number} repeats {times} times; 1 to {MAX_REPEAT} are allowed"
                        )));
                    }
                    check_steps(steps, state, true)?;
                    continue;
                }
                Step::IfScreen {
                    then, otherwise, ..
                } => {
                    check_steps(then, state, true)?;
                    check_steps(otherwise, state, true)?;
                    continue;
                }
                Step::Launch => {
                    if *running {
                        return Err(ScenarioError::AlreadyRunning(number));
                    }
                    *running = true;
                }
                Step::Close { .. } => *running = false,
                Step::Wait { seconds } => check_time(*seconds, true, number)?,
                Step::RandomPresses { seconds, .. }
                    if *seconds == 0 || *seconds > MAX_RANDOM_SECONDS =>
                {
                    return Err(ScenarioError::Time(number));
                }
                Step::Press { hold, buttons } => {
                    check_time(*hold, false, number)?;
                    if buttons.is_empty() {
                        return Err(ScenarioError::Format(format!(
                            "step {number} presses no buttons"
                        )));
                    }
                }
                Step::WaitFor { timeout, .. } if *timeout == 0 => {
                    return Err(ScenarioError::Time(number));
                }
                Step::Screenshot { name } | Step::ExpectScreen { name, .. } => {
                    if screenshots.contains(&name.as_str()) {
                        return Err(ScenarioError::DuplicateScreenshot(number, name.clone()));
                    }
                    screenshots.push(name);
                }
                Step::ExpectChange {
                    from, to, quiet, ..
                } => {
                    for name in [from, to].into_iter().chain(quiet.iter().flatten()) {
                        if !screenshots.contains(&name.as_str()) {
                            return Err(ScenarioError::UnknownScreenshot(number, name.clone()));
                        }
                    }
                }
                Step::ExpectPattern { name } if !screenshots.contains(&name.as_str()) => {
                    return Err(ScenarioError::UnknownScreenshot(number, name.clone()));
                }
                Step::ExpectSave => *remembered = true,
                Step::ExpectSaveKept if !*remembered => {
                    return Err(ScenarioError::NothingRemembered(number));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

impl Scenario {
    /// Returns the scenario for a game that has none of its own.
    ///
    /// It launches the game and waits for the first frame, presses A and looks for a change on the screen
    /// (a warning only), closes the game the way the HOME Menu does, checks the save data, launches the game
    /// again and checks that the save data is still there.
    #[must_use]
    pub fn standard(first_frame_timeout: u32) -> Self {
        let wait_for_frame = Step::WaitFor {
            milestone: Milestone::FirstFrame,
            timeout: first_frame_timeout,
        };
        Self {
            steps: vec![
                Step::Launch,
                wait_for_frame.clone(),
                Step::Wait { seconds: 5.0 },
                Step::Screenshot {
                    name: "still".to_owned(),
                },
                Step::Wait { seconds: 5.0 },
                Step::Screenshot {
                    name: "first-frame".to_owned(),
                },
                Step::Press {
                    buttons: vec![SwitchButton::A],
                    hold: default_hold(),
                },
                Step::Wait { seconds: 5.0 },
                Step::Screenshot {
                    name: "after-a".to_owned(),
                },
                Step::Wait { seconds: 5.0 },
                Step::Screenshot {
                    name: "settled".to_owned(),
                },
                Step::ExpectChange {
                    from: "first-frame".to_owned(),
                    to: "after-a".to_owned(),
                    soft: true,
                    quiet: vec![
                        ["still".to_owned(), "first-frame".to_owned()],
                        ["after-a".to_owned(), "settled".to_owned()],
                    ],
                },
                Step::Close {
                    timeout: default_close_timeout(),
                },
                Step::ExpectSave,
                Step::ExpectPerformance { threshold: None },
                Step::Launch,
                wait_for_frame,
                Step::ExpectSaveKept,
                Step::Wait { seconds: 5.0 },
                Step::Screenshot {
                    name: "relaunch".to_owned(),
                },
                Step::Close {
                    timeout: default_close_timeout(),
                },
            ],
        }
    }
}

impl Scenario {
    /// Returns the scenario for a graphics probe: wait for the first frame, take a picture, check the pattern.
    #[must_use]
    pub fn graphics_probe(first_frame_timeout: u32) -> Self {
        Self {
            steps: vec![
                Step::Launch,
                Step::WaitFor {
                    milestone: Milestone::FirstFrame,
                    timeout: first_frame_timeout,
                },
                Step::Wait { seconds: 5.0 },
                Step::Screenshot {
                    name: "pattern".to_owned(),
                },
                Step::ExpectPattern {
                    name: "pattern".to_owned(),
                },
                Step::Close {
                    timeout: default_close_timeout(),
                },
            ],
        }
    }

    /// Returns the scenario for the input probe: every Switch button pressed once, then the probe's log checked.
    #[must_use]
    pub fn input_probe(first_frame_timeout: u32) -> Self {
        let mut steps = vec![
            Step::Launch,
            Step::WaitFor {
                milestone: Milestone::FirstFrame,
                timeout: first_frame_timeout,
            },
            Step::Wait { seconds: 5.0 },
            Step::Screenshot {
                name: "before".to_owned(),
            },
        ];
        for button in SwitchButton::ALL {
            steps.push(Step::Press {
                buttons: vec![button],
                hold: 0.3,
            });
            steps.push(Step::Wait { seconds: 1.5 });
        }
        steps.extend([
            Step::Wait { seconds: 3.0 },
            Step::Screenshot {
                name: "after".to_owned(),
            },
            Step::ExpectInput,
            Step::Close {
                timeout: default_close_timeout(),
            },
        ]);
        Self { steps }
    }
}

fn check_time(seconds: f32, zero_allowed: bool, number: usize) -> Result<(), ScenarioError> {
    let ok = seconds.is_finite() && (seconds > 0.0 || (zero_allowed && seconds == 0.0));
    if ok {
        Ok(())
    } else {
        Err(ScenarioError::Time(number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_scenario_is_valid() {
        assert_eq!(Scenario::standard(600).check(), Ok(()));
    }

    #[test]
    fn blocks_need_a_running_game_and_neither_launch_nor_close_it() {
        let json = r#"{ "steps": [
            { "step": "launch" },
            { "step": "repeat", "times": 3, "steps": [
                { "step": "press", "buttons": ["A"] },
                { "step": "ifScreen", "reference": "game/menu.png",
                  "then": [{ "step": "screenshot", "name": "menu" }],
                  "otherwise": [{ "step": "wait", "seconds": 1 }] }
            ] },
            { "step": "close" }
        ] }"#;
        let scenario = Scenario::parse(json).unwrap();
        assert!(matches!(&scenario.steps[1], Step::Repeat { times: 3, steps } if steps.len() == 2));
        let closing = r#"{ "steps": [ { "step": "launch" },
            { "step": "repeat", "times": 2, "steps": [{ "step": "close" }] } ] }"#;
        assert!(matches!(
            Scenario::parse(closing),
            Err(ScenarioError::Format(_))
        ));
        let never = r#"{ "steps": [ { "step": "launch" }, { "step": "repeat", "times": 0, "steps": [] } ] }"#;
        assert!(matches!(
            Scenario::parse(never),
            Err(ScenarioError::Format(_))
        ));
        let early = r#"{ "steps": [ { "step": "repeat", "times": 2, "steps": [] } ] }"#;
        assert_eq!(Scenario::parse(early), Err(ScenarioError::NotRunning(1)));
    }

    #[test]
    fn the_probe_scenarios_are_valid() {
        assert_eq!(Scenario::graphics_probe(600).check(), Ok(()));
        let input = Scenario::input_probe(600);
        assert_eq!(input.check(), Ok(()));
        let presses = input
            .steps
            .iter()
            .filter(|step| matches!(step, Step::Press { .. }))
            .count();
        assert_eq!(presses, SwitchButton::ALL.len());
    }

    #[test]
    fn a_pattern_needs_its_picture() {
        let json = r#"{"steps": [{"step": "launch"}, {"step": "expectPattern", "name": "x"}]}"#;
        assert_eq!(
            Scenario::parse(json),
            Err(ScenarioError::UnknownScreenshot(2, "x".to_owned()))
        );
    }

    #[test]
    fn reads_a_scenario_from_json() {
        let json = r#"{"steps": [
            {"step": "launch"},
            {"step": "waitFor", "milestone": "firstFrame", "timeout": 300},
            {"step": "press", "buttons": ["A", "ZR"]},
            {"step": "screenshot", "name": "menu"},
            {"step": "close"}
        ]}"#;
        let scenario = Scenario::parse(json).unwrap();
        assert_eq!(scenario.steps.len(), 5);
        assert_eq!(
            scenario.steps[2],
            Step::Press {
                buttons: vec![SwitchButton::A, SwitchButton::Zr],
                hold: 0.15
            }
        );
        assert_eq!(scenario.steps[4], Step::Close { timeout: 60 });
    }

    #[test]
    fn rejects_unknown_steps_and_fields() {
        let unknown = r#"{"steps": [{"step": "jump"}]}"#;
        assert!(matches!(
            Scenario::parse(unknown),
            Err(ScenarioError::Format(_))
        ));
        let extra = r#"{"steps": [{"step": "wait", "seconds": 1, "fast": true}]}"#;
        assert!(matches!(
            Scenario::parse(extra),
            Err(ScenarioError::Format(_))
        ));
    }

    #[test]
    fn a_press_needs_a_running_game() {
        let json = r#"{"steps": [{"step": "press", "buttons": ["A"]}]}"#;
        assert_eq!(Scenario::parse(json), Err(ScenarioError::NotRunning(1)));
    }

    #[test]
    fn a_second_launch_needs_a_close_first() {
        let json = r#"{"steps": [{"step": "launch"}, {"step": "launch"}]}"#;
        assert_eq!(Scenario::parse(json), Err(ScenarioError::AlreadyRunning(2)));
    }

    #[test]
    fn comparisons_need_earlier_screenshots() {
        let json = r#"{"steps": [
            {"step": "launch"},
            {"step": "screenshot", "name": "a"},
            {"step": "expectChange", "from": "a", "to": "b"}
        ]}"#;
        assert_eq!(
            Scenario::parse(json),
            Err(ScenarioError::UnknownScreenshot(3, "b".to_owned()))
        );
    }

    #[test]
    fn screenshot_names_are_unique() {
        let json = r#"{"steps": [
            {"step": "launch"},
            {"step": "screenshot", "name": "a"},
            {"step": "screenshot", "name": "a"}
        ]}"#;
        assert_eq!(
            Scenario::parse(json),
            Err(ScenarioError::DuplicateScreenshot(3, "a".to_owned()))
        );
    }

    #[test]
    fn kept_save_needs_a_remembered_one() {
        let json = r#"{"steps": [{"step": "expectSaveKept"}]}"#;
        assert_eq!(
            Scenario::parse(json),
            Err(ScenarioError::NothingRemembered(1))
        );
    }

    #[test]
    fn times_must_make_sense() {
        let negative = r#"{"steps": [{"step": "wait", "seconds": -1}]}"#;
        assert_eq!(Scenario::parse(negative), Err(ScenarioError::Time(1)));
        let zero_hold =
            r#"{"steps": [{"step": "launch"}, {"step": "press", "buttons": ["A"], "hold": 0}]}"#;
        assert_eq!(Scenario::parse(zero_hold), Err(ScenarioError::Time(2)));
        let zero_timeout = r#"{"steps": [{"step": "launch"}, {"step": "waitFor", "milestone": "wine", "timeout": 0}]}"#;
        assert_eq!(Scenario::parse(zero_timeout), Err(ScenarioError::Time(2)));
    }

    #[test]
    fn a_press_needs_buttons() {
        let json = r#"{"steps": [{"step": "launch"}, {"step": "press", "buttons": []}]}"#;
        assert!(matches!(
            Scenario::parse(json),
            Err(ScenarioError::Format(_))
        ));
    }
}
