//! Keeping the path of the keys file out of messages, logs and reports.

use std::path::Path;

/// Returns `text` with every spelling of the path `secret` — with `\` or `/`, any letter case — and of its folder
/// replaced by `prod.keys` and `…`, so neither the keys file nor where it lives reaches a log or a report.
#[must_use]
pub fn redact(text: &str, secret: &Path) -> String {
    let file = secret.to_string_lossy().into_owned();
    let folder = secret
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = text.to_owned();
    for (needle, replacement) in [(file, "prod.keys"), (folder, "…")] {
        if needle.len() < 3 {
            continue;
        }
        for spelling in [
            needle.clone(),
            needle.replace('\\', "/"),
            needle.replace('/', "\\"),
        ] {
            out = replace_ignoring_case(&out, &spelling, replacement);
        }
    }
    out
}

fn replace_ignoring_case(text: &str, needle: &str, replacement: &str) -> String {
    let lower_text = text.to_lowercase();
    let lower_needle = needle.to_lowercase();
    if lower_text.len() != text.len() || lower_needle.is_empty() {
        return text.replace(needle, replacement);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower_text[at..].find(&lower_needle) {
        out.push_str(&text[at..at + found]);
        out.push_str(replacement);
        at += found + lower_needle.len();
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_path_never_shows() {
        let secret = Path::new("D:\\Backup\\prod.keys");
        let text = "docker: invalid mount D:\\Backup\\prod.keys:/work and d:/backup/PROD.KEYS, folder D:\\Backup";
        let clean = redact(text, secret);
        assert!(!clean.to_lowercase().contains("backup"), "{clean}");
        assert_eq!(
            clean,
            "docker: invalid mount prod.keys:/work and prod.keys, folder …"
        );
    }

    #[test]
    fn redact_leaves_other_text_alone() {
        assert_eq!(
            redact("nothing here", Path::new("D:\\Keys\\prod.keys")),
            "nothing here"
        );
    }
}
