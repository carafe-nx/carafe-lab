//! Starting `carafe-lab` so that it outlives the window: outside the window's process tree and its job object, with
//! its output and exit code in a log file.

use std::path::Path;
use std::process::Command;

/// Prefix of the last line the wrapper adds to the log, followed by the exit code.
pub const EXIT_MARK: &str = "carafe-lab-exit ";

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const WMI_CREATE: &str = "$si = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly \
    -Property @{ ShowWindow = [uint16]0 }; \
    $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create \
    -Arguments @{ CommandLine = $env:LAB_COMMAND; CurrentDirectory = $env:LAB_DIR; ProcessStartupInformation = $si }; \
    exit $r.ReturnValue";

/// Returns the line `cmd.exe` runs: `program` with `args`, its output into `log`, then [`EXIT_MARK`] and the exit code.
#[must_use]
pub fn wrapper_line(program: &Path, args: &[String], log: &Path) -> String {
    let mut line = format!("\"{}\"", program.display());
    for arg in args {
        line.push_str(&format!(" \"{arg}\""));
    }
    let log = log.display();
    format!("/d /s /c \"{line} > \"{log}\" 2>&1 & call echo {EXIT_MARK}%^errorlevel%>> \"{log}\"\"")
}

/// Starts `program` with `args` in `dir`, detached from the window, writing its output to `log`.
///
/// The process leaves the window's job object when the job allows it; otherwise Windows' management service starts
/// it, which puts it outside the job.
///
/// # Errors
///
/// A message when neither way starts it.
pub fn spawn(program: &Path, args: &[String], dir: &Path, log: &Path) -> Result<(), String> {
    let line = wrapper_line(program, args, log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let flags = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
        let away = Command::new("cmd.exe")
            .raw_arg(&line)
            .current_dir(dir)
            .creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB)
            .spawn();
        if away.is_ok() {
            return Ok(());
        }
        let command_line = format!("cmd.exe {line}");
        let created = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", WMI_CREATE])
            .env("LAB_COMMAND", command_line)
            .env("LAB_DIR", dir)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|error| error.to_string())?;
        if created.success() {
            Ok(())
        } else {
            Err(format!(
                "Windows did not start carafe-lab (code {})",
                created.code().unwrap_or(-1)
            ))
        }
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .arg("-c")
            .arg(format!(
                "\"{}\" {} > \"{}\" 2>&1; echo {EXIT_MARK}$? >> \"{}\"",
                program.display(),
                args.iter()
                    .map(|arg| format!("\"{arg}\""))
                    .collect::<Vec<_>>()
                    .join(" "),
                log.display(),
                log.display()
            ))
            .current_dir(dir)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// Returns the exit code the wrapper wrote at the end of `log`, or `None` while the program runs.
#[must_use]
pub fn exit_code(log: &str) -> Option<i32> {
    log.lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(EXIT_MARK))
        .and_then(|code| code.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wrapper_quotes_every_part_and_appends_the_exit_code() {
        let line = wrapper_line(
            Path::new("D:\\Lab\\carafe-lab.exe"),
            &["run".to_owned(), "NFSU menu".to_owned()],
            Path::new("D:\\Lab\\work\\run.log"),
        );
        assert_eq!(
            line,
            "/d /s /c \"\"D:\\Lab\\carafe-lab.exe\" \"run\" \"NFSU menu\" > \"D:\\Lab\\work\\run.log\" 2>&1 \
             & call echo carafe-lab-exit %^errorlevel%>> \"D:\\Lab\\work\\run.log\"\""
        );
    }

    #[test]
    fn the_exit_code_is_read_from_the_last_mark() {
        assert_eq!(exit_code("{\"event\":\"runStarted\"}\n"), None);
        assert_eq!(
            exit_code("carafe-lab: no games\ncarafe-lab-exit 1 \r\n"),
            Some(1)
        );
    }
}
