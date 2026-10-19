//! Milestones of one launch, read from the overlay and runtime logs.

use serde::Serialize;
use ts_rs::TS;

const OVERLAY_PREFIX: &str = "[carafe-overlay] ";
const CARAFE_PREFIX: &str = "[CARAFE] ";

/// Why the loading screen went away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "text")]
pub enum HideReason {
    /// The game drew its first picture through OpenGL or the framebuffer.
    Picture,
    /// The game presented Vulkan frames.
    VulkanFrames,
    /// The game showed its windows.
    Windows,
    /// The game opened the on-screen keyboard.
    Keyboard,
    /// The loading screen gave up waiting.
    TimeLimit,
    /// A reason this version of the lab does not know.
    Other(String),
}

impl HideReason {
    fn parse(text: &str) -> Self {
        match text {
            "the game drew its picture" => Self::Picture,
            "Vulkan frames" => Self::VulkanFrames,
            "the game is drawing its windows" => Self::Windows,
            "the program opened the keyboard" => Self::Keyboard,
            "time limit" => Self::TimeLimit,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Returns whether the game reached the screen: every reason except [`HideReason::TimeLimit`].
    #[must_use]
    pub fn is_success(&self) -> bool {
        !matches!(self, Self::TimeLimit)
    }

    /// Returns the reason as the runtime writes it.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Picture => "the game drew its picture",
            Self::VulkanFrames => "Vulkan frames",
            Self::Windows => "the game is drawing its windows",
            Self::Keyboard => "the program opened the keyboard",
            Self::TimeLimit => "time limit",
            Self::Other(text) => text,
        }
    }
}

/// One commit of the device save data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    /// What caused it: `after writes`, `on close request`…
    pub reason: String,
    /// The commit succeeded (`rc=0x00000000`).
    pub ok: bool,
    /// Open files that could not be reopened after it.
    pub lost: u32,
}

/// How the game process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "code")]
pub enum Exit {
    /// Exit code zero.
    Clean,
    /// A non-zero exit code, such as `0xc0000005` for an access violation.
    Code(u32),
}

/// The furthest point a launch reached, in start-up order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Stage {
    /// No log line from the overlay: the loader or the runtime did not start.
    NotStarted,
    /// The overlay started and mounted the game.
    Overlay,
    /// Wine's virtual memory is ready.
    Wine,
    /// The loading screen is on the screen.
    LoadingScreen,
    /// The game reached the screen.
    FirstFrame,
}

/// What one launch did, as far as its logs tell.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    /// The overlay wrote its first line.
    pub overlay_started: bool,
    /// The overlay is installed over the SD card and its commit thread runs.
    pub overlay_installed: bool,
    /// Wine's virtual memory is ready.
    pub wine_ready: bool,
    /// Second of the launch the loading screen was drawn at.
    pub loading_drawn_at: Option<u32>,
    /// Second of the launch the loading screen went away at, and why.
    pub loading_hidden: Option<(u32, HideReason)>,
    /// Times the loading screen reported no progress.
    pub stalls: u32,
    /// Commits of the device save data in log order.
    pub commits: Vec<Commit>,
    /// How the game process ended; `None` if the log does not say.
    pub exit: Option<Exit>,
    /// Why the runtime refused to start the program, from its `[FAIL]` line; it then waits for HOME.
    pub refused: Option<String>,
    /// The overlay shut down in order.
    pub shutdown: bool,
}

impl Launch {
    /// Reads a launch from the overlay log `carafe-overlay.log` and the runtime log `logs/autorun_runtime.log`.
    ///
    /// Lines the loading screen writes into both logs are counted once.
    #[must_use]
    pub fn read(overlay: &str, runtime: &str) -> Self {
        let mut launch = Self::default();
        for line in overlay.lines() {
            let Some(text) = line.strip_prefix(OVERLAY_PREFIX) else {
                continue;
            };
            launch.overlay_started = true;
            launch.overlay_line(text);
        }
        let stalls_in_overlay = launch.stalls;
        let mut stalls_in_runtime = 0;
        for line in runtime.lines() {
            if line.starts_with("[INIT] virtual memory ready") {
                launch.wine_ready = true;
            } else if let Some(code) =
                line.strip_prefix("[EXIT] NtTerminateProcess(self) exit_code=")
            {
                launch.exit = Some(parse_exit(code));
            } else if let Some(reason) = line.strip_prefix("[FAIL] ") {
                launch
                    .refused
                    .get_or_insert_with(|| reason.trim().to_owned());
            } else if let Some(text) = line.strip_prefix(CARAFE_PREFIX) {
                if text.starts_with("no progress for ") {
                    stalls_in_runtime += 1;
                } else {
                    launch.loading_line(text);
                }
            }
        }
        launch.stalls = stalls_in_overlay.max(stalls_in_runtime);
        launch
    }

    fn overlay_line(&mut self, text: &str) {
        if text.starts_with("installed over ") {
            self.overlay_installed = true;
        } else if text == "shutdown" {
            self.shutdown = true;
        } else if let Some(rest) = text.strip_prefix("commit ") {
            if let Some(commit) = parse_commit(rest) {
                self.commits.push(commit);
            }
        } else if let Some(rest) = text.strip_prefix(CARAFE_PREFIX) {
            if rest.starts_with("no progress for ") {
                self.stalls += 1;
            } else {
                self.loading_line(rest);
            }
        }
    }

    fn loading_line(&mut self, text: &str) {
        if let Some(rest) = text.strip_prefix("loading screen drawn at ") {
            if self.loading_drawn_at.is_none() {
                self.loading_drawn_at = leading_number(rest);
            }
        } else if let Some(rest) = text.strip_prefix("loading screen hidden after ")
            && self.loading_hidden.is_none()
            && let Some((seconds, reason)) = rest.split_once(" s: ")
            && let Ok(seconds) = seconds.parse()
        {
            self.loading_hidden = Some((seconds, HideReason::parse(reason)));
        }
    }

    /// Returns the furthest point the launch reached.
    #[must_use]
    pub fn stage(&self) -> Stage {
        if self
            .loading_hidden
            .as_ref()
            .is_some_and(|(_, reason)| reason.is_success())
        {
            Stage::FirstFrame
        } else if self.loading_drawn_at.is_some() {
            Stage::LoadingScreen
        } else if self.wine_ready {
            Stage::Wine
        } else if self.overlay_started {
            Stage::Overlay
        } else {
            Stage::NotStarted
        }
    }

    /// Returns whether every commit succeeded and kept every open file; `true` if there were none.
    #[must_use]
    pub fn commits_ok(&self) -> bool {
        self.commits
            .iter()
            .all(|commit| commit.ok && commit.lost == 0)
    }
}

fn parse_commit(rest: &str) -> Option<Commit> {
    let (head, tail) = rest.split_once(": rc=")?;
    let reason = match head.rsplit_once(" at ") {
        Some((reason, _)) => reason,
        None => head,
    };
    let rc = tail.split(',').next()?.trim();
    let lost = tail
        .split(", ")
        .find_map(|part| part.strip_suffix(" lost"))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Some(Commit {
        reason: reason.to_owned(),
        ok: parse_hex(rc) == Some(0),
        lost,
    })
}

fn parse_exit(code: &str) -> Exit {
    match parse_hex(code.trim()) {
        Some(0) => Exit::Clean,
        Some(code) => Exit::Code(code),
        None => Exit::Code(u32::MAX),
    }
}

fn parse_hex(text: &str) -> Option<u32> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok()
}

fn leading_number(text: &str) -> Option<u32> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENTTD_OVERLAY: &str = "\
[carafe-overlay] started 2026-10-10 04:32:36
[carafe-overlay] program 0x0563ab630a362000, volatile root /switch/carafe/0563ab630a362000
[carafe-overlay] devices: base sdmc, read-only carafe-ro, writable carafe-rw
[carafe-overlay] romfs index: 1902 entries in 4 ms
[carafe-overlay] installed over sdmc:/switch/wine, commit thread running (rc=0x00000000)
[carafe-overlay] [CARAFE] game icon 256x256
[carafe-overlay] [CARAFE] loading screen drawn at 0 s, 1280x720
[carafe-overlay] [CARAFE] loading screen hidden after 34 s: the game drew its picture
[carafe-overlay] commit after writes at 41.120 s: rc=0x00000000, 1 files reopened, 0 lost, 3120 us
[carafe-overlay] commit on close request at 95.004 s: rc=0x00000000, 0 files reopened, 0 lost, 812 us
[carafe-overlay] shutdown
";

    const OPENTTD_RUNTIME: &str = "\
[INIT] Mesa from mesa-switch: OpenGL through nvc0, Vulkan through NVK; Vulkan probe off
[INIT] virtual memory ready
[CARAFE] loading screen drawn at 0 s, 1280x720
[CARAFE] loading screen hidden after 34 s: the game drew its picture
[EXIT] the program terminated itself; returning to the launcher
[EXIT] NtTerminateProcess(self) exit_code=0x00000000
";

    #[test]
    fn a_clean_launch_reaches_the_first_frame() {
        let launch = Launch::read(OPENTTD_OVERLAY, OPENTTD_RUNTIME);
        assert!(launch.overlay_installed);
        assert!(launch.wine_ready);
        assert_eq!(launch.loading_drawn_at, Some(0));
        assert_eq!(launch.loading_hidden, Some((34, HideReason::Picture)));
        assert_eq!(launch.stage(), Stage::FirstFrame);
        assert_eq!(launch.exit, Some(Exit::Clean));
        assert!(launch.shutdown);
        assert_eq!(launch.stalls, 0);
    }

    #[test]
    fn commits_keep_their_reason_and_result() {
        let launch = Launch::read(OPENTTD_OVERLAY, "");
        assert_eq!(
            launch.commits,
            vec![
                Commit {
                    reason: "after writes".to_owned(),
                    ok: true,
                    lost: 0
                },
                Commit {
                    reason: "on close request".to_owned(),
                    ok: true,
                    lost: 0
                },
            ]
        );
        assert!(launch.commits_ok());
    }

    #[test]
    fn a_failed_or_lossy_commit_is_not_ok() {
        let failed = "[carafe-overlay] commit after writes at 2.000 s: rc=0x00002EE2, 0 files reopened, 0 lost, 9 us";
        assert!(!Launch::read(failed, "").commits_ok());
        let lossy = "[carafe-overlay] commit after writes at 2.000 s: rc=0x00000000, 2 files reopened, 1 lost, 9 us";
        let launch = Launch::read(lossy, "");
        assert_eq!(launch.commits[0].lost, 1);
        assert!(!launch.commits_ok());
    }

    #[test]
    fn time_limit_stops_at_the_loading_screen() {
        let overlay = "\
[carafe-overlay] started 2026-10-10 04:32:36
[carafe-overlay] [CARAFE] loading screen drawn at 3 s, 1280x720
[carafe-overlay] [CARAFE] no progress for 30 s, reporting the threads
[carafe-overlay] [CARAFE] loading screen hidden after 180 s: time limit
";
        let launch = Launch::read(overlay, "");
        assert_eq!(launch.stage(), Stage::LoadingScreen);
        assert_eq!(launch.stalls, 1);
        assert!(!launch.loading_hidden.unwrap().1.is_success());
    }

    #[test]
    fn stalls_in_both_logs_are_counted_once() {
        let overlay = "[carafe-overlay] [CARAFE] no progress for 30 s, reporting the threads\n";
        let runtime = "[CARAFE] no progress for 30 s, reporting the threads\n";
        assert_eq!(Launch::read(overlay, runtime).stalls, 1);
    }

    #[test]
    fn a_crash_keeps_its_code() {
        let runtime = "[EXIT] NtTerminateProcess(self) exit_code=0xc0000005\n";
        assert_eq!(
            Launch::read("", runtime).exit,
            Some(Exit::Code(0xc000_0005))
        );
    }

    #[test]
    fn a_refusal_keeps_its_first_reason() {
        let runtime = "[CPU] FEX-2609\n\
            [FAIL] AMD64 requires the 39-bit forwarder; current address space 0x8000000-0x4000000000\n\
            [FAIL] second\n";
        assert_eq!(
            Launch::read("", runtime).refused.as_deref(),
            Some(
                "AMD64 requires the 39-bit forwarder; current address space 0x8000000-0x4000000000"
            )
        );
        assert_eq!(Launch::read("", "[CPU] FEX-2609\n").refused, None);
    }

    #[test]
    fn stages_follow_start_up_order() {
        assert_eq!(Launch::read("", "").stage(), Stage::NotStarted);
        let overlay = "[carafe-overlay] started 2026-10-10 04:32:36\n";
        assert_eq!(Launch::read(overlay, "").stage(), Stage::Overlay);
        assert_eq!(
            Launch::read(overlay, "[INIT] virtual memory ready\n").stage(),
            Stage::Wine
        );
    }

    #[test]
    fn unknown_reason_still_counts_as_a_frame() {
        let runtime = "[CARAFE] loading screen hidden after 5 s: something new\n";
        let launch = Launch::read("", runtime);
        assert_eq!(
            launch.loading_hidden,
            Some((5, HideReason::Other("something new".to_owned())))
        );
        assert_eq!(launch.stage(), Stage::FirstFrame);
    }

    #[test]
    fn foreign_lines_are_ignored() {
        let overlay = "garbage\n[carafe-overlay] commit nonsense\n[carafe-overlay] [CARAFE] loading screen hidden after x s: y\n";
        let launch = Launch::read(overlay, "[CARAFE] loading screen drawn at soon\n");
        assert!(launch.commits.is_empty());
        assert_eq!(launch.loading_hidden, None);
        assert_eq!(launch.loading_drawn_at, None);
    }
}
