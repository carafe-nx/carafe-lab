//! The command line of `carafe-lab`.

use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use lab_core::diagnostics::{self, Diagnostic};
use lab_core::emulator::{Gpu, Video};
use lab_core::lang::Lang;

/// Help printed for `carafe-lab help` and after a command line error.
pub const HELP: &str = "\
carafe-lab: builds games and runs them in the Eden emulator

Usage:
  carafe-lab run [set] [options]   the enabled games of lab.json, or the games of a set
  carafe-lab probes [options]      the probe programs over a pairwise set of settings
  carafe-lab gpu [options]         what Vulkan the emulator container sees
  carafe-lab status [--json]       who holds the emulator, who waits, which Carafe the lab packs with
  carafe-lab games [--json]        the games and sets of lab.json
  carafe-lab runs [--json]         past runs and the one going, newest first
  carafe-lab stop [id]             stops the run holding the emulator, or the run with this queue id
  carafe-lab play <game>           launches the game and keeps it running for --minutes (default 30) or
                                   until stop, for hands or for the commands below
  carafe-lab press <A+B…>          presses buttons together in the running game; --hold <s>, default 0.15
  carafe-lab screen <file.png>     saves the running game's screen
  carafe-lab perf [--json]         the emulator's FPS and speed over the last 10 s
  carafe-lab logs [--json]         the running game's start-up stages and the errors and warnings of its logs
  carafe-lab nsp <file> [--json]   what a ready NSP was built from: runtime version, settings, game folder
  carafe-lab bisect <game> --good <rev> --bad <rev>
                                   finds the Carafe commit that broke the game: builds the runtime of the
                                   commits between, runs the game --repeat times with each, halves the range
  carafe-lab release <tag>         builds the runtime of a Carafe release once from its tag and keeps it;
                                   latest is the newest release
  carafe-lab tidy [--apply]        what keeping the runs folder small would do; --apply does it: the 10 newest runs
                                   stay whole, older failed runs get their logs compressed, older passing runs
                                   lose their raw logs and timeline pictures; NSPs the newest runs do not use,
                                   Eden's crash dumps and stopped lab containers go
  carafe-lab help

Options:
  --config <file>     default: lab.json in the repository
  --keys <file>       default: the file chosen in Carafe's settings
  --gpu wsl|software  overrides lab.json
  --lang ru|en        overrides lab.json
  --timeout <s>       seconds to wait for a first frame; overrides lab.json
  --only <name>       only the game or probe with this name
  --games <a,b,…>     run these games of lab.json, in this order, instead of a set
  --repeat <n>        run every game n times in a row; default 1
  --ab <json>         A/B: run the one game as it is (A) and with these settings merged over its own (B),
                      alternating, --repeat times each; the run ends with a verdict
  --build-only        build the NSPs and stop
  --clean             a new Eden for every launch instead of one for the whole run
  --no-retry          do not run a failed game again with detailed logs (A/B runs and probes never do)
  --diag <a,b,…>      run with these diagnostics: verbose (runtime traces with Wine's output), profiler, hud (DXVK's
                      full HUD), stacks (thread stacks of a hung game in backtraces.txt), eden-debug (Eden's log at
                      the debug level); a retry after a failure picks its own
  --scenario <s>      every game runs this scenario: standard, or a file relative to lab.json
  --like <run>        run the games of this past run again with the runtime and Eden image it ran with;
                      every run keeps them from now on
  --nsp <file>        run: a ready NSP made by Carafe instead of the games of lab.json, as it is, with the
                      runtime and settings inside it; give it once for every NSP
  --runtime <file>    run: pack with this runtime archive instead of the Carafe checkout's
  --good <rev>        bisect: a Carafe commit or tag where the game worked
  --bad <rev>         bisect: a later one where it fails
  --ab-runtime <r>    A/B between runtimes: run the one game with the lab's runtime (A) and with r (B), alternating,
                      --repeat times each; r is release:<tag>, release:latest or a runtime archive
  --rebuild           with --nsp: build each NSP again from its game folder with the lab's runtime and the
                      NSP's own settings
  --video <v>         full, low (about five pictures a second, for measuring) or off; default full
  --no-open           do not open the live page in the browser
  --json              print the run as JSON lines, one event each, without the live page;
                      status, games and runs print one JSON document
  --who <name>        who starts the run, shown in the queue; default: command line
  --minutes <n>       play: minutes to keep the game running
  --hold <s>          press: seconds to hold the buttons
  --random <n>        probes: add n random combinations to the pairwise set
  --seed <n>          probes: seed of the random combinations; default: the date
  --probes-dir <dir>  probes: folder with i686/ and x86_64/ builds; default: tools/probes/build
";

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Build and run games from `lab.json`.
    Run {
        /// The set to run; `None` means every enabled game.
        set: Option<String>,
    },
    /// Run the probe programs.
    Probes,
    /// Show what Vulkan the container sees.
    Gpu,
    /// Show the queue and the Carafe checkout.
    Status,
    /// List the games and sets of `lab.json`.
    Games,
    /// List the runs.
    Runs,
    /// Stop a run.
    Stop {
        /// The queue id of the run; `None` means the run holding the emulator.
        id: Option<String>,
    },
    /// Launch a game and keep it running.
    Play {
        /// The game's name in `lab.json`.
        game: Option<String>,
    },
    /// Press buttons in the running game.
    Press {
        /// The buttons, joined with `+`.
        buttons: Option<String>,
    },
    /// Save the running game's screen.
    Screen {
        /// Where to save the picture.
        file: Option<PathBuf>,
    },
    /// Show the emulator's recent performance.
    Perf,
    /// Show the running game's stages and the problems of its logs.
    Logs,
    /// Find the Carafe commit that broke a game.
    Bisect {
        /// The game's name in `lab.json`.
        game: Option<String>,
    },
    /// Build and keep the runtime of a Carafe release.
    Release {
        /// The release tag, or `latest`.
        tag: Option<String>,
    },
    /// Show what a ready NSP was built from.
    Nsp {
        /// The NSP file.
        file: Option<PathBuf>,
    },
    /// Keep the runs folder small.
    Tidy,
    /// Print the help.
    Help,
}

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// What to do.
    pub command: Command,
    /// `--config`.
    pub config: Option<PathBuf>,
    /// `--keys`.
    pub keys: Option<PathBuf>,
    /// `--gpu`.
    pub gpu: Option<Gpu>,
    /// `--lang`.
    pub lang: Option<Lang>,
    /// `--timeout`.
    pub timeout: Option<u32>,
    /// `--only`.
    pub only: Option<String>,
    /// `--games`, split at commas.
    pub games: Vec<String>,
    /// `--repeat`.
    pub repeat: u32,
    /// `--runtime`.
    pub runtime: Option<PathBuf>,
    /// `--good`.
    pub good: Option<String>,
    /// `--bad`.
    pub bad: Option<String>,
    /// `--ab-runtime`: the runtime of variant B.
    pub ab_runtime: Option<String>,
    /// `--ab`: the settings of variant B, as JSON.
    pub ab: Option<String>,
    /// `--minutes`.
    pub minutes: u32,
    /// `--hold`, in milliseconds.
    pub hold_ms: u32,
    /// `--build-only`.
    pub build_only: bool,
    /// `--clean`.
    pub clean: bool,
    /// `--apply`.
    pub apply: bool,
    /// `--no-retry`.
    pub no_retry: bool,
    /// `--diag`.
    pub diagnostics: Vec<Diagnostic>,
    /// `--like`: the folder of a past run to repeat with its own parts.
    pub like: Option<String>,
    /// `--nsp`, once for every ready NSP to run.
    pub nsp: Vec<PathBuf>,
    /// `--rebuild`.
    pub rebuild: bool,
    /// `--scenario`: `standard` or a scenario file relative to `lab.json`.
    pub scenario: Option<String>,
    /// `--video`.
    pub video: Video,
    /// `--json`.
    pub json: bool,
    /// `--who`.
    pub who: Option<String>,
    /// `--no-open`.
    pub no_open: bool,
    /// `--random`.
    pub random: usize,
    /// `--seed`.
    pub seed: Option<u64>,
    /// `--probes-dir`.
    pub probes_dir: Option<PathBuf>,
}

/// A command line error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArgsError {
    /// No command or an unknown one.
    #[error("unknown command: {0}")]
    Command(String),
    /// An option the command does not know.
    #[error("unknown option: {0}")]
    Option(String),
    /// An option without its value or with a wrong one.
    #[error("{0} needs {1}")]
    Value(String, &'static str),
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// [`ArgsError`] for an unknown command or option, or a missing or wrong value.
pub fn parse(args: &[String]) -> Result<Args, ArgsError> {
    let mut rest = args.iter();
    let command = match rest.next().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => Command::Help,
        Some("run") => Command::Run { set: None },
        Some("probes") => Command::Probes,
        Some("gpu") => Command::Gpu,
        Some("status") => Command::Status,
        Some("games") => Command::Games,
        Some("runs") => Command::Runs,
        Some("stop") => Command::Stop { id: None },
        Some("play") => Command::Play { game: None },
        Some("press") => Command::Press { buttons: None },
        Some("screen") => Command::Screen { file: None },
        Some("perf") => Command::Perf,
        Some("logs") => Command::Logs,
        Some("tidy") => Command::Tidy,
        Some("nsp") => Command::Nsp { file: None },
        Some("release") => Command::Release { tag: None },
        Some("bisect") => Command::Bisect { game: None },
        Some(other) => return Err(ArgsError::Command(other.to_owned())),
    };
    let mut parsed = Args {
        command,
        config: None,
        keys: None,
        gpu: None,
        lang: None,
        timeout: None,
        only: None,
        games: Vec::new(),
        repeat: 1,
        ab: None,
        ab_runtime: None,
        runtime: None,
        good: None,
        bad: None,
        minutes: 30,
        hold_ms: 150,
        build_only: false,
        clean: false,
        apply: false,
        no_retry: false,
        diagnostics: Vec::new(),
        like: None,
        nsp: Vec::new(),
        rebuild: false,
        scenario: None,
        video: Video::Full,
        json: false,
        who: None,
        no_open: false,
        random: 0,
        seed: None,
        probes_dir: None,
    };
    while let Some(arg) = rest.next() {
        let mut value = |what: &'static str| {
            rest.next()
                .cloned()
                .ok_or_else(|| ArgsError::Value(arg.clone(), what))
        };
        match arg.as_str() {
            "--config" => parsed.config = Some(PathBuf::from(value("a file")?)),
            "--keys" => parsed.keys = Some(PathBuf::from(value("a file")?)),
            "--gpu" => {
                let text = value("wsl or software")?;
                parsed.gpu = Some(
                    Gpu::parse(&text)
                        .ok_or_else(|| ArgsError::Value(arg.clone(), "wsl or software"))?,
                );
            }
            "--video" => {
                let text = value("full, low or off")?;
                parsed.video = Video::parse(&text)
                    .ok_or_else(|| ArgsError::Value(arg.clone(), "full, low or off"))?;
            }
            "--lang" => {
                let text = value("ru or en")?;
                parsed.lang = Some(
                    Lang::parse(&text).ok_or_else(|| ArgsError::Value(arg.clone(), "ru or en"))?,
                );
            }
            "--timeout" => {
                let seconds: u32 = number(arg, value("seconds")?, "seconds above zero")?;
                if seconds == 0 {
                    return Err(ArgsError::Value(arg.clone(), "seconds above zero"));
                }
                parsed.timeout = Some(seconds);
            }
            "--only" => parsed.only = Some(value("a name")?),
            "--games" => {
                parsed.games = value("game names")?
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "--ab" => parsed.ab = Some(value("settings as JSON")?),
            "--runtime" => parsed.runtime = Some(PathBuf::from(value("a runtime archive")?)),
            "--good" => parsed.good = Some(value("a commit or tag")?),
            "--bad" => parsed.bad = Some(value("a commit or tag")?),
            "--ab-runtime" => {
                parsed.ab_runtime = Some(value("release:<tag> or a runtime archive")?)
            }
            "--scenario" => parsed.scenario = Some(value("standard or a scenario file")?),
            "--like" => parsed.like = Some(value("a run folder")?),
            "--nsp" => parsed.nsp.push(PathBuf::from(value("an NSP file")?)),
            "--rebuild" => parsed.rebuild = true,
            "--minutes" => parsed.minutes = number(arg, value("minutes")?, "minutes")?,
            "--hold" => {
                let seconds: f64 = number(arg, value("seconds")?, "seconds")?;
                if !(0.01..=60.0).contains(&seconds) {
                    return Err(ArgsError::Value(arg.clone(), "seconds from 0.01 to 60"));
                }
                parsed.hold_ms = Duration::from_secs_f64(seconds)
                    .as_millis()
                    .try_into()
                    .unwrap_or(u32::MAX);
            }
            "--repeat" => {
                let times: u32 = number(arg, value("a count")?, "a count from 1 to 100")?;
                if !(1..=100).contains(&times) {
                    return Err(ArgsError::Value(arg.clone(), "a count from 1 to 100"));
                }
                parsed.repeat = times;
            }
            "--who" => parsed.who = Some(value("a name")?),
            "--random" => parsed.random = number(arg, value("a count")?, "a count")?,
            "--seed" => parsed.seed = Some(number(arg, value("a number")?, "a number")?),
            "--probes-dir" => parsed.probes_dir = Some(PathBuf::from(value("a folder")?)),
            "--build-only" => parsed.build_only = true,
            "--clean" => parsed.clean = true,
            "--apply" => parsed.apply = true,
            "--no-retry" => parsed.no_retry = true,
            "--diag" => {
                parsed.diagnostics = diagnostics::parse_list(&value("diagnostics")?)
                    .map_err(|_| ArgsError::Value(arg.clone(), "names such as verbose,stacks"))?;
            }
            "--no-open" => parsed.no_open = true,
            "--json" => parsed.json = true,
            other if other.starts_with("--") => return Err(ArgsError::Option(other.to_owned())),
            other => match &mut parsed.command {
                Command::Run { set } if set.is_none() => *set = Some(other.to_owned()),
                Command::Stop { id } | Command::Release { tag: id } if id.is_none() => {
                    *id = Some(other.to_owned())
                }
                Command::Play { game } | Command::Bisect { game } if game.is_none() => {
                    *game = Some(other.to_owned())
                }
                Command::Press { buttons } if buttons.is_none() => {
                    *buttons = Some(other.to_owned())
                }
                Command::Screen { file } | Command::Nsp { file } if file.is_none() => {
                    *file = Some(PathBuf::from(other))
                }
                _ => return Err(ArgsError::Option(other.to_owned())),
            },
        }
    }
    Ok(parsed)
}

fn number<T: std::str::FromStr>(
    arg: &str,
    text: String,
    what: &'static str,
) -> Result<T, ArgsError> {
    text.parse()
        .map_err(|_| ArgsError::Value(arg.to_owned(), what))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn run_takes_a_set_and_overrides() {
        let parsed = parse(&args(
            "run quick --gpu software --lang en --only OpenTTD --timeout 900 --no-open",
        ))
        .unwrap();
        assert_eq!(
            parsed.command,
            Command::Run {
                set: Some("quick".to_owned())
            }
        );
        assert_eq!(parsed.gpu, Some(Gpu::Software));
        assert_eq!(parsed.lang, Some(Lang::En));
        assert_eq!(parsed.only.as_deref(), Some("OpenTTD"));
        assert_eq!(parsed.timeout, Some(900));
        assert!(parsed.no_open);
    }

    #[test]
    fn a_plain_run_takes_lab_json_as_it_is() {
        let parsed = parse(&args("run")).unwrap();
        assert_eq!(parsed.command, Command::Run { set: None });
        assert_eq!(parsed.gpu, None);
        assert_eq!(parsed.lang, None);
        assert_eq!(parsed.timeout, None);
        assert!(!parsed.build_only);
        assert!(!parsed.clean);
        assert!(parse(&args("run --clean")).unwrap().clean);
        assert_eq!(parse(&args("run")).unwrap().scenario, None);
        assert_eq!(parse(&args("run")).unwrap().video, Video::Full);
        assert_eq!(parse(&args("run --video low")).unwrap().video, Video::Low);
        assert!(parse(&args("run --video high")).is_err());
        assert_eq!(
            parse(&args("run --scenario standard"))
                .unwrap()
                .scenario
                .as_deref(),
            Some("standard")
        );
    }

    #[test]
    fn the_window_names_itself_and_stops_a_run_by_id() {
        let parsed = parse(&args("run nfsu --json --who window")).unwrap();
        assert_eq!(parsed.who.as_deref(), Some("window"));
        assert!(parsed.json);
        assert_eq!(
            parse(&args("stop 100-42")).unwrap().command,
            Command::Stop {
                id: Some("100-42".to_owned())
            }
        );
        assert_eq!(parse(&args("runs --json")).unwrap().command, Command::Runs);
    }

    #[test]
    fn a_plan_names_its_games_and_repeats() {
        let parsed = parse(&args("run --games OpenTTD,NFSU_menu --repeat 3")).unwrap();
        assert_eq!(parsed.games, ["OpenTTD", "NFSU_menu"]);
        assert_eq!(parsed.repeat, 3);
        assert_eq!(parse(&args("run")).unwrap().repeat, 1);
        assert!(matches!(
            parse(&args("run --repeat 0")),
            Err(ArgsError::Value(..))
        ));
    }

    #[test]
    fn claude_plays_presses_and_looks() {
        assert_eq!(
            parse(&args("play NFSU_menu --minutes 10")).unwrap().command,
            Command::Play {
                game: Some("NFSU_menu".to_owned())
            }
        );
        let press = parse(&args("press A+ZR --hold 0.5")).unwrap();
        assert_eq!(
            press.command,
            Command::Press {
                buttons: Some("A+ZR".to_owned())
            }
        );
        assert_eq!(press.hold_ms, 500);
        assert_eq!(
            parse(&args("screen shot.png")).unwrap().command,
            Command::Screen {
                file: Some(PathBuf::from("shot.png"))
            }
        );
    }

    #[test]
    fn no_arguments_mean_help() {
        assert_eq!(parse(&[]).unwrap().command, Command::Help);
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(
            parse(&args("fly")),
            Err(ArgsError::Command("fly".to_owned()))
        );
        assert_eq!(
            parse(&args("probes --fast")),
            Err(ArgsError::Option("--fast".to_owned()))
        );
        assert!(matches!(
            parse(&args("probes --gpu amd")),
            Err(ArgsError::Value(..))
        ));
        assert!(matches!(
            parse(&args("probes --timeout")),
            Err(ArgsError::Value(..))
        ));
        assert!(matches!(
            parse(&args("probes --timeout 0")),
            Err(ArgsError::Value(..))
        ));
        assert_eq!(
            parse(&args("probes extra")),
            Err(ArgsError::Option("extra".to_owned()))
        );
        assert_eq!(
            parse(&args("run quick slow")),
            Err(ArgsError::Option("slow".to_owned()))
        );
    }
}
