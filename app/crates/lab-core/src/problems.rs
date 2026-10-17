//! Errors and warnings in a launch's logs, grouped by their text.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const TEXT_LIMIT: usize = 200;

/// How bad a log line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Severity {
    /// Something may be wrong.
    Warning,
    /// Something went wrong.
    Error,
}

/// Lines of one log that say the same, with the number of times they came.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Problem {
    /// Where the lines come from: `eden`, `carafe`, `runtime`, `dxvk` or `game`.
    pub source: String,
    /// How bad they are.
    pub severity: Severity,
    /// The first of the lines, without its time stamp.
    pub text: String,
    /// How many lines say the same once numbers are left out.
    pub count: u32,
}

/// How many error and warning lines a launch's logs have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProblemCounts {
    /// Error lines.
    pub errors: u32,
    /// Warning lines.
    pub warnings: u32,
}

/// Returns the source a log file belongs to, from its name.
#[must_use]
pub fn source_of(file_name: &str) -> &'static str {
    let name = file_name.to_ascii_lowercase();
    if name.starts_with("eden_log") {
        "eden"
    } else if name.starts_with("carafe-overlay") {
        "carafe"
    } else if name.starts_with("autorun_runtime") {
        "runtime"
    } else if ["_d3d", "_dxgi", "_d3d9", "_d3d11", "dxvk"]
        .iter()
        .any(|part| name.contains(part))
    {
        "dxvk"
    } else {
        "game"
    }
}

/// Returns how bad `line` is and its text without the time stamp; `None` for a line that is neither an error nor a
/// warning.
///
/// Eden's `<Error>`, `<Critical>` and `<Warning>`, Wine's and DXVK's `err:` and `warn:` and Mesa's warnings count;
/// Wine's `fixme:` does not.
#[must_use]
pub fn classify(line: &str) -> Option<(Severity, String)> {
    let text = strip_stamp(line.trim());
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let severity = if text.contains("<Error>") || text.contains("<Critical>") {
        Severity::Error
    } else if text.contains("<Warning>") {
        Severity::Warning
    } else if lower.starts_with("err:") || lower.contains(":err:") {
        Severity::Error
    } else if lower.starts_with("warn:")
        || lower.contains(":warn:")
        || (lower.starts_with("mesa") && (lower.contains("warn") || lower.contains("fail")))
    {
        Severity::Warning
    } else {
        return None;
    };
    Some((severity, text.chars().take(TEXT_LIMIT).collect()))
}

/// Returns the problems in the text of one log from `source`, in the order they first came.
#[must_use]
pub fn problems(source: &str, text: &str) -> Vec<Problem> {
    let mut found: Vec<(String, Problem)> = Vec::new();
    for (severity, line) in text.lines().filter_map(classify) {
        let key = shape(&line);
        match found
            .iter_mut()
            .find(|(known, problem)| *known == key && problem.severity == severity)
        {
            Some((_, problem)) => problem.count += 1,
            None => found.push((
                key,
                Problem {
                    source: source.to_owned(),
                    severity,
                    text: line,
                    count: 1,
                },
            )),
        }
    }
    found.into_iter().map(|(_, problem)| problem).collect()
}

/// Returns how many error and warning lines `problems` stand for.
#[must_use]
pub fn count(problems: &[Problem]) -> ProblemCounts {
    problems
        .iter()
        .fold(ProblemCounts::default(), |mut counts, problem| {
            match problem.severity {
                Severity::Error => counts.errors += problem.count,
                Severity::Warning => counts.warnings += problem.count,
            }
            counts
        })
}

/// Returns `lists` as one list: problems that say the same have their counts added, in the order they first came.
#[must_use]
pub fn merge(lists: impl IntoIterator<Item = Vec<Problem>>) -> Vec<Problem> {
    let mut merged: Vec<(String, Problem)> = Vec::new();
    for problem in lists.into_iter().flatten() {
        let key = key(&problem);
        match merged.iter_mut().find(|(known, _)| *known == key) {
            Some((_, known)) => known.count += problem.count,
            None => merged.push((key, problem)),
        }
    }
    merged.into_iter().map(|(_, problem)| problem).collect()
}

/// Returns the problems of `now` that `before` does not have, from the same source with the same severity and the
/// same text once numbers are left out.
#[must_use]
pub fn new_since(now: &[Problem], before: &[Problem]) -> Vec<Problem> {
    let known: Vec<String> = before.iter().map(key).collect();
    now.iter()
        .filter(|problem| !known.contains(&key(problem)))
        .cloned()
        .collect()
}

fn key(problem: &Problem) -> String {
    format!(
        "{}|{:?}|{}",
        problem.source,
        problem.severity,
        shape(&problem.text)
    )
}

fn strip_stamp(line: &str) -> &str {
    match line.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
        Some((stamp, rest)) if stamp.trim().chars().all(|c| c.is_ascii_digit() || c == '.') => {
            rest.trim_start()
        }
        _ => line,
    }
}

fn shape(line: &str) -> String {
    let mut shaped = String::with_capacity(line.len());
    let mut in_number = false;
    for c in line.chars() {
        if c.is_ascii_hexdigit() && (c.is_ascii_digit() || in_number) {
            if !in_number {
                shaped.push('#');
                in_number = true;
            }
        } else {
            in_number = false;
            shaped.push(c);
        }
    }
    shaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eden_wine_dxvk_and_mesa_lines_are_told_apart() {
        let eden = "[   5.208010] Debug <Critical> core/hle/service/service.cpp:71: assert Unknown function";
        assert_eq!(
            classify(eden),
            Some((
                Severity::Error,
                "Debug <Critical> core/hle/service/service.cpp:71: assert Unknown function"
                    .to_owned()
            ))
        );
        assert_eq!(
            classify("[ 1.5] Service.FS <Warning> GetFileTimeStampRaw: called")
                .map(|(severity, _)| severity),
            Some(Severity::Warning)
        );
        assert_eq!(
            classify("0024:err:module:import_dll Library d3dx9_43.dll not found")
                .map(|(severity, _)| severity),
            Some(Severity::Error)
        );
        assert_eq!(
            classify("warn:  DXGI: MakeWindowAssociation: Ignoring flags")
                .map(|(severity, _)| severity),
            Some(Severity::Warning)
        );
        assert_eq!(
            classify("MESA: warning: unsupported format").map(|(severity, _)| severity),
            Some(Severity::Warning)
        );
        assert_eq!(classify("0024:fixme:ntdll:NtQuerySystemInformation"), None);
        assert_eq!(classify("[MAP] start-up: heap region 3080200000"), None);
    }

    #[test]
    fn lines_differing_only_in_numbers_are_one_problem() {
        let text = "\
0024:err:module:load 0x1000
0030:err:module:load 0x2fff
0024:warn:seh:raise 7
0024:err:module:load 0x3000
";
        let found = problems("runtime", text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].count, 3);
        assert_eq!(found[0].text, "0024:err:module:load 0x1000");
        assert_eq!(
            count(&found),
            ProblemCounts {
                errors: 3,
                warnings: 1
            }
        );
    }

    #[test]
    fn new_problems_are_the_ones_the_run_before_lacked() {
        let first = problems(
            "runtime",
            "0024:err:module:load 0x1000\n0024:warn:seh:raise 7\n",
        );
        let second = problems(
            "runtime",
            "0031:err:module:load 0x2000\n0031:err:d3d:create failed\n",
        );
        let fresh = new_since(&second, &first);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].text, "0031:err:d3d:create failed");
        let merged = merge([first.clone(), second]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].count, 2);
    }

    #[test]
    fn sources_come_from_file_names() {
        assert_eq!(source_of("eden_log.txt"), "eden");
        assert_eq!(source_of("carafe-overlay.log"), "carafe");
        assert_eq!(source_of("autorun_runtime.log"), "runtime");
        assert_eq!(source_of("speed_d3d9.log"), "dxvk");
        assert_eq!(source_of("openttd.log"), "game");
    }
}
