//! Checking the threads probe's log: how many threads the program could hold at each point of its run, and whether
//! Direct3D 9 lived through a thread it was refused.

/// What the threads probe wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadsLog {
    /// The threads the probe could hold, with the point of the run it measured them at, in log order.
    pub ceilings: Vec<(String, u32)>,
    /// `Some(true)` when a second Direct3D 9 device was created with every thread taken and the probe went on,
    /// `Some(false)` when it was tried and the probe ended there, `None` when it was not tried.
    pub dxvk_survived: Option<bool>,
    /// The probe wrote its last line.
    pub finished: bool,
}

impl ThreadsLog {
    /// Reads the probe's `ceiling`, `second device` and `exit` lines.
    #[must_use]
    pub fn read(log: &str) -> Self {
        let mut read = Self::default();
        for line in log.lines() {
            let text = line.split_once(' ').map_or(line, |(_, text)| text);
            if let Some(rest) = text.strip_prefix("ceiling ")
                && let Some((point, count)) = rest.split_once(": ")
                && let Some(count) = count.split_whitespace().next()
                && let Ok(count) = count.parse()
            {
                read.ceilings.push((point.to_owned(), count));
            } else if text == "second device with every thread taken" {
                read.dxvk_survived = Some(false);
            } else if text.starts_with("second device survived") {
                read.dxvk_survived = Some(true);
            } else if text == "exit" {
                read.finished = true;
            }
        }
        read
    }

    /// Checks the log against `min`, the fewest threads the probe must be able to hold at every point.
    ///
    /// # Errors
    ///
    /// What failed: no ceiling at all, a ceiling below `min`, Direct3D 9 dying on a refused thread, or a probe that
    /// did not finish.
    pub fn check(&self, min: u32) -> Result<String, String> {
        let Some(lowest) = self.ceilings.iter().min_by_key(|(_, count)| *count) else {
            return Err("the probe wrote no ceiling".to_owned());
        };
        let highest = self
            .ceilings
            .iter()
            .map(|(_, count)| *count)
            .max()
            .unwrap_or(0);
        if lowest.1 < min {
            return Err(format!(
                "{} threads {}, fewer than {min}",
                lowest.1, lowest.0
            ));
        }
        if self.dxvk_survived == Some(false) {
            return Err("Direct3D 9 ended the probe when a thread was refused".to_owned());
        }
        if !self.finished {
            return Err("the probe did not finish".to_owned());
        }
        let range = if lowest.1 == highest {
            format!("{highest}")
        } else {
            format!("{}–{highest}", lowest.1)
        };
        let dxvk = if self.dxvk_survived == Some(true) {
            ", Direct3D 9 lived through a refused thread"
        } else {
            ""
        };
        Ok(format!(
            "{range} threads at {} points of the run{dxvk}",
            self.ceilings.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
0.036 probe threads ready, 32-bit, log C:\\Games\\probe-threads-x86\\probe-threads.log
1.873 ceiling at start: 35 threads, started 35, refused with error 8
14.824 ceiling after detached churn: 31 threads, started 31, refused with error 8
27.430 ceiling with a Direct3D 9 device: 30 threads, started 30, refused with error 8
";

    #[test]
    fn ceilings_are_read_in_order() {
        let read = ThreadsLog::read(LOG);
        assert_eq!(
            read.ceilings,
            vec![
                ("at start".to_owned(), 35),
                ("after detached churn".to_owned(), 31),
                ("with a Direct3D 9 device".to_owned(), 30),
            ]
        );
        assert_eq!(read.dxvk_survived, None);
        assert!(!read.finished);
    }

    #[test]
    fn a_finished_probe_above_the_minimum_passes() {
        let read = ThreadsLog::read(&format!("{LOG}30.000 exit\n"));
        assert_eq!(
            read.check(30),
            Ok("30–35 threads at 3 points of the run".to_owned())
        );
    }

    #[test]
    fn the_lowest_ceiling_must_reach_the_minimum() {
        let read = ThreadsLog::read(&format!("{LOG}30.000 exit\n"));
        assert_eq!(
            read.check(31),
            Err("30 threads with a Direct3D 9 device, fewer than 31".to_owned())
        );
    }

    #[test]
    fn direct3d_must_live_through_a_refusal() {
        let died = ThreadsLog::read(&format!(
            "{LOG}27.436 second device with every thread taken\n"
        ));
        assert_eq!(
            died.check(30),
            Err("Direct3D 9 ended the probe when a thread was refused".to_owned())
        );
        let lived = ThreadsLog::read(&format!(
            "{LOG}27.436 second device with every thread taken\n27.500 second device survived: 0x8876086a\n\
             30.000 exit\n"
        ));
        assert_eq!(
            lived.check(30),
            Ok(
                "30–35 threads at 3 points of the run, Direct3D 9 lived through a refused thread"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_probe_that_stopped_fails() {
        assert_eq!(
            ThreadsLog::read(LOG).check(30),
            Err("the probe did not finish".to_owned())
        );
        assert_eq!(
            ThreadsLog::read("").check(30),
            Err("the probe wrote no ceiling".to_owned())
        );
    }
}
