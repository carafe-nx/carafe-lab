//! What the emulator runs on: the GPU that draws its picture and the way it runs the Switch's processor code.

/// What draws the emulator's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    /// The PC's graphics card through WSL2 and Mesa's Direct3D 12 drivers.
    Wsl,
    /// Mesa's software drivers: slow, but work everywhere.
    Software,
}

impl Gpu {
    /// Reads `wsl` or `software`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "wsl" => Some(Self::Wsl),
            "software" => Some(Self::Software),
            _ => None,
        }
    }

    /// Returns the name in `lab.json`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Wsl => "wsl",
            Self::Software => "software",
        }
    }
}

/// How Eden runs the Switch's processor code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdenCpu {
    /// Translated by Eden's dynarmic recompiler: slower, works on any host.
    Dynarmic,
    /// Run directly on an ARM64 host's processor: faster.
    Nce,
}

impl EdenCpu {
    /// Reads `dynarmic` or `nce`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "dynarmic" => Some(Self::Dynarmic),
            "nce" => Some(Self::Nce),
            _ => None,
        }
    }

    /// Returns the name in `lab.json`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Dynarmic => "dynarmic",
            Self::Nce => "nce",
        }
    }
}
