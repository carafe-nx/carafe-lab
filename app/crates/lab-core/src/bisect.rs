//! Finding the Carafe commit that broke a game: the commits between one where it worked and one where it did not are
//! halved until one commit is left.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What a commit's runs said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "BisectVerdict")]
pub enum Verdict {
    /// The game worked as at the good commit.
    Good,
    /// The game failed as at the bad commit.
    Bad,
    /// The commit could not be built or run, so it tells nothing.
    Skip,
}

/// Where a bisection stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "state")]
#[ts(export, rename = "BisectNext")]
pub enum Next {
    /// This commit is to be built and run next.
    #[serde(rename_all = "camelCase")]
    Test {
        /// The commit.
        commit: String,
        /// Commits still in question, the ones skipped included.
        left: usize,
    },
    /// The first bad commit is found.
    #[serde(rename_all = "camelCase")]
    Found {
        /// The commit.
        commit: String,
    },
    /// Only skipped commits are left between the last good and the first bad one, so any of them may be it.
    #[serde(rename_all = "camelCase")]
    Unsure {
        /// The commits, oldest first, the first bad one last.
        commits: Vec<String>,
    },
}

/// Returns what to do next to find the first bad commit of `commits`, oldest first: the good commit, the commits after
/// it and the bad commit last; `verdicts` are the commits tested so far.
#[must_use]
pub fn next(commits: &[String], verdicts: &[(String, Verdict)]) -> Next {
    let verdict_of = |commit: &String| {
        verdicts
            .iter()
            .rev()
            .find(|(tested, _)| tested == commit)
            .map(|(_, verdict)| *verdict)
    };
    let last_good = commits
        .iter()
        .rposition(|commit| verdict_of(commit) == Some(Verdict::Good))
        .unwrap_or(0);
    let first_bad = commits
        .iter()
        .enumerate()
        .skip(last_good + 1)
        .find(|(_, commit)| verdict_of(commit) == Some(Verdict::Bad))
        .map_or(commits.len().saturating_sub(1), |(index, _)| index);
    let open: Vec<usize> = (last_good + 1..first_bad)
        .filter(|index| verdict_of(&commits[*index]).is_none())
        .collect();
    if open.is_empty() {
        let between: Vec<String> =
            commits[last_good + 1..=first_bad.min(commits.len() - 1)].to_vec();
        return match between.as_slice() {
            [only] => Next::Found {
                commit: only.clone(),
            },
            _ => Next::Unsure { commits: between },
        };
    }
    let middle = open[open.len() / 2];
    Next::Test {
        commit: commits[middle].clone(),
        left: first_bad - last_good - 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commits(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("c{index}")).collect()
    }

    fn run(commits: &[String], first_bad: usize, skipped: &[usize]) -> (Next, usize) {
        let mut verdicts = Vec::new();
        let mut tests = 0;
        loop {
            match next(commits, &verdicts) {
                Next::Test { commit, .. } => {
                    tests += 1;
                    let index: usize = commit[1..].parse().unwrap();
                    let verdict = if skipped.contains(&index) {
                        Verdict::Skip
                    } else if index >= first_bad {
                        Verdict::Bad
                    } else {
                        Verdict::Good
                    };
                    verdicts.push((commit, verdict));
                }
                done => return (done, tests),
            }
        }
    }

    #[test]
    fn the_first_bad_commit_is_found_in_a_logarithm_of_tests() {
        let list = commits(17);
        for first_bad in 1..17 {
            let (found, tests) = run(&list, first_bad, &[]);
            assert_eq!(
                found,
                Next::Found {
                    commit: format!("c{first_bad}")
                }
            );
            assert!(tests <= 4, "{first_bad}: {tests} tests");
        }
    }

    #[test]
    fn skipped_commits_leave_the_answer_open_only_where_they_stand() {
        let list = commits(9);
        let (found, _) = run(&list, 5, &[2]);
        assert_eq!(
            found,
            Next::Found {
                commit: "c5".to_owned()
            }
        );
        let (unsure, _) = run(&list, 5, &[4]);
        assert_eq!(
            unsure,
            Next::Unsure {
                commits: vec!["c4".to_owned(), "c5".to_owned()]
            }
        );
    }

    #[test]
    fn neighbours_need_no_test() {
        assert_eq!(
            next(&commits(2), &[]),
            Next::Found {
                commit: "c1".to_owned()
            }
        );
    }
}
