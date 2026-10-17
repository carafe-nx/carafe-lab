//! Diagnostics a run turns on by name instead of with a temporary patch: more logs, counters and probes, each at some
//! cost in speed, so they are on only where they are needed.

use carafe_core::settings::{AutorunSettings, DxvkHud};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A diagnostic a run can turn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Diagnostic {
    /// The runtime's verbose traces, with Wine's standard output and errors.
    Verbose,
    /// The runtime's profiler.
    Profiler,
    /// DXVK's full HUD on the game's picture.
    Hud,
    /// Eden's gdb stub, so the stacks of a hung game's threads are saved before it is stopped.
    Stacks,
    /// Eden's log at the debug level.
    EdenDebug,
}

impl Diagnostic {
    /// Every diagnostic, in the order they are listed.
    pub const ALL: [Self; 5] = [
        Self::Verbose,
        Self::Profiler,
        Self::Hud,
        Self::Stacks,
        Self::EdenDebug,
    ];

    /// Returns its name on the command line and in `lab.json`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Verbose => "verbose",
            Self::Profiler => "profiler",
            Self::Hud => "hud",
            Self::Stacks => "stacks",
            Self::EdenDebug => "eden-debug",
        }
    }

    /// Returns the diagnostic called `name`; `None` for an unknown name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|diagnostic| diagnostic.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Returns whether it changes the game's Carafe settings, and so the NSP, rather than the emulator.
    #[must_use]
    pub fn in_the_nsp(self) -> bool {
        matches!(self, Self::Verbose | Self::Profiler | Self::Hud)
    }

    /// Turns it on in `settings`; does nothing for a diagnostic of the emulator.
    pub fn apply(self, settings: &mut AutorunSettings) {
        match self {
            Self::Verbose => settings.debug.verbose = true,
            Self::Profiler => settings.debug.profile = true,
            Self::Hud => settings.graphics.hud = DxvkHud::Full,
            Self::Stacks | Self::EdenDebug => {}
        }
    }
}

/// Returns the diagnostics named in `text`, separated by commas, sorted and without repeats.
///
/// # Errors
///
/// A message naming the first unknown diagnostic and listing the known ones.
pub fn parse_list(text: &str) -> Result<Vec<Diagnostic>, String> {
    let mut list = text
        .split(',')
        .filter(|name| !name.trim().is_empty())
        .map(|name| {
            Diagnostic::parse(name).ok_or_else(|| {
                let known: Vec<&str> = Diagnostic::ALL.iter().map(|known| known.name()).collect();
                format!("no diagnostic {}; known: {}", name.trim(), known.join(", "))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    list.sort_unstable();
    list.dedup();
    Ok(list)
}

/// Returns the names of `list`, separated by commas.
#[must_use]
pub fn label(list: &[Diagnostic]) -> String {
    list.iter()
        .map(|diagnostic| diagnostic.name())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_back_and_unknown_names_are_refused() {
        assert_eq!(
            parse_list("stacks, Verbose,stacks"),
            Ok(vec![Diagnostic::Verbose, Diagnostic::Stacks])
        );
        assert!(
            parse_list("verbose,trace")
                .unwrap_err()
                .contains("no diagnostic trace")
        );
        for diagnostic in Diagnostic::ALL {
            assert_eq!(Diagnostic::parse(diagnostic.name()), Some(diagnostic));
        }
    }

    #[test]
    fn only_the_nsp_diagnostics_change_the_settings() {
        let mut settings = AutorunSettings::default();
        for diagnostic in Diagnostic::ALL {
            diagnostic.apply(&mut settings);
        }
        assert!(settings.debug.verbose && settings.debug.profile);
        assert_eq!(settings.graphics.hud, DxvkHud::Full);
        assert!(!Diagnostic::Stacks.in_the_nsp() && Diagnostic::Hud.in_the_nsp());
    }
}
