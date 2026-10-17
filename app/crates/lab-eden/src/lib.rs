//! The Eden emulator in a Docker container: the [`Emulator`] the lab runs games in.
//!
//! The container runs Eden's Qt front end full screen on Xvfb. Input goes in as key presses through `xdotool`
//! with Eden's default keyboard layout, pictures come out through ImageMagick's `import`, and Eden's data folder
//! is a folder on the host, so logs and the SD card are read directly. `nand`, which holds the save data, lives in
//! a Docker volume instead: Eden checks the save's files thousands of times while a game starts, and through the
//! host folder each check takes milliseconds. It is copied there before every launch and back when the lab
//! reads the save.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use carafe_core::TitleId;
use carafe_core::settings::SwitchButton;
use lab_core::emulator::{EdenCpu, Gpu};
pub use lab_core::redact::redact;
use sha2::{Digest, Sha256};

use lab_run::run::{Closed, Emulator, EmulatorError, GameLogs, SaveFile};

/// Name of the lab's Eden image: Eden v0.2.1 with `patches/eden`, built by `docker compose build eden-patched`.
pub const IMAGE: &str = "carafe-lab-eden";
const CONTAINER: &str = "carafe-lab-eden";
const MEMORY_LIMIT: &str = "6g";
const CPU_LIMIT: &str = "4";
const NAND_VOLUME: &str = "carafe-lab-eden-nand";
const SCREEN_FILE: &str = "lab-screen.png";
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
        }
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
            qt_config(self.gpu, self.cpu, &self.settings),
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
        docker_owned(&[
            "run".to_owned(),
            "--rm".to_owned(),
            "-v".to_owned(),
            format!("{}:/from:ro", nand.display()),
            "-v".to_owned(),
            format!("{NAND_VOLUME}:/to"),
            "--entrypoint".to_owned(),
            "sh".to_owned(),
            self.image.clone(),
            "-c".to_owned(),
            "find /to -mindepth 1 -delete && cp -a /from/. /to/".to_owned(),
        ])
        .map(|_| ())
    }

    fn pull_nand(&self) {
        let nand = self.user_dir.join("nand");
        let staged = self.user_dir.join("nand.pulling");
        let _ = fs::remove_dir_all(&staged);
        let copied = docker_owned(&[
            "cp".to_owned(),
            format!("{CONTAINER}:/work/user/nand/."),
            staged.display().to_string(),
        ]);
        if copied.is_ok() && staged.is_dir() {
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
        (status == "exited").then(|| describe_end(code, oom))
    }
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
        self.prepare()?;
        self.remove_container();
        self.push_nand()?;
        let dir = nsp
            .parent()
            .ok_or_else(|| EmulatorError::Control(format!("no folder for {}", nsp.display())))?;
        let name = nsp
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| EmulatorError::Control(format!("no file name in {}", nsp.display())))?;
        self.title = nsp
            .file_stem()
            .and_then(|stem| TitleId::parse(&stem.to_string_lossy()).ok());
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
            "--cap-add".to_owned(),
            "SYS_PTRACE".to_owned(),
        ];
        self.ended = None;
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
        args.push(format!("/nsp/{name}"));
        docker_owned(&args)?;
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
    fn launch(&mut self, nsp: &Path) -> Result<(), EmulatorError> {
        let started = self.start(nsp);
        started.map_err(|error| self.hide_keys(error))
    }

    fn running(&mut self) -> bool {
        if self.container_running() {
            return !self.shut_down();
        }
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
        let script = format!(
            "xdotool mousemove 640 360 keydown {keys} && sleep {:.3} && xdotool keyup {keys}",
            hold.as_secs_f64()
        );
        self.exec(&script).map(|_| ())
    }

    fn screenshot(&mut self, dest: &Path) -> Result<(), EmulatorError> {
        let inside = format!("/work/user/screenshots/{SCREEN_FILE}");
        self.exec(&format!("import -window root -display :1 png24:{inside}"))?;
        let outside = self.user_dir.join("screenshots").join(SCREEN_FILE);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::copy(&outside, dest).map_err(io)?;
        let _ = fs::remove_file(&outside);
        Ok(())
    }

    fn close(&mut self, timeout: Duration) -> Result<Closed, EmulatorError> {
        if !self.container_running() {
            return Ok(if self.shut_down() {
                Closed::ByRequest
            } else {
                Closed::Forced
            });
        }
        self.presses
            .record(&["HOME".to_owned()], PressLog::SHOWN_AT_LEAST);
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
            fs::copy(&emulator_log, dest.join("eden_log.txt")).map_err(io)?;
        }
        let source = self.logs_dir(title);
        if !source.exists() {
            return Ok(());
        }
        move_tree(&source, dest).map_err(io)
    }

    fn clear_save(&mut self, title: TitleId) -> Result<(), EmulatorError> {
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
        read_text(&self.user_dir.join("log").join("eden_log.txt"))
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

/// Returns Eden's `qt-config.ini` for the lab: full screen, no update check, no stop confirmation, no
/// controller applet, audio through SDL's dummy driver, the processor mode `cpu`, and the renderer `gpu` can
/// drive — OpenGL on Mesa's Direct3D 12 driver through WSL2, Vulkan on Mesa's software driver. `extra` replaces or
/// adds settings; each key is `Section/key`, and a key without a section is ignored.
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
    let sections: [(&str, &[(&str, &str)]); 4] = [
        ("Cpu", &[("cpu_backend", cpu_backend)]),
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
