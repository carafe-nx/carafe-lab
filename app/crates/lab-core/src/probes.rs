//! The probe runs: which probe program and bitness each combination of settings is tried with.

use carafe_core::record::Arch;
use carafe_core::settings::{AutorunSettings, Binding, Direct3d, InputMode, SwitchButton};

use crate::matrix::Combination;
use crate::scenario::Scenario;

/// A probe program, built as `probe-<name>.exe` for i686 and x86_64 by `tools/probes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Logs keys, mouse and the Xbox controller.
    Input,
    /// Draws the pattern with GDI.
    Gdi,
    /// Draws the pattern with Direct3D 9.
    D3d9,
    /// Draws the pattern with Direct3D 11.
    D3d11,
    /// Draws the pattern with Direct3D 12.
    D3d12,
    /// Draws the pattern with OpenGL.
    Gl,
    /// Switches DirectDraw to 1024×768 full screen and logs the mouse moves in its window.
    Screen,
}

impl Probe {
    /// Returns the name in `probe-<name>.exe`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Gdi => "gdi",
            Self::D3d9 => "d3d9",
            Self::D3d11 => "d3d11",
            Self::D3d12 => "d3d12",
            Self::Gl => "gl",
            Self::Screen => "screen",
        }
    }
}

/// One run of a probe.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeRun {
    /// Name of the game folder the lab makes for it, such as `probe-07-d3d11-x64`.
    pub name: String,
    /// The program.
    pub probe: Probe,
    /// Its bitness.
    pub arch: Arch,
    /// The settings it is built with.
    pub settings: AutorunSettings,
    /// A short label for the report.
    pub label: String,
    /// What the lab does with it.
    pub scenario: Scenario,
}

/// Assigns a graphics probe and a bitness to every combination.
///
/// The probe follows the combination's Direct3D choice, so every API meets every setting it goes through:
/// DXVK takes Direct3D 11, 9, OpenGL and GDI in turn; DXVK with VKD3D takes Direct3D 12 and 11; WineD3D takes
/// Direct3D 9, 11, OpenGL and GDI. Bitness alternates.
#[must_use]
pub fn graphics_runs(combinations: &[Combination], first_frame_timeout: u32) -> Vec<ProbeRun> {
    combinations
        .iter()
        .enumerate()
        .map(|(index, combination)| {
            let candidates: &[Probe] = match combination.settings.graphics.direct3d {
                Direct3d::Dxvk => &[Probe::D3d11, Probe::D3d9, Probe::Gl, Probe::Gdi],
                Direct3d::DxvkVkd3d => &[Probe::D3d12, Probe::D3d11],
                Direct3d::Wine => &[Probe::D3d9, Probe::D3d11, Probe::Gl, Probe::Gdi],
            };
            let probe = candidates[index % candidates.len()];
            let arch = if index % 2 == 0 { Arch::X64 } else { Arch::X86 };
            ProbeRun {
                name: format!(
                    "probe-{:02}-{}-{}",
                    index + 1,
                    probe.name(),
                    arch_name(arch)
                ),
                probe,
                arch,
                settings: combination.settings.clone(),
                label: format!(
                    "{} · {} {}",
                    combination.label,
                    probe.name(),
                    arch_name(arch)
                ),
                scenario: Scenario::graphics_probe(first_frame_timeout),
            }
        })
        .collect()
}

/// Returns the input probe runs: the controller and keyboard modes with the default bindings, and the
/// controller with Switch A and B swapped, which shows whether the `pad.*` bindings reach the game.
#[must_use]
pub fn input_runs(first_frame_timeout: u32) -> Vec<ProbeRun> {
    let controller = AutorunSettings::default();
    let mut keyboard = AutorunSettings::default();
    keyboard.input.mode = InputMode::KeyboardMouse;
    keyboard.input.keys[0] = Binding {
        button: SwitchButton::A,
        codes: vec![0x11, 0x53],
    };
    let mut swapped = AutorunSettings::default();
    swapped.input.pad[0].codes = vec![2];
    swapped.input.pad[1].codes = vec![1];
    let variants = [
        (
            "controller",
            Arch::X64,
            controller,
            "controller, default bindings",
        ),
        (
            "keyboard",
            Arch::X86,
            keyboard,
            "keyboard and mouse, A = Ctrl+S",
        ),
        ("swapped", Arch::X64, swapped, "controller, A and B swapped"),
    ];
    variants
        .into_iter()
        .map(|(name, arch, settings, label)| ProbeRun {
            name: format!("probe-input-{name}-{}", arch_name(arch)),
            probe: Probe::Input,
            arch,
            settings,
            label: format!("{label} · input {}", arch_name(arch)),
            scenario: Scenario::input_probe(first_frame_timeout),
        })
        .collect()
}

/// Returns the screen probe run: a 32-bit program in the keyboard and mouse mode, whose picture is fitted into the
/// console's screen and centred, as for a game at 1024×768.
#[must_use]
pub fn screen_runs(first_frame_timeout: u32) -> Vec<ProbeRun> {
    let mut settings = AutorunSettings::default();
    settings.input.mode = InputMode::KeyboardMouse;
    let arch = Arch::X86;
    vec![ProbeRun {
        name: format!("probe-screen-{}", arch_name(arch)),
        probe: Probe::Screen,
        arch,
        settings,
        label: format!(
            "1024×768, mouse on the right stick · screen {}",
            arch_name(arch)
        ),
        scenario: Scenario::screen_probe(first_frame_timeout),
    }]
}

/// Returns the folder name of a bitness in the probe builds: `i686` or `x86_64`.
#[must_use]
pub fn build_dir(arch: Arch) -> &'static str {
    match arch {
        Arch::X86 => "i686",
        Arch::X64 => "x86_64",
    }
}

fn arch_name(arch: Arch) -> &'static str {
    match arch {
        Arch::X86 => "x86",
        Arch::X64 => "x64",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix;

    #[test]
    fn d3d12_runs_only_with_vkd3d() {
        for run in graphics_runs(&matrix::pairwise(), 600) {
            if run.probe == Probe::D3d12 {
                assert_eq!(
                    run.settings.graphics.direct3d,
                    Direct3d::DxvkVkd3d,
                    "{}",
                    run.label
                );
            }
        }
    }

    #[test]
    fn every_graphics_api_and_both_bitnesses_are_tried() {
        let runs = graphics_runs(&matrix::pairwise(), 600);
        for probe in [
            Probe::Gdi,
            Probe::D3d9,
            Probe::D3d11,
            Probe::D3d12,
            Probe::Gl,
        ] {
            assert!(
                runs.iter().any(|run| run.probe == probe),
                "{}",
                probe.name()
            );
        }
        assert!(runs.iter().any(|run| run.arch == Arch::X86));
        assert!(runs.iter().any(|run| run.arch == Arch::X64));
    }

    #[test]
    fn run_names_are_unique_folder_names() {
        let mut names: Vec<String> = graphics_runs(&matrix::pairwise(), 600)
            .into_iter()
            .chain(input_runs(600))
            .chain(screen_runs(600))
            .map(|run| run.name)
            .collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(
            names
                .iter()
                .all(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        );
    }

    #[test]
    fn input_variants_are_valid_settings() {
        let runs = input_runs(600);
        assert_eq!(runs.len(), 3);
        for run in &runs {
            assert_eq!(run.settings.validate(), Ok(()), "{}", run.label);
            assert_eq!(run.probe, Probe::Input);
        }
        assert_eq!(runs[2].settings.input.pad[0].codes, vec![2]);
    }

    #[test]
    fn the_screen_probe_moves_the_mouse_with_the_stick() {
        let runs = screen_runs(600);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].settings.validate(), Ok(()));
        assert_eq!(runs[0].settings.input.mode, InputMode::KeyboardMouse);
        assert_eq!(runs[0].probe, Probe::Screen);
    }
}
