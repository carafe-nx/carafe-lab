//! The Eden emulator in a Docker container: the [`Emulator`] the lab runs games in.
//!
//! The container runs Eden's Qt front end full screen on Xvfb. Input goes in as key presses of Eden's default
//! keyboard layout, through the control channel when one Eden serves the run and through `xdotool` otherwise.
//! Pictures are the game's own frame, which Eden saves on the channel's `screenshot`, or ImageMagick's `import` of
//! the screen without the channel. Eden's data folder is a folder on the host, so logs and the SD card are read
//! directly. `nand`, which holds the save data, lives in
//! a Docker volume instead: Eden checks the save's files thousands of times while a game starts, and through the
//! host folder each check takes milliseconds. It is copied there before every launch and back when the lab
//! reads the save.
//!
//! With [`Eden::with_one_eden`] one container and one Eden serve the whole run: each game is booted and stopped
//! through Eden's control channel (`patches/eden/0046`), a one-line `command` file and a numbered `replies` file in
//! a shared `control/` folder. An Eden that crashed, stopped answering or wrote no status line for 45 s is replaced
//! at the next launch.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use carafe_core::TitleId;
use carafe_core::settings::SwitchButton;
use lab_core::diagnostics::Diagnostic;
use lab_core::emulator::{EdenCpu, Gpu, Video};
use lab_core::firmware;
use lab_core::frames;
use lab_core::perf::{self, PerfSample};
pub use lab_core::redact::redact;
use lab_core::resources::{self, ResourceSample};
use lab_core::stacks;
use sha2::{Digest, Sha256};

use lab_run::run::{Closed, Emulator, EmulatorError, GameLogs, SaveFile};

/// Name of the lab's Eden image: Eden v0.2.1 with `patches/eden`, built by `docker compose build eden-patched`.
pub const IMAGE: &str = "carafe-lab-eden";
const CONTAINER: &str = "carafe-lab-eden";
const MEMORY_LIMIT: &str = "6g";
const CPU_LIMIT: &str = "4";
const NAND_VOLUME: &str = "carafe-lab-eden-nand";
const SCREEN_FILE: &str = "lab-screen.png";
const FRAME_FILE: &str = "lab-frame.png";
const GLIMPSE_FILE: &str = "lab-glimpse.jpg";
const FRAME_WAIT: Duration = Duration::from_secs(5);
const CONTAINER_END_FILE: &str = "container-end.log";
const SESSION_FILE: &str = "session.log";
const RESOURCES_FILE: &str = "resources.log";
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const BOOT_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_GRACE: Duration = Duration::from_secs(15);
const PRESS_GRACE: Duration = Duration::from_secs(5);
const HANG_AFTER_MS: u64 = 45_000;
const GDB_PORT: u16 = 6543;
const GDB_INPUT: &str = "/tmp/lab-gdb.in";
const GDB_OUTPUT: &str = "/tmp/lab-gdb.out";
const GDB_END: &str = "--- carafe-lab end ---";
const GDB_SERVER_WAIT: Duration = Duration::from_secs(30);
const GDB_INTERRUPT_WAIT: Duration = Duration::from_secs(3);
const GDB_ANSWER_WAIT: Duration = Duration::from_secs(90);
const PERF_MARGIN_MS: u64 = 2000;
const GAME_MEMORY: u64 = 5 * 1024 * 1024 * 1024;
const PULLED_NAND: [&str; 2] = ["user", "system/save"];
const PUSH_NAND: &str = "set -e
R=/to/system/Contents/registered
S=/to/system/.carafe-firmware
find /to -mindepth 1 ! -path /to/system ! -path /to/system/Contents ! -path $R ! -path \"$R/*\" ! -path $S -delete
cp -a /from/. /to/
if [ -n \"$FIRMWARE\" ]; then
  if [ \"$(cat $S 2>/dev/null)\" != \"$FIRMWARE\" ]; then
    rm -rf $R && mkdir -p $R
    find /firmware -maxdepth 1 -type f -iname '*.nca' -exec cp {} $R/ \\;
    echo \"$FIRMWARE\" > $S
  fi
elif [ -f $S ]; then
  rm -rf $R $S
fi";
const ZERO_USER: &str = "00000000000000000000000000000000";
const KEEP_FILE: &str = ".carafe-lab";
const SHARED_DIRS: [&str; 7] = [
    "config",
    "sdmc",
    "nand",
    "screenshots",
    "log",
    "load",
    "cache",
];

/// Eden in Docker.
pub struct Eden {
    user_dir: PathBuf,
    keys: PathBuf,
    gpu: Gpu,
    cpu: EdenCpu,
    image: String,
    settings: BTreeMap<String, String>,
    title: Option<TitleId>,
    ended: Option<String>,
    presses: PressLog,
    one_eden: bool,
    fresh: bool,
    session: Option<Session>,
    perf_from: Option<u64>,
    firmware: Option<PathBuf>,
    video: Video,
    diagnostics: Vec<Diagnostic>,
}

struct Session {
    nsp_dir: PathBuf,
    seen: u64,
    booted_at: u64,
    log_from: usize,
    diagnostics: Vec<Diagnostic>,
}

/// A numbered answer of Eden's control channel (`patches/eden/0046`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Its number, from 1, counted since Eden started.
    pub number: u64,
    /// Its text, such as `booted` or `stopped exited`.
    pub text: String,
}

/// Returns the answers in the text of the control channel's `replies` file numbered above `after`, in order.
#[must_use]
pub fn replies_after(text: &str, after: u64) -> Vec<Reply> {
    text.lines()
        .filter_map(|line| {
            let (number, reply) = line.trim().split_once(' ')?;
            let number: u64 = number.parse().ok()?;
            (number > after).then(|| Reply {
                number,
                text: reply.to_owned(),
            })
        })
        .collect()
}

/// The buttons the lab pressed lately, shared with the live page.
#[derive(Debug, Clone, Default)]
pub struct PressLog(Arc<Mutex<Vec<(String, Instant)>>>);

impl PressLog {
    /// Shortest time a press stays visible, so that a page polling once a second sees it.
    pub const SHOWN_AT_LEAST: Duration = Duration::from_millis(1200);

    /// Records that `buttons`, by their names in `lab.json` such as `A` or `ZL`, are held for `hold`.
    pub fn record(&self, buttons: &[String], hold: Duration) {
        let until = Instant::now() + hold.max(Self::SHOWN_AT_LEAST);
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(name, _)| !buttons.contains(name));
        held.extend(buttons.iter().map(|name| (name.clone(), until)));
    }

    /// Returns the names of the buttons held now.
    #[must_use]
    pub fn held(&self) -> Vec<String> {
        let now = Instant::now();
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(_, until)| *until > now);
        held.iter().map(|(name, _)| name.clone()).collect()
    }
}

impl Eden {
    /// Creates the emulator.
    ///
    /// `user_dir` holds Eden's data folders (`nand/`, `sdmc/`, `log/`…). `keys` is the `prod.keys` file: it is
    /// mounted read-only into a `keys/` folder that lives only in the container, so neither it nor the keys Eden
    /// derives from it reach the host.
    pub fn new(
        user_dir: impl Into<PathBuf>,
        keys: impl Into<PathBuf>,
        gpu: Gpu,
        cpu: EdenCpu,
    ) -> Self {
        Self {
            user_dir: user_dir.into(),
            keys: keys.into(),
            gpu,
            cpu,
            image: IMAGE.to_owned(),
            settings: BTreeMap::new(),
            title: None,
            ended: None,
            presses: PressLog::default(),
            one_eden: false,
            fresh: false,
            session: None,
            perf_from: None,
            firmware: None,
            video: Video::Full,
            diagnostics: Vec::new(),
        }
    }

    /// Returns the emulator that keeps one Eden for the whole run and loads each game into it through the control
    /// channel, instead of a container for every launch.
    ///
    /// A crashed or hung Eden is replaced by a new one at the next launch.
    #[must_use]
    pub fn with_one_eden(mut self, one_eden: bool) -> Self {
        self.one_eden = one_eden;
        self
    }

    /// Returns the emulator that starts a new Eden for every launch and still drives it through the control channel,
    /// so no game can affect the next one; it implies [`Eden::with_one_eden`].
    #[must_use]
    pub fn with_fresh_eden(mut self, fresh: bool) -> Self {
        self.fresh = fresh;
        if fresh {
            self.one_eden = true;
        }
        self
    }

    /// Returns the emulator that runs with the Switch firmware in the folder `firmware`, a folder of `.nca` files;
    /// `None` runs Eden without firmware.
    ///
    /// The files are copied into Eden's NAND once and again only when the folder changes.
    #[must_use]
    pub fn with_firmware(mut self, firmware: Option<PathBuf>) -> Self {
        self.firmware = firmware;
        self
    }

    /// Returns the emulator whose screen goes out as live video as `video` says.
    #[must_use]
    pub fn with_video(mut self, video: Video) -> Self {
        self.video = video;
        self
    }

    /// Returns the emulator running the Docker image `image` instead of [`IMAGE`].
    #[must_use]
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = image.into();
        self
    }

    /// Returns the emulator with `settings` written over the lab's `qt-config.ini`.
    ///
    /// Each key is `Section/key`, such as `Renderer/gpu_accuracy`.
    #[must_use]
    pub fn with_settings(mut self, settings: BTreeMap<String, String>) -> Self {
        self.settings = settings;
        self
    }

    /// Returns the log of the buttons this emulator presses, for the live page.
    #[must_use]
    pub fn press_log(&self) -> PressLog {
        self.presses.clone()
    }

    /// Joins the one Eden another process started, so presses go through its control channel and the logs read are
    /// those of the game it booted last.
    ///
    /// # Returns
    ///
    /// `true` when an Eden with a control channel is running; `false` leaves this emulator as it was.
    pub fn join_running(&mut self) -> bool {
        let replies = self.replies(0);
        let Some(last) = replies.last() else {
            return false;
        };
        if !self.container_running() {
            return false;
        }
        let booted = replies
            .iter()
            .rev()
            .find_map(|reply| reply.text.strip_prefix("booting "));
        self.title = booted
            .and_then(|nsp| Path::new(nsp).file_stem())
            .and_then(|stem| TitleId::parse(&stem.to_string_lossy()).ok());
        let log = read_text(&self.user_dir.join("log").join("eden_log.txt"));
        self.session = Some(Session {
            nsp_dir: PathBuf::new(),
            seen: last.number,
            booted_at: last.number,
            log_from: log.rfind("Control: booting").unwrap_or(0),
            diagnostics: Vec::new(),
        });
        true
    }

    /// Returns the Title ID of the game this emulator launched or, after [`Eden::join_running`], the one the joined
    /// Eden booted last.
    #[must_use]
    pub fn title(&self) -> Option<TitleId> {
        self.title
    }

    /// Returns Eden's version from the image, such as `v0.2.1`.
    ///
    /// # Errors
    ///
    /// [`EmulatorError::Control`] if Docker or the image is missing.
    pub fn version(&self) -> Result<String, EmulatorError> {
        let output = docker(&[
            "run",
            "--rm",
            "--entrypoint",
            "cat",
            &self.image,
            "/opt/eden/carafe-version",
        ])?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Returns the Vulkan summary the container sees with this GPU choice.
    ///
    /// # Errors
    ///
    /// [`EmulatorError::Control`] if Docker or the image is missing.
    pub fn gpu_info(&self) -> Result<String, EmulatorError> {
        let mut args = vec!["run".to_owned(), "--rm".to_owned()];
        args.extend(self.gpu_args());
        args.push(self.image.clone());
        args.push("gpu-info".to_owned());
        let output = docker_owned(&args)?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn gpu_args(&self) -> Vec<String> {
        let mut args = vec!["-e".to_owned(), format!("CARAFE_GPU={}", self.gpu.name())];
        if self.gpu == Gpu::Wsl {
            args.extend([
                "--device".to_owned(),
                "/dev/dxg".to_owned(),
                "-v".to_owned(),
                "/usr/lib/wsl:/usr/lib/wsl:ro".to_owned(),
            ]);
        }
        args
    }

    fn prepare(&self) -> Result<(), EmulatorError> {
        for dir in SHARED_DIRS {
            fs::create_dir_all(self.user_dir.join(dir)).map_err(io)?;
        }
        fs::write(
            self.user_dir.join("config").join("qt-config.ini"),
            qt_config(self.gpu, self.cpu, &self.effective_settings()),
        )
        .map_err(io)?;
        if !self.keys.is_file() {
            return Err(EmulatorError::Control(
                "the keys file chosen in Carafe's settings is missing".to_owned(),
            ));
        }
        Ok(())
    }

    fn hide_keys(&self, error: EmulatorError) -> EmulatorError {
        match error {
            EmulatorError::Control(text) => EmulatorError::Control(redact(&text, &self.keys)),
            EmulatorError::Io(text) => EmulatorError::Io(redact(&text, &self.keys)),
        }
    }

    /// Returns the folder on the host where the runtime of `title` writes its logs.
    #[must_use]
    pub fn logs_dir(&self, title: TitleId) -> PathBuf {
        self.user_dir
            .join("sdmc")
            .join("switch")
            .join("carafe")
            .join(title.to_string())
    }

    fn save_dirs(&self, title: TitleId) -> [PathBuf; 2] {
        let upper = title.to_string().to_ascii_uppercase();
        let save = self.user_dir.join("nand").join("user").join("save");
        [
            save.join("0000000000000000").join(ZERO_USER).join(&upper),
            save.join("device").join(&upper).join("0"),
        ]
    }

    fn exec(&self, script: &str) -> Result<Output, EmulatorError> {
        docker(&["exec", "-e", "DISPLAY=:1", CONTAINER, "sh", "-c", script])
    }

    fn shut_down(&self) -> bool {
        let Some(title) = self.title else {
            return false;
        };
        let logs = self.logs(title);
        logs.overlay
            .lines()
            .any(|line| line == "[carafe-overlay] shutdown")
            || logs
                .runtime
                .lines()
                .any(|line| line.starts_with("[EXIT] NtTerminateProcess"))
    }

    fn logs_of_dir(&self, dir: &Path) -> GameLogs {
        GameLogs {
            overlay: read_text(&dir.join("carafe-overlay.log")),
            runtime: read_text(&dir.join("logs").join("autorun_runtime.log")),
        }
    }

    fn container_running(&self) -> bool {
        docker(&["inspect", "-f", "{{.State.Running}}", CONTAINER])
            .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
            .unwrap_or(false)
    }

    fn remove_container(&self) {
        let _ = docker(&["rm", "-f", CONTAINER]);
    }

    fn push_nand(&self) -> Result<(), EmulatorError> {
        let nand = self.user_dir.join("nand");
        let mut args = vec![
            "run".to_owned(),
            "--rm".to_owned(),
            "-v".to_owned(),
            format!("{}:/from:ro", nand.display()),
            "-v".to_owned(),
            format!("{NAND_VOLUME}:/to"),
        ];
        if let Some(folder) = &self.firmware {
            let files = firmware::read_folder(folder)
                .map_err(|error| EmulatorError::Control(format!("firmware: {error}")))?;
            args.extend([
                "-v".to_owned(),
                format!("{}:/firmware:ro", folder.display()),
                "-e".to_owned(),
                format!("FIRMWARE={}", firmware::stamp(&files)),
            ]);
        }
        args.extend([
            "--entrypoint".to_owned(),
            "sh".to_owned(),
            self.image.clone(),
            "-c".to_owned(),
            PUSH_NAND.to_owned(),
        ]);
        docker_owned(&args).map(|_| ())
    }

    fn pull_nand(&self) {
        let nand = self.user_dir.join("nand");
        let staged = self.user_dir.join("nand.pulling");
        let _ = fs::remove_dir_all(&staged);
        let mut copied = true;
        for part in PULLED_NAND {
            let inside = format!("{CONTAINER}:/work/user/nand/{part}/.");
            let outside = staged.join(part);
            let exists = self
                .exec(&format!("test -d /work/user/nand/{part}"))
                .is_ok();
            if !exists {
                continue;
            }
            let parent_made = outside
                .parent()
                .is_some_and(|parent| fs::create_dir_all(parent).is_ok());
            copied &= parent_made
                && docker_owned(&["cp".to_owned(), inside, outside.display().to_string()]).is_ok();
        }
        if copied && staged.is_dir() {
            let _ = fs::remove_dir_all(&nand);
            let _ = fs::rename(&staged, &nand);
        } else {
            let _ = fs::remove_dir_all(&staged);
        }
    }

    fn container_end(&self) -> Option<String> {
        let output = docker(&[
            "inspect",
            "-f",
            "{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}}",
            CONTAINER,
        ])
        .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut words = text.split_whitespace();
        let status = words.next()?;
        let code: i32 = words.next()?.parse().ok()?;
        let oom = words.next() == Some("true");
        if status != "exited" {
            return None;
        }
        let reason = describe_end(code, oom);
        if let Ok(logs) = docker(&["logs", "--tail", "200", CONTAINER]) {
            let text = format!(
                "{reason}\n\n{}{}",
                String::from_utf8_lossy(&logs.stdout),
                String::from_utf8_lossy(&logs.stderr)
            );
            let _ = fs::write(
                self.user_dir.join("log").join(CONTAINER_END_FILE),
                redact(&text, &self.keys),
            );
        }
        Some(reason)
    }
}

/// Returns whether a container with the cgroup readings `readings` has `needed` bytes left.
///
/// # Arguments
///
/// * `readings` — `memory.current` and `memory.max` one after the other, as `cat` prints them; `max` means no limit.
/// * `needed` — the bytes a game may take.
///
/// # Returns
///
/// `true` also when the readings cannot be understood, so a wrong reading never forces a new emulator.
#[must_use]
pub fn room_left(readings: &str, needed: u64) -> bool {
    let mut words = readings.split_whitespace();
    let (Some(Ok(current)), Some(Ok(max))) = (
        words.next().map(str::parse::<u64>),
        words.next().map(str::parse::<u64>),
    ) else {
        return true;
    };
    max.saturating_sub(current) >= needed
}

/// Returns how long Eden has written no status line, when that is at least `limit_ms`.
///
/// # Arguments
///
/// * `samples` — the status lines Eden wrote, oldest first.
/// * `booted` — when the game was booted, in Unix milliseconds; lines before it do not count.
/// * `now` — the time now, in Unix milliseconds.
/// * `limit_ms` — the silence that makes Eden hung.
///
/// # Returns
///
/// The silence in milliseconds, or `None` while Eden answers or the game was booted less than `limit_ms` ago.
#[must_use]
pub fn silent_for(samples: &[PerfSample], booted: u64, now: u64, limit_ms: u64) -> Option<u64> {
    let last = samples
        .iter()
        .map(|sample| sample.at)
        .filter(|&at| at >= booted)
        .max()
        .unwrap_or(booted);
    let silence = now.saturating_sub(last);
    (silence >= limit_ms).then_some(silence)
}

/// Returns why the emulator process ended, from the container's exit code.
#[must_use]
pub fn describe_end(code: i32, out_of_memory: bool) -> String {
    if out_of_memory {
        return format!("the emulator ran out of memory (exit code {code})");
    }
    let signal = match code - 128 {
        4 => Some("SIGILL, an instruction the processor refused"),
        6 => Some("SIGABRT, the emulator aborted"),
        7 => Some("SIGBUS, a bad memory access"),
        9 => Some("SIGKILL, killed from outside"),
        11 => Some("SIGSEGV, a bad memory access"),
        _ => None,
    };
    match (code, signal) {
        (0, _) => "the emulator closed by itself".to_owned(),
        (_, Some(signal)) => format!("the emulator crashed: {signal} (exit code {code})"),
        _ => format!("the emulator ended with exit code {code}"),
    }
}

impl Eden {
    fn start(&mut self, nsp: &Path) -> Result<(), EmulatorError> {
        let (dir, name) = split_nsp(nsp)?;
        self.title = nsp
            .file_stem()
            .and_then(|stem| TitleId::parse(&stem.to_string_lossy()).ok());
        if self.one_eden {
            self.boot_in_session(dir, &name)?;
            return self.attach_debugger();
        }
        self.prepare()?;
        self.remove_container();
        let _ = fs::remove_file(self.user_dir.join("log").join(RESOURCES_FILE));
        self.push_nand()?;
        self.ended = None;
        let mut args = self.container_args(dir);
        args.push(format!("/nsp/{name}"));
        docker_owned(&args)?;
        self.perf_from = Some(unix_millis());
        self.wait_for_window()?;
        self.attach_debugger()
    }

    fn effective_settings(&self) -> BTreeMap<String, String> {
        let mut settings = self.settings.clone();
        if self.watches_stacks() {
            settings.insert("Debugging/use_gdbstub".to_owned(), "true".to_owned());
            settings.insert("Debugging/gdbstub_port".to_owned(), GDB_PORT.to_string());
        }
        if self.diagnostics.contains(&Diagnostic::EdenDebug) {
            settings.insert("Miscellaneous/log_filter".to_owned(), "*:Debug".to_owned());
        }
        settings
    }

    fn watches_stacks(&self) -> bool {
        self.diagnostics.contains(&Diagnostic::Stacks)
    }

    fn attach_debugger(&mut self) -> Result<(), EmulatorError> {
        if !self.watches_stacks() {
            return Ok(());
        }
        let log_from = self.session.as_ref().map_or(0, |session| session.log_from);
        let deadline = Instant::now() + GDB_SERVER_WAIT;
        while Instant::now() < deadline {
            let log = read_text(&self.user_dir.join("log").join("eden_log.txt"));
            if log
                .get(log_from..)
                .is_some_and(|tail| tail.contains("Starting server on port"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
        let script = format!(
            "rm -f {GDB_INPUT} {GDB_OUTPUT}; touch {GDB_INPUT}; \
             tail -f {GDB_INPUT} | gdb -q -nx -ex 'set pagination off' -ex 'set confirm off' -ex 'set width 0' \
             -ex 'target remote 127.0.0.1:{GDB_PORT}' -ex 'continue' > {GDB_OUTPUT} 2>&1"
        );
        docker(&["exec", "-d", CONTAINER, "sh", "-c", &script])?;
        Ok(())
    }

    fn read_backtraces(&mut self) -> Option<String> {
        if !self.watches_stacks() || !self.container_running() {
            return None;
        }
        let mark = format!("--- carafe-lab {} ---", unix_millis());
        self.exec("kill -INT $(pgrep -x gdb)").ok()?;
        thread::sleep(GDB_INTERRUPT_WAIT);
        let commands = format!(
            "cat >> {GDB_INPUT} <<'EOF'\necho {mark}\\n\nmonitor get info\ninfo threads\n\
             thread apply all bt 40\necho {GDB_END}\\n\ncontinue\nEOF"
        );
        self.exec(&commands).ok()?;
        let deadline = Instant::now() + GDB_ANSWER_WAIT;
        while Instant::now() < deadline {
            let output = self.exec(&format!("cat {GDB_OUTPUT}")).ok()?;
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(start) = text.rfind(&mark)
                && let Some(length) = text[start..].find(GDB_END)
            {
                let body = &text[start + mark.len()..start + length];
                return Some(stacks::annotate(body.trim()));
            }
            thread::sleep(Duration::from_secs(1));
        }
        None
    }

    fn boot_in_session(&mut self, dir: &Path, name: &str) -> Result<(), EmulatorError> {
        let alive = self.session.as_ref().is_some_and(|session| {
            session.nsp_dir == dir && session.diagnostics == self.diagnostics
        }) && self.container_running();
        let readings = if alive {
            self.memory_readings()
        } else {
            String::new()
        };
        let roomy = alive && !self.fresh && room_left(&readings, GAME_MEMORY);
        let choice = match (roomy, self.fresh) {
            (true, _) => "same Eden",
            (false, true) => "new Eden: clean start",
            (false, false) => "new Eden",
        };
        let note = format!(
            "before boot: memory {} (current, max); a game needs {GAME_MEMORY}; {choice}\n",
            readings.split_whitespace().collect::<Vec<_>>().join(", "),
        );
        let _ = fs::write(self.user_dir.join("log").join(SESSION_FILE), note);
        if alive && !roomy {
            self.end_session();
        }
        if !roomy {
            self.start_session(dir)?;
        }
        let seen = self.session.as_ref().map_or(0, |session| session.seen);
        let log_from = read_text(&self.user_dir.join("log").join("eden_log.txt")).len();
        if let Some(session) = self.session.as_mut() {
            session.log_from = log_from;
        }
        self.send(&format!("boot /nsp/{name}"))?;
        let replies = self.wait_for_reply(seen, BOOT_TIMEOUT, |text| {
            text == "booted" || text.starts_with("error")
        })?;
        let last = replies
            .last()
            .map(|reply| reply.text.clone())
            .unwrap_or_default();
        if last != "booted" {
            return Err(EmulatorError::Control(format!(
                "Eden did not boot the game: {last}"
            )));
        }
        if let (Some(session), Some(reply)) = (self.session.as_mut(), replies.last()) {
            session.booted_at = reply.number;
        }
        self.perf_from = Some(unix_millis().saturating_sub(PERF_MARGIN_MS));
        let _ = self.exec("xdotool mousemove 640 360");
        Ok(())
    }

    fn start_session(&mut self, dir: &Path) -> Result<(), EmulatorError> {
        self.session = None;
        self.prepare()?;
        self.remove_container();
        let _ = fs::remove_file(self.user_dir.join("log").join(RESOURCES_FILE));
        self.push_nand()?;
        self.ended = None;
        let control = self.control_dir();
        let _ = fs::remove_dir_all(&control);
        fs::create_dir_all(&control).map_err(io)?;
        let mut args = self.container_args(dir);
        let image = args.pop().unwrap_or_default();
        args.extend([
            "-v".to_owned(),
            format!("{}:/work/control", control.display()),
            "-e".to_owned(),
            "CARAFE_CONTROL=/work/control".to_owned(),
            image,
            "session".to_owned(),
        ]);
        docker_owned(&args)?;
        self.wait_for_window()?;
        let replies = self.wait_for_reply(0, READY_TIMEOUT, |text| text == "ready")?;
        self.session = Some(Session {
            nsp_dir: dir.to_path_buf(),
            seen: replies.last().map_or(0, |reply| reply.number),
            booted_at: 0,
            log_from: 0,
            diagnostics: self.diagnostics.clone(),
        });
        Ok(())
    }

    fn frame_shot(&mut self) -> Result<PathBuf, EmulatorError> {
        let seen = self.session.as_ref().map_or(0, |session| session.seen);
        let outside = self.user_dir.join("screenshots").join(FRAME_FILE);
        let _ = fs::remove_file(&outside);
        self.send(&format!("screenshot /work/user/screenshots/{FRAME_FILE}"))?;
        let replies = self.wait_for_reply(seen, FRAME_WAIT, |text| {
            text.starts_with("capturing") || text.starts_with("error")
        })?;
        if let Some(reply) = replies.iter().find(|reply| reply.text.starts_with("error")) {
            return Err(EmulatorError::Control(format!(
                "Eden took no picture: {}",
                reply.text
            )));
        }
        let deadline = Instant::now() + FRAME_WAIT;
        let mut last_size = 0;
        while Instant::now() < deadline {
            let size = fs::metadata(&outside).map_or(0, |metadata| metadata.len());
            if size > 0 && size == last_size {
                return Ok(outside);
            }
            last_size = size;
            thread::sleep(Duration::from_millis(200));
        }
        Err(EmulatorError::Control(format!(
            "Eden wrote no picture in {} s",
            FRAME_WAIT.as_secs()
        )))
    }

    fn frame_times_file(&self, title: TitleId) -> Option<PathBuf> {
        let suffix = format!("_{}.csv", title.to_string().to_ascii_uppercase());
        let since = UNIX_EPOCH + Duration::from_millis(self.perf_from?);
        fs::read_dir(self.user_dir.join("log"))
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(&suffix))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .filter(|(modified, _)| *modified >= since)
            .max_by_key(|(modified, _)| *modified)
            .map(|(_, path)| path)
    }

    fn hung(&self) -> Option<String> {
        self.session.as_ref()?;
        let from = self.perf_from?;
        let samples = perf::parse_stats(&read_text(&self.control_dir().join("stats")));
        silent_for(&samples, from, unix_millis(), HANG_AFTER_MS)
            .map(|ms| format!("Eden stopped responding: no status for {} s", ms / 1000))
    }

    fn memory_readings(&self) -> String {
        self.exec("cat /sys/fs/cgroup/memory.current /sys/fs/cgroup/memory.max")
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default()
    }

    fn control_dir(&self) -> PathBuf {
        self.user_dir.join("control")
    }

    fn send(&self, command: &str) -> Result<(), EmulatorError> {
        let control = self.control_dir();
        let staged = control.join("command.part");
        fs::write(&staged, format!("{command}\n")).map_err(io)?;
        fs::rename(&staged, control.join("command")).map_err(io)
    }

    fn replies(&self, after: u64) -> Vec<Reply> {
        replies_after(&read_text(&self.control_dir().join("replies")), after)
    }

    fn wait_for_reply(
        &mut self,
        after: u64,
        timeout: Duration,
        wanted: impl Fn(&str) -> bool,
    ) -> Result<Vec<Reply>, EmulatorError> {
        let deadline = Instant::now() + timeout;
        loop {
            let replies = self.replies(after);
            if let Some(session) = self.session.as_mut()
                && let Some(last) = replies.last()
            {
                session.seen = session.seen.max(last.number);
            }
            if replies.iter().any(|reply| wanted(&reply.text)) {
                return Ok(replies);
            }
            if !self.container_running() {
                self.session = None;
                self.ended = self.container_end();
                return Err(EmulatorError::Control(format!(
                    "the emulator ended: {}",
                    self.ended
                        .clone()
                        .unwrap_or_else(|| "no reason known".to_owned())
                )));
            }
            if Instant::now() >= deadline {
                return Err(EmulatorError::Control(format!(
                    "Eden did not answer in {} s",
                    timeout.as_secs()
                )));
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn stopped_since_boot(&self) -> bool {
        self.session.as_ref().is_some_and(|session| {
            self.replies(session.booted_at)
                .iter()
                .any(|reply| reply.text.starts_with("stopped"))
        })
    }

    fn end_session(&mut self) {
        if self.session.take().is_some() && self.container_running() {
            let _ = self.send("quit");
            let deadline = Instant::now() + Duration::from_secs(15);
            while Instant::now() < deadline && self.container_running() {
                thread::sleep(Duration::from_millis(300));
            }
            self.pull_nand();
        }
        let _ = docker(&["stop", "-t", "5", CONTAINER]);
        self.remove_container();
    }

    fn container_args(&self, dir: &Path) -> Vec<String> {
        let mut args = vec![
            "run".to_owned(),
            "-d".to_owned(),
            "--name".to_owned(),
            CONTAINER.to_owned(),
            "-p".to_owned(),
            "127.0.0.1:6080:6080".to_owned(),
            "--ulimit".to_owned(),
            "core=1".to_owned(),
            "--memory".to_owned(),
            MEMORY_LIMIT.to_owned(),
            "--memory-swap".to_owned(),
            MEMORY_LIMIT.to_owned(),
            "--cpus".to_owned(),
            CPU_LIMIT.to_owned(),
            "-e".to_owned(),
            "SDL_AUDIODRIVER=dummy".to_owned(),
            "-e".to_owned(),
            format!("CARAFE_VIDEO={}", self.video.name()),
            "--cap-add".to_owned(),
            "SYS_PTRACE".to_owned(),
        ];
        for shared in SHARED_DIRS {
            args.push("-v".to_owned());
            if shared == "nand" {
                args.push(format!("{NAND_VOLUME}:/work/user/nand"));
            } else {
                args.push(format!(
                    "{}:/work/user/{shared}",
                    self.user_dir.join(shared).display()
                ));
            }
        }
        args.extend([
            "-v".to_owned(),
            format!("{}:/work/user/keys/prod.keys:ro", self.keys.display()),
            "-v".to_owned(),
            format!("{}:/nsp:ro", dir.display()),
        ]);
        args.extend(self.gpu_args());
        args.push(self.image.clone());
        args
    }

    fn wait_for_window(&self) -> Result<(), EmulatorError> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if !self.container_running() {
                let logs = docker(&["logs", CONTAINER]).ok();
                let text = logs
                    .map(|output| {
                        format!(
                            "{}{}",
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        )
                    })
                    .unwrap_or_default();
                return Err(EmulatorError::Control(format!(
                    "the emulator stopped at start: {}",
                    text.trim()
                )));
            }
            if self.exec("xdotool search --name . >/dev/null 2>&1").is_ok() {
                let _ = self.exec("xdotool mousemove 640 360");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }
        Err(EmulatorError::Control(
            "the emulator window did not appear in 30 s".to_owned(),
        ))
    }
}

impl Emulator for Eden {
    fn diagnose(&mut self, diagnostics: &[Diagnostic]) {
        self.diagnostics = diagnostics
            .iter()
            .filter(|diagnostic| !diagnostic.in_the_nsp())
            .copied()
            .collect();
    }

    fn backtraces(&mut self) -> Option<String> {
        self.read_backtraces()
    }

    fn launch(&mut self, nsp: &Path) -> Result<(), EmulatorError> {
        let started = self.start(nsp);
        started.map_err(|error| self.hide_keys(error))
    }

    fn running(&mut self) -> bool {
        if self.container_running() {
            if self.shut_down() || self.stopped_since_boot() {
                return false;
            }
            if let Some(reason) = self.hung() {
                self.ended = Some(reason);
                self.end_session();
                return false;
            }
            return true;
        }
        self.session = None;
        if self.ended.is_none() {
            self.ended = self.container_end();
        }
        false
    }

    fn end_reason(&self) -> Option<String> {
        self.ended.clone()
    }

    fn press(&mut self, buttons: &[SwitchButton], hold: Duration) -> Result<(), EmulatorError> {
        let names: Vec<String> = buttons.iter().map(|&button| button_name(button)).collect();
        self.presses.record(&names, hold);
        let keys: Vec<&str> = buttons.iter().map(|&button| eden_key(button)).collect();
        let keys = keys.join(" ");
        if let Some(seen) = self.session.as_ref().map(|session| session.seen) {
            let hold_ms = hold.as_millis().max(1);
            self.send(&format!("press {hold_ms} {keys}"))?;
            let replies = self.wait_for_reply(seen, hold + PRESS_GRACE, |text| {
                text == "released" || text.starts_with("error")
            })?;
            return match replies.iter().find(|reply| reply.text.starts_with("error")) {
                Some(reply) => Err(EmulatorError::Control(format!(
                    "Eden did not press {keys}: {}",
                    reply.text
                ))),
                None => Ok(()),
            };
        }
        let script = format!(
            "xdotool mousemove 640 360 keydown {keys} && sleep {:.3} && xdotool keyup {keys}",
            hold.as_secs_f64()
        );
        self.exec(&script).map(|_| ())
    }

    fn glimpse(&mut self, dest: &Path) -> Result<(), EmulatorError> {
        let inside = format!("/work/user/screenshots/{GLIMPSE_FILE}");
        self.exec(&format!(
            "import -window root -display :1 -resize 640x360 -quality 70 jpg:{inside}"
        ))?;
        let outside = self.user_dir.join("screenshots").join(GLIMPSE_FILE);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::copy(&outside, dest).map_err(io)?;
        let _ = fs::remove_file(&outside);
        Ok(())
    }

    fn screenshot(&mut self, dest: &Path) -> Result<(), EmulatorError> {
        let frame = if self.session.is_some() {
            self.frame_shot().ok()
        } else {
            None
        };
        let outside = match frame {
            Some(file) => file,
            None => {
                let inside = format!("/work/user/screenshots/{SCREEN_FILE}");
                self.exec(&format!("import -window root -display :1 png24:{inside}"))?;
                self.user_dir.join("screenshots").join(SCREEN_FILE)
            }
        };
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::copy(&outside, dest).map_err(io)?;
        let _ = fs::remove_file(&outside);
        Ok(())
    }

    fn close(&mut self, timeout: Duration) -> Result<Closed, EmulatorError> {
        if !self.container_running() {
            self.session = None;
            if self.ended.is_none() {
                self.ended = self.container_end();
            }
            return Ok(if self.shut_down() {
                Closed::ByRequest
            } else {
                Closed::Forced
            });
        }
        self.presses
            .record(&["HOME".to_owned()], PressLog::SHOWN_AT_LEAST);
        if let Some(booted_at) = self.session.as_ref().map(|session| session.booted_at) {
            if self.stopped_since_boot() {
                return Ok(if self.shut_down() {
                    Closed::ByRequest
                } else {
                    Closed::Forced
                });
            }
            self.send("stop")?;
            let waited = self.wait_for_reply(booted_at, timeout + STOP_GRACE, |text| {
                text.starts_with("stopped")
            });
            return Ok(match waited {
                Ok(replies) => {
                    let exited = replies.iter().any(|reply| reply.text == "stopped exited");
                    if exited || self.shut_down() {
                        Closed::ByRequest
                    } else {
                        Closed::Forced
                    }
                }
                Err(_) => {
                    self.end_session();
                    Closed::Forced
                }
            });
        }
        let _ = self.exec("pkill -USR1 -x eden");
        let deadline = Instant::now() + timeout;
        let mut closed = Closed::Forced;
        while Instant::now() < deadline {
            if self.shut_down() {
                closed = Closed::ByRequest;
                thread::sleep(Duration::from_secs(2));
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
        let _ = docker(&["stop", "-t", "5", CONTAINER]);
        self.remove_container();
        Ok(closed)
    }

    fn live_logs(&self, title: TitleId) -> Option<PathBuf> {
        Some(self.logs_dir(title))
    }

    fn logs(&self, title: TitleId) -> GameLogs {
        self.logs_of_dir(&self.logs_dir(title))
    }

    fn take_logs(&mut self, title: TitleId, dest: &Path) -> Result<(), EmulatorError> {
        let emulator_log = self.user_dir.join("log").join("eden_log.txt");
        if emulator_log.is_file() {
            fs::create_dir_all(dest).map_err(io)?;
            fs::write(dest.join("eden_log.txt"), self.emulator_log()).map_err(io)?;
        }
        if let Some(recorded) = self.frame_times_file(title) {
            fs::create_dir_all(dest).map_err(io)?;
            if fs::rename(&recorded, dest.join(frames::FILE)).is_err() {
                fs::copy(&recorded, dest.join(frames::FILE)).map_err(io)?;
                let _ = fs::remove_file(&recorded);
            }
        }
        let readings = self.launch_resources();
        if !readings.is_empty() {
            fs::create_dir_all(dest).map_err(io)?;
            let lines: String = readings
                .iter()
                .map(|sample| format!("{} {} {}\n", sample.at, sample.memory, sample.cpu_usec))
                .collect();
            fs::write(dest.join(RESOURCES_FILE), lines).map_err(io)?;
        }
        let container_end = self.user_dir.join("log").join(CONTAINER_END_FILE);
        if container_end.is_file() {
            fs::create_dir_all(dest).map_err(io)?;
            fs::copy(&container_end, dest.join(CONTAINER_END_FILE)).map_err(io)?;
            let _ = fs::remove_file(&container_end);
        }
        let session = self.user_dir.join("log").join(SESSION_FILE);
        if session.is_file() {
            fs::create_dir_all(dest).map_err(io)?;
            fs::copy(&session, dest.join(SESSION_FILE)).map_err(io)?;
            let _ = fs::remove_file(&session);
        }
        let source = self.logs_dir(title);
        if !source.exists() {
            return Ok(());
        }
        move_tree(&source, dest).map_err(io)
    }

    fn clear_save(&mut self, title: TitleId) -> Result<(), EmulatorError> {
        if self.session.is_some() && self.container_running() {
            let upper = title.to_string().to_ascii_uppercase();
            let save = "/work/user/nand/user/save";
            let script = format!(
                "rm -rf {save}/0000000000000000/{ZERO_USER}/{upper} {save}/device/{upper} {save}/{upper} \
                 && mkdir -p {save}/device/{upper}/0 && touch {save}/device/{upper}/{KEEP_FILE}"
            );
            return self.exec(&script).map(|_| ());
        }
        let misplaced = self
            .user_dir
            .join("nand")
            .join("user")
            .join("save")
            .join(title.to_string().to_ascii_uppercase());
        for dir in self.save_dirs(title).iter().chain([&misplaced]) {
            if dir.exists() {
                fs::remove_dir_all(dir).map_err(io)?;
            }
        }
        let [device, _] = self.save_dirs(title);
        fs::create_dir_all(&device).map_err(io)?;
        let user = device.parent().unwrap_or(&device);
        fs::write(user.join(KEEP_FILE), b"").map_err(io)
    }

    fn save_files(&self, title: TitleId) -> Vec<SaveFile> {
        self.pull_nand();
        let mut files = Vec::new();
        for dir in self.save_dirs(title) {
            collect_files(&dir, &dir, &mut files);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    }

    fn emulator_log(&self) -> String {
        let text = read_text(&self.user_dir.join("log").join("eden_log.txt"));
        match &self.session {
            Some(session) => text.get(session.log_from..).unwrap_or(&text).to_owned(),
            None => text,
        }
    }

    fn launch_resources(&self) -> Vec<ResourceSample> {
        let Some(from) = self.perf_from else {
            return Vec::new();
        };
        let text = read_text(&self.user_dir.join("log").join(RESOURCES_FILE));
        resources::since(&resources::parse(&text), from)
    }

    fn launch_perf(&self) -> Vec<PerfSample> {
        let Some(from) = self.perf_from else {
            return Vec::new();
        };
        perf::parse_stats(&read_text(&self.control_dir().join("stats")))
            .into_iter()
            .filter(|sample| sample.at >= from)
            .collect()
    }

    fn finish(&mut self) {
        if self.one_eden {
            self.end_session();
        }
    }

    fn find_file(&self, title: TitleId, name: &str) -> Option<String> {
        self.pull_nand();
        let [user_save, device_save] = self.save_dirs(title);
        [user_save, device_save, self.logs_dir(title)]
            .iter()
            .find_map(|dir| find_named(dir, name))
            .map(|path| read_text(&path))
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn split_nsp(nsp: &Path) -> Result<(&Path, String), EmulatorError> {
    let dir = nsp
        .parent()
        .ok_or_else(|| EmulatorError::Control(format!("no folder for {}", nsp.display())))?;
    let name = nsp
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| EmulatorError::Control(format!("no file name in {}", nsp.display())))?;
    Ok((dir, name))
}

fn find_named(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut folders = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            folders.push(path);
        } else if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Some(path);
        }
    }
    folders.iter().find_map(|folder| find_named(folder, name))
}

/// Returns the name of `button` as `lab.json` and Carafe's settings write it, such as `A`, `ZL` or `LUP`.
#[must_use]
pub fn button_name(button: SwitchButton) -> String {
    serde_json::to_value(button)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Returns every button with the key of Eden's default keyboard layout that presses it, as a JSON object from
/// key to button name, for the live page to light up the buttons a viewer presses.
#[must_use]
pub fn eden_keys_json() -> String {
    let pairs: serde_json::Map<String, serde_json::Value> = SwitchButton::ALL
        .iter()
        .map(|&button| (eden_key(button).to_owned(), button_name(button).into()))
        .collect();
    serde_json::Value::Object(pairs).to_string()
}

/// Returns the key of Eden's default keyboard layout that presses `button`.
#[must_use]
pub fn eden_key(button: SwitchButton) -> &'static str {
    match button {
        SwitchButton::A => "c",
        SwitchButton::B => "x",
        SwitchButton::X => "v",
        SwitchButton::Y => "z",
        SwitchButton::L => "q",
        SwitchButton::R => "e",
        SwitchButton::Zl => "r",
        SwitchButton::Zr => "t",
        SwitchButton::Plus => "m",
        SwitchButton::Minus => "n",
        SwitchButton::StickL => "f",
        SwitchButton::StickR => "g",
        SwitchButton::Up => "Up",
        SwitchButton::Down => "Down",
        SwitchButton::Left => "Left",
        SwitchButton::Right => "Right",
        SwitchButton::LUp => "w",
        SwitchButton::LDown => "s",
        SwitchButton::LLeft => "a",
        SwitchButton::LRight => "d",
        SwitchButton::RUp => "i",
        SwitchButton::RDown => "k",
        SwitchButton::RLeft => "j",
        SwitchButton::RRight => "l",
    }
}

/// Returns Eden's `qt-config.ini` for the lab: the console's four cores and 4 GB, docked, full screen, no update
/// check, no stop confirmation, no controller applet, audio through SDL's dummy driver, the processor mode `cpu`, and
/// the renderer `gpu` can drive — OpenGL on Mesa's Direct3D 12 driver through WSL2, Vulkan on Mesa's software driver.
/// `extra` replaces or adds settings; each key is `Section/key`, and a key without a section is ignored.
#[must_use]
pub fn qt_config(gpu: Gpu, cpu: EdenCpu, extra: &BTreeMap<String, String>) -> String {
    let backend = match gpu {
        Gpu::Wsl => "0",
        Gpu::Software => "1",
    };
    let cpu_backend = match cpu {
        EdenCpu::Dynarmic => "0",
        EdenCpu::Nce => "1",
    };
    let sections: [(&str, &[(&str, &str)]); 7] = [
        ("Cpu", &[("cpu_backend", cpu_backend)]),
        (
            "Core",
            &[("use_multi_core", "true"), ("memory_layout_mode", "0")],
        ),
        ("System", &[("use_docked_mode", "1")]),
        ("Debugging", &[("record_frame_times", "true")]),
        (
            "UI",
            &[
                ("check_for_updates", "false"),
                ("confirmStop", "2"),
                ("fullscreen", "true"),
                ("disableControllerApplet", "true"),
                ("Screenshots\\screenshot_path", "/work/user/screenshots"),
            ],
        ),
        (
            "Renderer",
            &[
                ("backend", backend),
                ("resolution_setup", "3"),
                ("fullscreen_mode", "0"),
            ],
        ),
        ("Audio", &[("output_engine", "2")]),
    ];
    let mut merged: Vec<(String, Vec<(String, String)>)> = sections
        .iter()
        .map(|(section, values)| {
            let values = values
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect();
            ((*section).to_owned(), values)
        })
        .collect();
    for (path, value) in extra {
        let Some((section, key)) = path.split_once('/') else {
            continue;
        };
        let index = match merged.iter().position(|(name, _)| name == section) {
            Some(index) => index,
            None => {
                merged.push((section.to_owned(), Vec::new()));
                merged.len() - 1
            }
        };
        let values = &mut merged[index].1;
        match values.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => entry.1.clone_from(value),
            None => values.push((key.to_owned(), value.clone())),
        }
    }
    let mut out = String::new();
    for (section, values) in merged {
        out.push_str(&format!("[{section}]\n"));
        for (key, value) in values {
            out.push_str(&format!("{key}\\default=false\n{key}={value}\n"));
        }
        out.push('\n');
    }
    out
}

fn move_tree(source: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            move_tree(&path, &target)?;
            let _ = fs::remove_dir(&path);
        } else if path.extension().is_some_and(|ext| ext == "shm") {
            let _ = fs::remove_file(&path);
        } else if fs::rename(&path, &target).is_err() {
            fs::copy(&path, &target)?;
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<SaveFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect_files(root, &path, files);
        } else if let Ok(mut file) = fs::File::open(&path) {
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 64 * 1024];
            let mut size = 0u64;
            loop {
                match file.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        hasher.update(&buffer[..read]);
                        size += read as u64;
                    }
                }
            }
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push(SaveFile {
                path: relative,
                size,
                sha256: hex(&hasher.finalize()),
            });
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_text(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn io(error: std::io::Error) -> EmulatorError {
    EmulatorError::Io(error.to_string())
}

fn docker(args: &[&str]) -> Result<Output, EmulatorError> {
    let owned: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    docker_owned(&owned)
}

fn docker_owned(args: &[String]) -> Result<Output, EmulatorError> {
    let output = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| EmulatorError::Control(format!("docker: {error}")))?;
    if output.status.success() {
        Ok(output)
    } else {
        let action = args.first().map_or("", String::as_str);
        Err(EmulatorError::Control(format!(
            "docker {action}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_has_its_own_key() {
        let mut keys: Vec<&str> = SwitchButton::ALL
            .iter()
            .map(|&button| eden_key(button))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SwitchButton::ALL.len());
    }

    #[test]
    fn every_setting_carries_its_default_flag() {
        let config = qt_config(Gpu::Software, EdenCpu::Dynarmic, &BTreeMap::new());
        let values: Vec<&str> = config
            .lines()
            .filter(|line| line.contains('=') && !line.contains("\\default="))
            .collect();
        for line in &values {
            let key = line.split('=').next().unwrap();
            assert!(config.contains(&format!("{key}\\default=false")), "{key}");
        }
        assert!(config.contains("[Renderer]\nbackend\\default=false\nbackend=1"));
    }

    #[test]
    fn the_processor_mode_reaches_eden() {
        assert!(
            qt_config(Gpu::Wsl, EdenCpu::Dynarmic, &BTreeMap::new())
                .contains("[Cpu]\ncpu_backend\\default=false\ncpu_backend=0\n")
        );
        assert!(qt_config(Gpu::Wsl, EdenCpu::Nce, &BTreeMap::new()).contains("\ncpu_backend=1\n"));
    }

    #[test]
    fn eden_is_set_up_like_the_console() {
        let config = qt_config(Gpu::Wsl, EdenCpu::Dynarmic, &BTreeMap::new());
        assert!(config.contains("[Core]\nuse_multi_core\\default=false\nuse_multi_core=true\n"));
        assert!(config.contains("\nmemory_layout_mode=0\n"));
        assert!(config.contains("[System]\nuse_docked_mode\\default=false\nuse_docked_mode=1\n"));
    }

    #[test]
    fn eden_is_hung_after_a_long_silence_since_its_last_status() {
        let sample = |at| PerfSample {
            at,
            fps: 30.0,
            speed: 1.0,
            frametime: 33.0,
            shaders: 0,
        };
        let samples = [sample(500), sample(10_000), sample(20_000)];
        assert_eq!(silent_for(&samples, 1_000, 30_000, 45_000), None);
        assert_eq!(silent_for(&samples, 1_000, 70_000, 45_000), Some(50_000));
        assert_eq!(silent_for(&[], 1_000, 40_000, 45_000), None);
        assert_eq!(
            silent_for(&[sample(500)], 1_000, 50_000, 45_000),
            Some(49_000)
        );
    }

    #[test]
    fn a_game_needs_its_memory_left_under_the_limit() {
        let gib = 1024 * 1024 * 1024;
        assert!(room_left("1073741824\n6442450944\n", 4 * gib));
        assert!(!room_left("2684354560\n6442450944\n", 4 * gib));
        assert!(room_left("2684354560\nmax\n", 4 * gib));
        assert!(room_left("", 4 * gib));
    }

    #[test]
    fn crashes_are_named_by_their_signal() {
        assert_eq!(
            describe_end(132, false),
            "the emulator crashed: SIGILL, an instruction the processor refused (exit code 132)"
        );
        assert!(describe_end(139, false).contains("SIGSEGV"));
        assert!(describe_end(137, true).contains("out of memory"));
        assert_eq!(describe_end(0, false), "the emulator closed by itself");
        assert_eq!(
            describe_end(3, false),
            "the emulator ended with exit code 3"
        );
    }

    #[test]
    fn the_wsl_gpu_draws_with_opengl() {
        assert!(qt_config(Gpu::Wsl, EdenCpu::Nce, &BTreeMap::new()).contains("\nbackend=0\n"));
    }

    #[test]
    fn extra_settings_replace_and_add_keys() {
        let extra = BTreeMap::from([
            ("Renderer/backend".to_owned(), "1".to_owned()),
            ("Renderer/gpu_accuracy".to_owned(), "0".to_owned()),
            ("Core/use_multi_core".to_owned(), "false".to_owned()),
            ("no_section".to_owned(), "x".to_owned()),
        ]);
        let config = qt_config(Gpu::Wsl, EdenCpu::Dynarmic, &extra);
        assert!(config.contains("\nbackend=1\n"));
        assert!(!config.contains("\nbackend=0\n"));
        assert!(config.contains("gpu_accuracy\\default=false\ngpu_accuracy=0\n"));
        assert!(config.contains("[Core]\nuse_multi_core\\default=false\nuse_multi_core=false\n"));
        assert!(!config.contains("no_section"));
    }

    #[test]
    fn logs_move_out_and_session_files_go() {
        let root = std::env::temp_dir().join(format!("carafe-lab-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        fs::create_dir_all(source.join("logs")).unwrap();
        fs::write(source.join("carafe-overlay.log"), "a").unwrap();
        fs::write(source.join("logs").join("autorun_runtime.log"), "b").unwrap();
        fs::write(source.join("wine-nx-session-1.shm"), "c").unwrap();
        move_tree(&source, &root.join("dest")).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dest").join("carafe-overlay.log")).unwrap(),
            "a"
        );
        assert!(
            root.join("dest")
                .join("logs")
                .join("autorun_runtime.log")
                .is_file()
        );
        assert!(!root.join("dest").join("wine-nx-session-1.shm").exists());
        assert!(!source.join("carafe-overlay.log").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn replies_are_read_after_the_last_one_seen() {
        let text = "1 ready\n2 booting /nsp/a.nsp\n3 booted\nnot a reply\n4 stopped exited\n";
        let fresh = replies_after(text, 2);
        let texts: Vec<&str> = fresh.iter().map(|reply| reply.text.as_str()).collect();
        assert_eq!(texts, ["booted", "stopped exited"]);
        assert_eq!(fresh[0].number, 3);
        assert!(replies_after(text, 4).is_empty());
    }

    #[test]
    fn save_files_are_hashed_with_paths_relative_to_the_save() {
        let root = std::env::temp_dir().join(format!("carafe-lab-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("dir")).unwrap();
        fs::write(root.join("dir").join("game.sav"), "abc").unwrap();
        let mut files = Vec::new();
        collect_files(&root, &root, &mut files);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "dir/game.sav");
        assert_eq!(files[0].size, 3);
        assert_eq!(
            files[0].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
