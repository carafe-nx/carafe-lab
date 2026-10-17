//! Carafe's release tags and where the lab keeps the runtime built from each.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use ts_rs::TS;

/// The prefix of a runtime named by its release, such as `release:v0.2.3` or `release:latest`.
pub const PREFIX: &str = "release:";

/// A Carafe release and whether the lab has its runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Release {
    /// Its tag, such as `v0.2.3`.
    pub tag: String,
    /// Its runtime is built and kept.
    pub kept: bool,
}

/// Returns where the runtime of the release `tag` is kept.
#[must_use]
pub fn kept(work: &Path, tag: &str) -> PathBuf {
    work.join("runtimes").join(format!("release-{tag}.bin"))
}

/// Returns the release tags of the Carafe checkout `carafe`, newest first.
#[must_use]
pub fn tags(carafe: &Path) -> Vec<String> {
    Command::new("git")
        .arg("-C")
        .arg(carafe)
        .args(["tag", "--list", "v*", "--sort=-v:refname"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Returns the releases of the Carafe checkout `carafe`, newest first, each with whether its runtime is kept in `work`.
#[must_use]
pub fn list(carafe: &Path, work: &Path) -> Vec<Release> {
    tags(carafe)
        .into_iter()
        .map(|tag| Release {
            kept: kept(work, &tag).is_file(),
            tag,
        })
        .collect()
}

/// Returns the tag `name` stands for: `latest` is the newest release tag.
///
/// # Errors
///
/// A message when the checkout has no such tag.
pub fn tag(carafe: &Path, name: &str) -> Result<String, String> {
    let known = tags(carafe);
    let found = if name == "latest" {
        known.first().cloned()
    } else {
        known.iter().find(|tag| tag.as_str() == name).cloned()
    };
    found.ok_or_else(|| format!("Carafe has no release {name}; it has {}", known.join(", ")))
}
