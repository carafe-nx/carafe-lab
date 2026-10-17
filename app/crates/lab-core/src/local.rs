//! `lab.local.json`: what belongs to this PC only and is chosen in the window, such as the console files Lab packs
//! with; and finding those files when the lab has not chosen them, in Carafe's own settings.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Name of the file next to `lab.json`; git ignores it.
pub const LOCAL_FILE: &str = "lab.local.json";

/// Where Carafe keeps its settings, inside the user's application data folder.
pub const CARAFE_PREFERENCES: &str = "app.carafe.builder/preferences.json";

/// What this PC chose in the window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSettings {
    /// The console files Lab packs with; `None` means the ones chosen in Carafe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<PathBuf>,
    /// The Switch firmware Eden runs with, a folder of `.nca` files; `None` means Eden runs without firmware.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware: Option<PathBuf>,
}

/// Where the console files come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum KeysSource {
    /// Given on the command line.
    Argument,
    /// Chosen in the lab's window.
    Lab,
    /// Chosen in Carafe's settings.
    Carafe,
}

impl LocalSettings {
    /// Reads the settings from `text`.
    ///
    /// # Errors
    ///
    /// The parser's message when the text is not such JSON.
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|error| format!("{LOCAL_FILE}: {error}"))
    }

    /// Reads `file`; a missing file means nothing is chosen.
    ///
    /// # Errors
    ///
    /// A message when the file exists but cannot be read or is broken.
    pub fn load(file: &Path) -> Result<Self, String> {
        match fs::read_to_string(file) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("{LOCAL_FILE}: {error}")),
        }
    }

    /// Writes the settings to `file`.
    ///
    /// # Errors
    ///
    /// A message when the file cannot be written.
    pub fn save(&self, file: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        fs::write(file, format!("{text}\n")).map_err(|error| format!("{LOCAL_FILE}: {error}"))
    }
}

/// Returns the console files chosen in Carafe's settings, read from the text of its `preferences.json`.
#[must_use]
pub fn carafe_keys(preferences: &str) -> Option<PathBuf> {
    let value: serde_json::Value = serde_json::from_str(preferences).ok()?;
    value
        .get("keysPath")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// Returns the console files to pack with and where they come from: `argument` first, then the lab's own choice in
/// `local`, then Carafe's settings under `appdata`.
///
/// Returns `None` when none of them names a file.
#[must_use]
pub fn find_keys(
    argument: Option<&Path>,
    local: &LocalSettings,
    appdata: Option<&Path>,
) -> Option<(PathBuf, KeysSource)> {
    if let Some(path) = argument {
        return Some((path.to_path_buf(), KeysSource::Argument));
    }
    if let Some(path) = &local.keys {
        return Some((path.clone(), KeysSource::Lab));
    }
    let text = fs::read_to_string(appdata?.join(CARAFE_PREFERENCES)).ok()?;
    carafe_keys(&text).map(|path| (path, KeysSource::Carafe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carafe_names_its_files_in_keys_path() {
        let text = r#"{"theme":"system","keysPath":"C:\\Switch\\files.txt"}"#;
        assert_eq!(
            carafe_keys(text),
            Some(PathBuf::from("C:\\Switch\\files.txt"))
        );
        assert_eq!(carafe_keys(r#"{"keysPath":""}"#), None);
        assert_eq!(carafe_keys("not json"), None);
    }

    #[test]
    fn the_lab_choice_comes_before_carafe() {
        let local = LocalSettings {
            keys: Some(PathBuf::from("D:\\lab-files")),
            firmware: None,
        };
        let found = find_keys(None, &local, None);
        assert_eq!(
            found,
            Some((PathBuf::from("D:\\lab-files"), KeysSource::Lab))
        );
        let argument = find_keys(Some(Path::new("E:\\given")), &local, None);
        assert_eq!(
            argument.map(|(_, source)| source),
            Some(KeysSource::Argument)
        );
        assert_eq!(find_keys(None, &LocalSettings::default(), None), None);
    }

    #[test]
    fn nothing_chosen_writes_an_empty_object() {
        let text = serde_json::to_string(&LocalSettings::default()).unwrap();
        assert_eq!(text, "{}");
        assert_eq!(
            LocalSettings::parse("{}").unwrap(),
            LocalSettings::default()
        );
    }
}
