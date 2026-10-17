//! `lab.json`: where the lab works, how it draws, and which games it checks.
//!
//! The repository keeps `lab.example.json`; each developer copies it to `lab.json`, which git ignores,
//! because the paths are their own.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use carafe_core::TitleId;
use carafe_core::metadata::Metadata;
use carafe_core::ports::FolderReport;
use carafe_core::record::{Arch, GameSource};
use carafe_core::settings::AutorunSettings;
use carafe_core::wizard::{self, BuildRequest};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::emulator::{EdenCpu, Gpu};
use crate::lang::Lang;
use crate::scenario::{Scenario, ScenarioError};
use crate::versions;

/// Name of the developer's own file, next to the template.
pub const CONFIG_FILE: &str = "lab.json";
/// Name of the template kept in the repository.
pub const TEMPLATE_FILE: &str = "lab.example.json";

/// An error in `lab.json`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// There is no `lab.json`.
    #[error("no {0}: copy {TEMPLATE_FILE} to {CONFIG_FILE} next to it and put your paths in")]
    Missing(String),
    /// The file could not be read.
    #[error("{0}: {1}")]
    Io(String, String),
    /// The file is not valid JSON or has unknown fields.
    #[error("{CONFIG_FILE}: {0}")]
    Format(String),
    /// `gpu`, `lang` or `timeout` has a value the lab does not know.
    #[error("{CONFIG_FILE}: {0} must be {1}")]
    Value(&'static str, &'static str),
    /// Two games share a name, ignoring case.
    #[error("{CONFIG_FILE}: two games are named {0}")]
    DuplicateName(String),
    /// A game name is empty or has characters a folder name cannot have.
    #[error("{CONFIG_FILE}: the game name {0:?} must be letters, digits, spaces, - or _")]
    Name(String),
    /// A set names a game that is not in `games`.
    #[error("{CONFIG_FILE}: set {0} names {1}, which is not in games")]
    UnknownInSet(String, String),
    /// A set asked for on the command line is not in `sets`.
    #[error("{CONFIG_FILE}: there is no set {0}")]
    UnknownSet(String),
    /// A game asked for by name is not in `games`.
    #[error("{CONFIG_FILE}: there is no game {0}")]
    UnknownGame(String),
    /// A game's settings, merged over the defaults, are not valid.
    #[error("{CONFIG_FILE}: {0}: settings: {1}")]
    Settings(String, String),
    /// A game's scenario file could not be read or is not a scenario.
    #[error("{CONFIG_FILE}: {0}: scenario file {1}: {2}")]
    ScenarioFile(String, String, String),
    /// A game's scenario does not make sense.
    #[error("{CONFIG_FILE}: {0}: scenario: {1}")]
    Scenario(String, ScenarioError),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawConfig {
    work: PathBuf,
    gpu: Option<String>,
    emulator_cpu: Option<String>,
    carafe: Option<PathBuf>,
    eden_image: Option<String>,
    one_eden: Option<bool>,
    #[serde(default)]
    eden_settings: BTreeMap<String, String>,
    lang: Option<String>,
    timeout: Option<u32>,
    #[serde(default)]
    games: Vec<RawGame>,
    #[serde(default)]
    sets: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawGame {
    name: String,
    folder: PathBuf,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    executable: Option<String>,
    title: Option<String>,
    arch: Option<Arch>,
    #[serde(default)]
    arguments: Vec<String>,
    settings: Option<Value>,
    scenario: Option<RawScenario>,
    #[serde(default)]
    tags: Vec<String>,
    notes: Option<String>,
    threshold: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawScenario {
    File(String),
    Steps(Scenario),
}

fn enabled_by_default() -> bool {
    true
}

/// The lab's settings and games.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The lab's own folder: NSPs, the emulator's data, runs.
    pub work: PathBuf,
    /// What draws the emulator's picture.
    pub gpu: Gpu,
    /// How the emulator runs the Switch's processor code.
    pub emulator_cpu: EdenCpu,
    /// The Carafe checkout the lab packs with; `None` means the `external/carafe` submodule.
    pub carafe: Option<PathBuf>,
    /// The emulator's Docker image; `None` means the image `compose.yaml` builds.
    pub eden_image: Option<String>,
    /// One emulator serves the whole run instead of one for every launch; `true` unless `lab.json` says otherwise.
    pub one_eden: bool,
    /// Settings written over the emulator's `qt-config.ini`, keyed `Section/key`.
    pub eden_settings: BTreeMap<String, String>,
    /// Language of `report.html`.
    pub lang: Lang,
    /// Seconds a game may take to its first frame.
    pub timeout: u32,
    /// Every game in file order, enabled or not.
    pub games: Vec<Game>,
    /// Named sets of game names.
    pub sets: BTreeMap<String, Vec<String>>,
}

/// A game of the lab.
#[derive(Debug, Clone, PartialEq)]
pub struct Game {
    /// Its name in the report and the name of its folder in a run.
    pub name: String,
    /// The game folder on this PC.
    pub folder: PathBuf,
    /// The game runs when no set is asked for.
    pub enabled: bool,
    /// The `.exe` relative to the folder; `None` means the one the build wizard would pick.
    pub executable: Option<String>,
    /// The title; `None` means the one the build wizard would show.
    pub title: Option<String>,
    /// Bitness chosen by hand.
    pub arch: Option<Arch>,
    /// Launch arguments.
    pub arguments: Vec<String>,
    /// The defaults with the game's own settings merged over them.
    pub settings: AutorunSettings,
    /// Its own scenario; `None` means [`Scenario::standard`].
    pub scenario: Option<Scenario>,
    /// The file the scenario was read from, relative to `lab.json`; `None` when it is written in `lab.json` or the
    /// game has none.
    pub scenario_file: Option<String>,
    /// Words to find and group the game by, such as `dx9` or `slow-start`.
    pub tags: Vec<String>,
    /// Free notes about the game.
    pub notes: Option<String>,
    /// The percentage the game may get slower by against the last Carafe version before it fails.
    pub threshold: u32,
}

impl Config {
    /// Reads `lab.json`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Missing`] if there is no such file; another [`ConfigError`] if it is broken.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ConfigError::Missing(path.display().to_string()));
            }
            Err(error) => {
                return Err(ConfigError::Io(
                    path.display().to_string(),
                    error.to_string(),
                ));
            }
        };
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Self::parse_with(&text, |file| {
            fs::read_to_string(base.join(file)).map_err(|error| error.to_string())
        })
    }

    /// Reads the text of `lab.json` whose games' scenarios are all written in it.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] if the JSON is broken, has unknown fields, or a game, set or value is wrong; a scenario given
    /// as a file is [`ConfigError::ScenarioFile`].
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        Self::parse_with(text, |_| {
            Err("scenario files are read only from a lab file on disk".to_owned())
        })
    }

    /// Reads the text of `lab.json`; `read` returns the text of a scenario file named relative to the lab file.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] if the JSON is broken, has unknown fields, or a game, set, value or scenario file is wrong.
    pub fn parse_with(
        text: &str,
        read: impl Fn(&str) -> Result<String, String>,
    ) -> Result<Self, ConfigError> {
        let raw: RawConfig =
            serde_json::from_str(text).map_err(|error| ConfigError::Format(error.to_string()))?;
        let gpu = match raw.gpu.as_deref() {
            None => Gpu::Wsl,
            Some(text) => Gpu::parse(text).ok_or(ConfigError::Value("gpu", "wsl or software"))?,
        };
        let emulator_cpu = match raw.emulator_cpu.as_deref() {
            None => EdenCpu::Dynarmic,
            Some(text) => {
                EdenCpu::parse(text).ok_or(ConfigError::Value("emulatorCpu", "dynarmic or nce"))?
            }
        };
        for (path, value) in &raw.eden_settings {
            let valid_path = path
                .split_once('/')
                .is_some_and(|(section, key)| !section.is_empty() && !key.is_empty());
            if !valid_path || value.contains(['\n', '\r']) {
                return Err(ConfigError::Value(
                    "edenSettings",
                    "\"Section/key\": \"value\" on one line",
                ));
            }
        }
        let lang = match raw.lang.as_deref() {
            None => Lang::Ru,
            Some(text) => Lang::parse(text).ok_or(ConfigError::Value("lang", "ru or en"))?,
        };
        let timeout = raw.timeout.unwrap_or(600);
        if timeout == 0 {
            return Err(ConfigError::Value("timeout", "seconds above zero"));
        }
        let mut games: Vec<Game> = Vec::with_capacity(raw.games.len());
        for game in raw.games {
            if !valid_name(&game.name) {
                return Err(ConfigError::Name(game.name));
            }
            if games
                .iter()
                .any(|seen| seen.name.eq_ignore_ascii_case(&game.name))
            {
                return Err(ConfigError::DuplicateName(game.name));
            }
            let scenario_file = match &game.scenario {
                Some(RawScenario::File(file)) => Some(file.clone()),
                _ => None,
            };
            let scenario = match game.scenario {
                None => None,
                Some(RawScenario::Steps(scenario)) => Some(scenario),
                Some(RawScenario::File(file)) => {
                    let text = read(&file).map_err(|message| {
                        ConfigError::ScenarioFile(game.name.clone(), file.clone(), message)
                    })?;
                    let scenario: Scenario = serde_json::from_str(&text).map_err(|error| {
                        ConfigError::ScenarioFile(
                            game.name.clone(),
                            file.clone(),
                            error.to_string(),
                        )
                    })?;
                    Some(scenario)
                }
            };
            if let Some(scenario) = &scenario {
                scenario
                    .check()
                    .map_err(|error| ConfigError::Scenario(game.name.clone(), error))?;
            }
            let settings = merged_settings(game.settings.as_ref())
                .map_err(|message| ConfigError::Settings(game.name.clone(), message))?;
            games.push(Game {
                name: game.name,
                folder: game.folder,
                enabled: game.enabled,
                executable: game.executable,
                title: game.title,
                arch: game.arch,
                arguments: game.arguments,
                settings,
                scenario,
                scenario_file,
                tags: game.tags,
                notes: game.notes,
                threshold: game.threshold.unwrap_or(versions::DEFAULT_THRESHOLD),
            });
        }
        for (set, names) in &raw.sets {
            for name in names {
                if !games
                    .iter()
                    .any(|game| game.name.eq_ignore_ascii_case(name))
                {
                    return Err(ConfigError::UnknownInSet(set.clone(), name.clone()));
                }
            }
        }
        Ok(Self {
            work: raw.work,
            gpu,
            emulator_cpu,
            carafe: raw.carafe,
            eden_image: raw.eden_image,
            one_eden: raw.one_eden.unwrap_or(true),
            eden_settings: raw.eden_settings,
            lang,
            timeout,
            games,
            sets: raw.sets,
        })
    }

    /// Returns the games of a run: the set's games in the set's order, or every enabled game without a set.
    ///
    /// A set runs its games even when they are disabled.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnknownSet`] if `set` is not in `sets`.
    pub fn select(&self, set: Option<&str>) -> Result<Vec<&Game>, ConfigError> {
        let Some(set) = set else {
            return Ok(self.games.iter().filter(|game| game.enabled).collect());
        };
        let names = self
            .sets
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(set))
            .map(|(_, names)| names)
            .ok_or_else(|| ConfigError::UnknownSet(set.to_owned()))?;
        Ok(names
            .iter()
            .filter_map(|name| {
                self.games
                    .iter()
                    .find(|game| game.name.eq_ignore_ascii_case(name))
            })
            .collect())
    }

    /// Returns the games named in `names`, in that order, enabled or not; names are matched ignoring case.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnknownGame`] for the first name that is not in `games`.
    pub fn pick(&self, names: &[String]) -> Result<Vec<&Game>, ConfigError> {
        names
            .iter()
            .map(|name| {
                self.games
                    .iter()
                    .find(|game| game.name.eq_ignore_ascii_case(name))
                    .ok_or_else(|| ConfigError::UnknownGame(name.clone()))
            })
            .collect()
    }

    /// Returns the Carafe checkout the lab packs with: `carafe` as it is when absolute, relative to `repository`
    /// otherwise, or the `external/carafe` submodule of `repository`.
    #[must_use]
    pub fn carafe_checkout(&self, repository: &Path) -> PathBuf {
        match &self.carafe {
            Some(path) if path.is_absolute() => tidy(path),
            Some(path) => tidy(&repository.join(path)),
            None => tidy(&repository.join("external").join("carafe")),
        }
    }
}

/// Returns `path` with its `.` and `..` parts resolved by name, without asking the file system.
#[must_use]
pub fn tidy(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut tidied = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !tidied.pop() {
                    tidied.push(part);
                }
            }
            other => tidied.push(other),
        }
    }
    tidied
}

impl Game {
    /// Returns the entropy the build plan gets for the game: a hash of its name, case ignored.
    #[must_use]
    pub fn entropy(&self) -> u64 {
        name_entropy(&self.name)
    }

    /// Returns the Title ID the game always gets on the lab, so the emulator finds its data again.
    #[must_use]
    pub fn title_id(&self) -> TitleId {
        TitleId::from_entropy(self.entropy())
    }

    /// Returns the scenario of the game.
    #[must_use]
    pub fn scenario(&self, first_frame_timeout: u32) -> Scenario {
        self.scenario
            .clone()
            .unwrap_or_else(|| Scenario::standard(first_frame_timeout))
    }

    /// Composes the build request the way the build wizard would, from the folder scan.
    ///
    /// # Returns
    ///
    /// `None` if the folder has no game `.exe` and the game names none.
    #[must_use]
    pub fn request(&self, report: &FolderReport) -> Option<BuildRequest> {
        let draft = wizard::draft(report);
        let executable = self.executable.clone().or(draft.selected)?;
        let arch = report
            .executables
            .iter()
            .find(|info| same_path(&info.path, &executable))
            .map_or(Arch::X86, |info| info.arch);
        let title = self.title.clone().unwrap_or(draft.metadata.title);
        Some(BuildRequest {
            title_id: None,
            source: GameSource {
                folder: self.folder.to_string_lossy().into_owned(),
                executable,
                arch,
                arch_override: self.arch,
                arguments: self.arguments.clone(),
            },
            metadata: Metadata::new(title),
            settings: self.settings.clone(),
            icon: None,
        })
    }
}

/// Returns the build entropy for a name: FNV-1a of its bytes, case ignored.
#[must_use]
pub fn name_entropy(name: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Merges `overrides` over the default settings: objects field by field, everything else replaced whole.
///
/// # Errors
///
/// A message if the result is not valid settings.
pub fn merged_settings(overrides: Option<&Value>) -> Result<AutorunSettings, String> {
    let Some(overrides) = overrides else {
        return Ok(AutorunSettings::default());
    };
    let mut value =
        serde_json::to_value(AutorunSettings::default()).map_err(|error| error.to_string())?;
    merge(&mut value, overrides);
    let settings: AutorunSettings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.validate().map_err(|error| error.to_string())?;
    Ok(settings)
}

/// Merges `overrides` over `base`, the way a game's own settings are merged over the defaults.
///
/// # Errors
///
/// A message if the result is not valid settings.
pub fn settings_over(base: &AutorunSettings, overrides: &Value) -> Result<AutorunSettings, String> {
    let mut value = serde_json::to_value(base).map_err(|error| error.to_string())?;
    merge(&mut value, overrides);
    let settings: AutorunSettings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.validate().map_err(|error| error.to_string())?;
    Ok(settings)
}

fn merge(base: &mut Value, overrides: &Value) {
    match (base, overrides) {
        (Value::Object(base), Value::Object(overrides)) => {
            for (key, value) in overrides {
                match base.get_mut(key) {
                    Some(slot) => merge(slot, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (slot, value) => *slot = value.clone(),
    }
}

/// Returns a game name for `folder` that `lab.json` accepts and no game in `taken` has, ignoring case: the folder's
/// name with every other character turned into a space, followed by a number when the name is taken.
#[must_use]
pub fn game_name_for(folder: &Path, taken: &[String]) -> String {
    let raw = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let base = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let base = if base.is_empty() {
        "Game".to_owned()
    } else {
        base
    };
    let free = |name: &str| !taken.iter().any(|seen| seen.eq_ignore_ascii_case(name));
    if free(&base) {
        return base;
    }
    (2..)
        .map(|number| format!("{base} {number}"))
        .find(|name| free(name))
        .unwrap_or(base)
}

/// Returns the text of `lab.json` with the game `name` in `folder` added at the end of `games`; everything else keeps
/// its order. `read` reads scenario files, as for [`Config::parse_with`].
///
/// # Errors
///
/// [`ConfigError`] when the text or the result is not a valid lab file, such as a name that is taken.
pub fn add_game(
    text: &str,
    name: &str,
    folder: &Path,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<String, ConfigError> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| ConfigError::Format(error.to_string()))?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| ConfigError::Format("the lab file is not an object".to_owned()))?;
    let games = root
        .entry("games")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| ConfigError::Format("games is not a list".to_owned()))?;
    let mut game = serde_json::Map::new();
    game.insert("name".to_owned(), Value::String(name.to_owned()));
    game.insert(
        "folder".to_owned(),
        Value::String(folder.display().to_string()),
    );
    games.push(Value::Object(game));
    let updated = format!(
        "{}\n",
        serde_json::to_string_pretty(&value)
            .map_err(|error| ConfigError::Format(error.to_string()))?
    );
    Config::parse_with(&updated, read)?;
    Ok(updated)
}

/// Returns the text of `lab.json` with the game `name` reading its scenario from `file`; everything else keeps its
/// order. `read` reads scenario files, as for [`Config::parse_with`], and must already find `file`.
///
/// # Errors
///
/// [`ConfigError`] when the text is not a valid lab file, has no game `name`, or the scenario file is not valid.
pub fn set_scenario_file(
    text: &str,
    name: &str,
    file: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<String, ConfigError> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| ConfigError::Format(error.to_string()))?;
    let game = value
        .get_mut("games")
        .and_then(Value::as_array_mut)
        .and_then(|games| {
            games.iter_mut().find(|game| {
                game.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|known| known.eq_ignore_ascii_case(name))
            })
        })
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ConfigError::UnknownGame(name.to_owned()))?;
    game.insert("scenario".to_owned(), Value::String(file.to_owned()));
    let updated = format!(
        "{}\n",
        serde_json::to_string_pretty(&value)
            .map_err(|error| ConfigError::Format(error.to_string()))?
    );
    Config::parse_with(&updated, read)?;
    Ok(updated)
}

/// Returns the text of `lab.json` with the game `name`'s tags and notes replaced; empty ones are removed, everything
/// else keeps its order.
///
/// # Errors
///
/// [`ConfigError`] when the text is not a valid lab file or has no game `name`.
pub fn set_game_notes(
    text: &str,
    name: &str,
    tags: &[String],
    notes: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<String, ConfigError> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| ConfigError::Format(error.to_string()))?;
    let game = value
        .get_mut("games")
        .and_then(Value::as_array_mut)
        .and_then(|games| {
            games.iter_mut().find(|game| {
                game.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|known| known.eq_ignore_ascii_case(name))
            })
        })
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ConfigError::UnknownGame(name.to_owned()))?;
    let tags: Vec<Value> = tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(|tag| Value::String(tag.to_owned()))
        .collect();
    if tags.is_empty() {
        game.remove("tags");
    } else {
        game.insert("tags".to_owned(), Value::Array(tags));
    }
    if notes.trim().is_empty() {
        game.remove("notes");
    } else {
        game.insert("notes".to_owned(), Value::String(notes.trim().to_owned()));
    }
    let updated = format!(
        "{}\n",
        serde_json::to_string_pretty(&value)
            .map_err(|error| ConfigError::Format(error.to_string()))?
    );
    Config::parse_with(&updated, read)?;
    Ok(updated)
}

/// Returns the scenario file a game named `name` gets when its scenario is first saved on its own:
/// `scenarios/<name>.json` with the name in lower case and every character other than a letter or digit as `-`.
#[must_use]
pub fn scenario_file_for(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "scenarios/{}.json",
        if stem.is_empty() { "game" } else { &stem }
    )
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
}

fn same_path(a: &str, b: &str) -> bool {
    a.replace('\\', "/")
        .eq_ignore_ascii_case(&b.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use carafe_core::ports::ExecutableInfo;
    use carafe_core::settings::{CpuEmulator, InputMode};
    use serde_json::json;

    use super::*;

    const EXAMPLE: &str = include_str!("../../../../lab.example.json");

    fn report(paths: &[(&str, Arch, u64)]) -> FolderReport {
        FolderReport {
            folder: "D:\\Games\\OpenTTD".to_owned(),
            executables: paths
                .iter()
                .map(|&(path, arch, size_bytes)| ExecutableInfo {
                    path: path.to_owned(),
                    arch,
                    fixed_address: false,
                    size_bytes,
                    product_name: Some("OpenTTD".to_owned()),
                    company_name: None,
                })
                .collect(),
            has_steam_api: false,
        }
    }

    fn config(games: &str, sets: &str) -> Result<Config, ConfigError> {
        Config::parse(&format!(
            r#"{{"work": "D:\\Bench", "games": [{games}], "sets": {{{sets}}}}}"#
        ))
    }

    #[test]
    fn the_template_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let config = Config::parse_with(EXAMPLE, |file| {
            fs::read_to_string(root.join(file)).map_err(|error| error.to_string())
        })
        .unwrap();
        assert!(
            config
                .games
                .iter()
                .any(|game| game.name == "NFSU race" && game.scenario.is_some())
        );
        assert!(!config.games.is_empty());
        assert_eq!(config.gpu, Gpu::Wsl);
    }

    #[test]
    fn defaults_fill_what_is_left_out() {
        let config = config(r#"{"name": "OpenTTD", "folder": "D:\\Games\\OpenTTD"}"#, "").unwrap();
        assert_eq!(config.lang, Lang::Ru);
        assert_eq!(config.timeout, 600);
        let game = &config.games[0];
        assert!(game.enabled);
        assert_eq!(game.settings, AutorunSettings::default());
        assert_eq!(game.scenario(600), Scenario::standard(600));
    }

    #[test]
    fn settings_merge_over_the_defaults() {
        let settings = merged_settings(Some(
            &json!({"system": {"cpu": "box64"}, "input": {"mode": "keyboardMouse"}}),
        ))
        .unwrap();
        assert_eq!(settings.system.cpu, CpuEmulator::Box64);
        assert_eq!(settings.input.mode, InputMode::KeyboardMouse);
        assert_eq!(settings.graphics, AutorunSettings::default().graphics);
        assert_eq!(settings.input.keys.len(), 24);
    }

    #[test]
    fn invalid_settings_name_their_game() {
        let sarek = r#"{"name": "A", "folder": "x", "settings": {"graphics": {"direct3d": "dxvkVkd3d", "dxvkSource": "sarek"}}}"#;
        assert!(matches!(config(sarek, ""), Err(ConfigError::Settings(name, _)) if name == "A"));
        let unknown = r#"{"name": "A", "folder": "x", "settings": {"system": {"cpu": "qemu"}}}"#;
        assert!(matches!(
            config(unknown, ""),
            Err(ConfigError::Settings(..))
        ));
    }

    #[test]
    fn broken_games_are_refused() {
        let twice = r#"{"name": "OpenTTD", "folder": "a"}, {"name": "openttd", "folder": "b"}"#;
        assert_eq!(
            config(twice, ""),
            Err(ConfigError::DuplicateName("openttd".to_owned()))
        );
        let slash = r#"{"name": "a/b", "folder": "a"}"#;
        assert_eq!(config(slash, ""), Err(ConfigError::Name("a/b".to_owned())));
        let unknown = r#"{"name": "A", "folder": "a", "exe": "a.exe"}"#;
        assert!(matches!(config(unknown, ""), Err(ConfigError::Format(_))));
        let scenario = r#"{"name": "A", "folder": "a", "scenario": {"steps": [{"step": "press", "buttons": ["A"]}]}}"#;
        assert!(matches!(
            config(scenario, ""),
            Err(ConfigError::Scenario(..))
        ));
    }

    #[test]
    fn games_are_picked_by_name_in_the_order_asked() {
        let config = Config::parse(
            r#"{"work": "x", "games": [{"name": "A", "folder": "a"}, {"name": "B", "folder": "b", "enabled": false}]}"#,
        )
        .unwrap();
        let picked = config.pick(&["b".to_owned(), "A".to_owned()]).unwrap();
        let names: Vec<&str> = picked.iter().map(|game| game.name.as_str()).collect();
        assert_eq!(names, ["B", "A"]);
        assert_eq!(
            config.pick(&["C".to_owned()]),
            Err(ConfigError::UnknownGame("C".to_owned()))
        );
    }

    #[test]
    fn a_saved_scenario_moves_into_its_own_file() {
        assert_eq!(scenario_file_for("NFSU race"), "scenarios/nfsu-race.json");
        assert_eq!(scenario_file_for("Игра"), "scenarios/game.json");
        let text = r#"{"work": "x", "games": [{"name": "A", "folder": "a", "enabled": false}]}"#;
        let steps = r#"{"steps": [{"step": "launch"}]}"#;
        let updated = set_scenario_file(text, "a", "scenarios/a.json", |file| {
            assert_eq!(file, "scenarios/a.json");
            Ok(steps.to_owned())
        })
        .unwrap();
        assert!(updated.find("\"enabled\"").unwrap() < updated.find("\"scenario\"").unwrap());
        let config = Config::parse_with(&updated, |_| Ok(steps.to_owned())).unwrap();
        assert_eq!(
            config.games[0].scenario_file.as_deref(),
            Some("scenarios/a.json")
        );
        assert!(
            set_scenario_file(text, "B", "scenarios/b.json", |_| Ok(steps.to_owned())).is_err()
        );
    }

    #[test]
    fn tags_and_notes_are_kept_and_empty_ones_removed() {
        let text = r#"{"work": "x", "games": [{"name": "A", "folder": "a"}]}"#;
        let tagged = set_game_notes(
            text,
            "a",
            &["dx9".to_owned(), " ".to_owned()],
            "stalls at start",
            |_| Err("none".to_owned()),
        )
        .unwrap();
        let config = Config::parse(&tagged).unwrap();
        assert_eq!(config.games[0].tags, ["dx9"]);
        assert_eq!(config.games[0].notes.as_deref(), Some("stalls at start"));
        let cleared = set_game_notes(&tagged, "A", &[], "", |_| Err("none".to_owned())).unwrap();
        assert!(!cleared.contains("tags") && !cleared.contains("notes"));
    }

    #[test]
    fn a_dropped_folder_gets_a_free_name() {
        let taken = vec!["OpenTTD".to_owned(), "Game".to_owned()];
        assert_eq!(
            game_name_for(Path::new("D:\\Games\\Need for Speed: U2 (GOG)"), &taken),
            "Need for Speed U2 GOG"
        );
        assert_eq!(
            game_name_for(Path::new("D:\\Games\\openttd"), &taken),
            "openttd 2"
        );
        assert_eq!(game_name_for(Path::new("D:\\Games\\!!!"), &taken), "Game 2");
    }

    #[test]
    fn a_game_is_added_at_the_end_and_the_rest_keeps_its_order() {
        let text = r#"{"work": "W:\\lab", "gpu": "wsl", "games": [{"name": "A", "folder": "D:\\A"}], "sets": {}}"#;
        let updated = add_game(text, "B", Path::new("D:\\B"), |_| Err("none".to_owned())).unwrap();
        let config = Config::parse(&updated).unwrap();
        let names: Vec<&str> = config.games.iter().map(|game| game.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
        assert!(updated.find("\"work\"").unwrap() < updated.find("\"games\"").unwrap());
        assert!(add_game(text, "a", Path::new("D:\\A2"), |_| Err("none".to_owned())).is_err());
    }

    #[test]
    fn the_carafe_checkout_is_found_beside_the_repository() {
        let repository = Path::new("D:\\Lab\\app\\crates\\lab-cli\\..\\..\\..");
        let beside = Config::parse(r#"{"work": "x", "carafe": "../carafe"}"#).unwrap();
        assert_eq!(
            beside.carafe_checkout(repository),
            PathBuf::from("D:\\carafe")
        );
        let submodule = Config::parse(r#"{"work": "x"}"#).unwrap();
        assert_eq!(
            submodule.carafe_checkout(repository),
            PathBuf::from("D:\\Lab\\external\\carafe")
        );
    }

    #[test]
    fn a_scenario_can_live_in_its_own_file() {
        let text = r#"{"work": "x", "games": [{"name": "A", "folder": "a", "scenario": "scenarios/a.json"}]}"#;
        let config = Config::parse_with(text, |file| {
            assert_eq!(file, "scenarios/a.json");
            Ok(r#"{"steps": [{"step": "launch"}, {"step": "wait", "seconds": 1}]}"#.to_owned())
        })
        .unwrap();
        assert!(config.games[0].scenario.is_some());
        assert!(matches!(
            Config::parse(text),
            Err(ConfigError::ScenarioFile(name, file, _)) if name == "A" && file == "scenarios/a.json"
        ));
        let broken = Config::parse_with(text, |_| Ok("{\"steps\": 1}".to_owned()));
        assert!(matches!(broken, Err(ConfigError::ScenarioFile(..))));
    }

    #[test]
    fn eden_image_and_settings_are_read_and_checked() {
        let config = Config::parse(
            r#"{"work": "x", "edenImage": "carafe-eden:diag", "edenSettings": {"Renderer/gpu_accuracy": "0"}}"#,
        )
        .unwrap();
        assert_eq!(config.eden_image.as_deref(), Some("carafe-eden:diag"));
        assert_eq!(
            config
                .eden_settings
                .get("Renderer/gpu_accuracy")
                .map(String::as_str),
            Some("0")
        );
        let plain = Config::parse(r#"{"work": "x"}"#).unwrap();
        assert_eq!(plain.eden_image, None);
        assert!(plain.eden_settings.is_empty());
        assert!(plain.one_eden);
        let per_launch = Config::parse(r#"{"work": "x", "oneEden": false}"#).unwrap();
        assert!(!per_launch.one_eden);
        for bad in [
            r#"{"work": "x", "edenSettings": {"gpu_accuracy": "0"}}"#,
            r#"{"work": "x", "edenSettings": {"/gpu_accuracy": "0"}}"#,
            r#"{"work": "x", "edenSettings": {"Renderer/": "0"}}"#,
            r#"{"work": "x", "edenSettings": {"Renderer/backend": "0\n[UI]"}}"#,
        ] {
            assert_eq!(
                Config::parse(bad),
                Err(ConfigError::Value(
                    "edenSettings",
                    "\"Section/key\": \"value\" on one line"
                ))
            );
        }
    }

    #[test]
    fn values_are_checked() {
        let gpu = Config::parse(r#"{"work": "x", "gpu": "amd"}"#);
        assert_eq!(gpu, Err(ConfigError::Value("gpu", "wsl or software")));
        let cpu = Config::parse(r#"{"work": "x", "emulatorCpu": "jit"}"#);
        assert_eq!(
            cpu,
            Err(ConfigError::Value("emulatorCpu", "dynarmic or nce"))
        );
        let nce = Config::parse(r#"{"work": "x", "emulatorCpu": "nce"}"#).unwrap();
        assert_eq!(nce.emulator_cpu, EdenCpu::Nce);
        assert_eq!(
            Config::parse(r#"{"work": "x"}"#).unwrap().emulator_cpu,
            EdenCpu::Dynarmic
        );
        let zero = Config::parse(r#"{"work": "x", "timeout": 0}"#);
        assert_eq!(
            zero,
            Err(ConfigError::Value("timeout", "seconds above zero"))
        );
    }

    #[test]
    fn sets_name_known_games() {
        let games = r#"{"name": "A", "folder": "a"}"#;
        assert_eq!(
            config(games, r#""quick": ["B"]"#),
            Err(ConfigError::UnknownInSet(
                "quick".to_owned(),
                "B".to_owned()
            ))
        );
    }

    #[test]
    fn selection_follows_sets_and_enabled() {
        let games = r#"{"name": "A", "folder": "a"}, {"name": "B", "folder": "b", "enabled": false}, {"name": "C", "folder": "c"}"#;
        let config = config(games, r#""slow": ["B", "a"]"#).unwrap();
        let names = |set: Option<&str>| -> Vec<String> {
            config
                .select(set)
                .unwrap()
                .iter()
                .map(|game| game.name.clone())
                .collect()
        };
        assert_eq!(names(None), vec!["A", "C"]);
        assert_eq!(names(Some("SLOW")), vec!["B", "A"]);
        assert_eq!(
            config.select(Some("nope")),
            Err(ConfigError::UnknownSet("nope".to_owned()))
        );
    }

    #[test]
    fn the_title_id_follows_the_name() {
        let config = config(
            r#"{"name": "OpenTTD", "folder": "a"}, {"name": "Half-Life", "folder": "b"}"#,
            "",
        )
        .unwrap();
        assert_eq!(
            config.games[0].title_id(),
            TitleId::from_entropy(name_entropy("openttd"))
        );
        assert_ne!(config.games[0].title_id(), config.games[1].title_id());
    }

    #[test]
    fn the_request_follows_the_wizard() {
        let config = config(r#"{"name": "OpenTTD", "folder": "D:\\Games\\OpenTTD"}"#, "").unwrap();
        let folder = report(&[
            ("unins000.exe", Arch::X86, 50),
            ("openttd.exe", Arch::X86, 10),
        ]);
        let request = config.games[0].request(&folder).unwrap();
        assert_eq!(request.source.executable, "openttd.exe");
        assert_eq!(request.metadata.title, "OpenTTD");
        assert_eq!(request.source.folder, "D:\\Games\\OpenTTD");
    }

    #[test]
    fn the_game_overrides_the_wizard() {
        let game = r#"{"name": "Game", "folder": "g", "executable": "bin/game64.exe", "title": "Game 64", "arch": "x64", "arguments": ["-windowed"]}"#;
        let config = config(game, "").unwrap();
        let folder = report(&[
            ("game.exe", Arch::X86, 50),
            ("bin\\game64.exe", Arch::X64, 10),
        ]);
        let request = config.games[0].request(&folder).unwrap();
        assert_eq!(request.source.executable, "bin/game64.exe");
        assert_eq!(request.source.arch, Arch::X64);
        assert_eq!(request.source.arch_override, Some(Arch::X64));
        assert_eq!(request.source.arguments, vec!["-windowed".to_owned()]);
        assert_eq!(request.metadata.title, "Game 64");
    }

    #[test]
    fn no_request_without_a_game_exe() {
        let config = config(r#"{"name": "Setup", "folder": "s"}"#, "").unwrap();
        assert_eq!(
            config.games[0].request(&report(&[("setup.exe", Arch::X86, 50)])),
            None
        );
    }

    #[test]
    fn a_missing_file_says_how_to_make_it() {
        let error = Config::load(Path::new("Z:\\nowhere\\lab.json")).unwrap_err();
        assert!(matches!(error, ConfigError::Missing(_)));
        assert!(error.to_string().contains(TEMPLATE_FILE));
    }
}
