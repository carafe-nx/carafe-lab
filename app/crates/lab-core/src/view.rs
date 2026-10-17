//! `lab.json` as the window and `carafe-lab games` show it.

use std::collections::BTreeMap;

use carafe_core::settings::AutorunSettings;
use serde::Serialize;
use ts_rs::TS;

use crate::config::{Config, Game};
use crate::scenario::Scenario;

/// The lab's settings and games, for showing.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabView {
    /// The lab's own folder.
    pub work: String,
    /// What draws the emulator's picture: `wsl` or `software`.
    pub gpu: String,
    /// How the emulator runs the Switch's processor code: `dynarmic` or `nce`.
    pub emulator_cpu: String,
    /// Seconds a game may take to its first frame.
    pub timeout: u32,
    /// Every game in file order.
    pub games: Vec<GameView>,
    /// Named sets of game names.
    pub sets: BTreeMap<String, Vec<String>>,
}

/// A game of the lab, for showing.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameView {
    /// Its name in `lab.json`.
    pub name: String,
    /// The game folder.
    pub folder: String,
    /// The game runs when no set is asked for.
    pub enabled: bool,
    /// The Title ID the game always gets in the lab.
    pub title_id: String,
    /// The `.exe` named in `lab.json`; `None` means the one the build wizard picks.
    pub executable: Option<String>,
    /// The title named in `lab.json`; `None` means the one the build wizard shows.
    pub title: Option<String>,
    /// The settings the NSP is built with.
    pub settings: AutorunSettings,
    /// What the lab does in the game.
    pub scenario: Scenario,
    /// The game has its own scenario rather than the standard one.
    pub own_scenario: bool,
    /// The sets the game is in.
    pub sets: Vec<String>,
}

impl LabView {
    /// Creates the view of `config`.
    #[must_use]
    pub fn of(config: &Config) -> Self {
        Self {
            work: config.work.display().to_string(),
            gpu: config.gpu.name().to_owned(),
            emulator_cpu: config.emulator_cpu.name().to_owned(),
            timeout: config.timeout,
            games: config
                .games
                .iter()
                .map(|game| GameView::of(game, config))
                .collect(),
            sets: config.sets.clone(),
        }
    }
}

impl GameView {
    fn of(game: &Game, config: &Config) -> Self {
        Self {
            name: game.name.clone(),
            folder: game.folder.display().to_string(),
            enabled: game.enabled,
            title_id: game.title_id().to_string(),
            executable: game.executable.clone(),
            title: game.title.clone(),
            settings: game.settings.clone(),
            scenario: game.scenario(config.timeout),
            own_scenario: game.scenario.is_some(),
            sets: config
                .sets
                .iter()
                .filter(|(_, names)| {
                    names
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&game.name))
                })
                .map(|(set, _)| set.clone())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_knows_its_sets_and_scenario() {
        let config = Config::parse(
            r#"{"work": "W:\\lab", "timeout": 90,
                "games": [{"name": "A", "folder": "D:\\A"}, {"name": "B", "folder": "D:\\B", "enabled": false}],
                "sets": {"quick": ["a"], "all": ["A", "B"]}}"#,
        )
        .unwrap();
        let view = LabView::of(&config);
        assert_eq!(view.games[0].sets, ["all", "quick"]);
        assert_eq!(view.games[1].sets, ["all"]);
        assert!(!view.games[0].own_scenario);
        assert_eq!(view.games[0].scenario, Scenario::standard(90));
        assert_eq!(view.gpu, "wsl");
    }
}
