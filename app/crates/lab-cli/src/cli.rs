//! The command line of `carafe-lab`.

use std::path::PathBuf;

use thiserror::Error;

use lab_core::emulator::Gpu;
use lab_core::lang::Lang;

/// Help printed for `carafe-lab help` and after a command line error.
pub const HELP: &str = "\
carafe-lab: builds games and runs them in the Eden emulator

Usage:
  carafe-lab run [set] [options]   the enabled games of lab.json, or the games of a set
  carafe-lab probes [options]      the probe programs over a pairwise set of settings
  carafe-lab gpu [options]         what Vulkan the emulator container sees
  carafe-lab help

Options:
  --config <file>     default: lab.json in the repository
  --keys <file>       default: the file chosen in Carafe's settings
  --gpu wsl|software  overrides lab.json
  --lang ru|en        overrides lab.json
  --timeout <s>       seconds to wait for a first frame; overrides lab.json
  --only <name>       only the game or probe with this name
  --build-only        build the NSPs and stop
  --no-open           do not open the live page in the browser
  --json              print the run as JSON lines, one event each, without the live page
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
    /// `--build-only`.
    pub build_only: bool,
    /// `--json`.
    pub json: bool,
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
        build_only: false,
        json: false,
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
            "--random" => parsed.random = number(arg, value("a count")?, "a count")?,
            "--seed" => parsed.seed = Some(number(arg, value("a number")?, "a number")?),
            "--probes-dir" => parsed.probes_dir = Some(PathBuf::from(value("a folder")?)),
            "--build-only" => parsed.build_only = true,
            "--no-open" => parsed.no_open = true,
            "--json" => parsed.json = true,
            other if other.starts_with("--") => return Err(ArgsError::Option(other.to_owned())),
            other => match &mut parsed.command {
                Command::Run { set } if set.is_none() => *set = Some(other.to_owned()),
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
