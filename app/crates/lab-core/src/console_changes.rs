//! Settings of an A/B run's second variant written as changes made on the console's settings menu, so both variants
//! run from one NSP.

use carafe_core::autorun_files::AutorunFiles;
use carafe_core::settings::AutorunSettings;

const HEADER: &str = "carafe-changes\t1\n";

/// Returns the text of Carafe's `changes.txt` that turns the settings files of `base` into those of `variant` for
/// the game whose executable is `executable`: one `file\tkey\tvalue` line for every key `variant` sets otherwise
/// and a `-` value for every key it drops. Equal settings give the header alone.
#[must_use]
pub fn console_changes(
    executable: &str,
    base: &AutorunSettings,
    variant: &AutorunSettings,
) -> String {
    let from = AutorunFiles::render(executable, base, &[]);
    let to = AutorunFiles::render(executable, variant, &[]);
    let mut out = HEADER.to_owned();
    for (file, before, after) in [
        ("program", &from.program, &to.program),
        ("keys", &from.keys, &to.keys),
    ] {
        out.push_str(&file_changes(file, before, after));
    }
    out
}

fn file_changes(file: &str, before: &str, after: &str) -> String {
    let before = entries(before);
    let after = entries(after);
    let mut out = String::new();
    for (key, value) in &after {
        if before
            .iter()
            .all(|(other, old)| other != key || old != value)
        {
            out.push_str(&format!("{file}\t{key}\t{value}\n"));
        }
    }
    for (key, _) in &before {
        if after.iter().all(|(other, _)| other != key) {
            out.push_str(&format!("{file}\t{key}\t-\n"));
        }
    }
    out
}

fn entries(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once(" = "))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .filter(|(key, _)| !key.is_empty() && !key.starts_with('#'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_option(id: &str, value: &str) -> AutorunSettings {
        let mut settings = AutorunSettings::default();
        settings.options.set(id, Some(value));
        settings
    }

    #[test]
    fn equal_settings_give_the_header_alone() {
        let settings = AutorunSettings::default();
        assert_eq!(console_changes("game.exe", &settings, &settings), HEADER);
    }

    #[test]
    fn an_option_set_in_the_variant_becomes_one_line() {
        let base = AutorunSettings::default();
        let variant = with_option("dxvk-trackPipelineLifetime", "False");
        assert_eq!(
            console_changes("game.exe", &base, &variant),
            format!("{HEADER}program\tdxvk.trackPipelineLifetime\tFalse\n")
        );
    }

    #[test]
    fn an_option_the_variant_drops_is_removed() {
        let base = with_option("dxvk-trackPipelineLifetime", "False");
        let variant = AutorunSettings::default();
        assert_eq!(
            console_changes("game.exe", &base, &variant),
            format!("{HEADER}program\tdxvk.trackPipelineLifetime\t-\n")
        );
    }

    #[test]
    fn quoted_values_keep_their_quotes() {
        let base = AutorunSettings::default();
        let variant = with_option("dxvk-shaderCompilationMethod", "\"none\"");
        assert_eq!(
            console_changes("game.exe", &base, &variant),
            format!("{HEADER}program\tdxvk.shaderCompilationMethod\t\"none\"\n")
        );
    }
}
