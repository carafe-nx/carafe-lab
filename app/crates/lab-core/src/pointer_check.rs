//! Checking the screen probe's log: how far the mouse got across a window at another resolution than the console's.

const EDGE_SLACK: i32 = 2;

/// The part of the probe's window the mouse moves reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// The window's width, as the probe measured it after the mode change.
    pub width: i32,
    /// The window's height.
    pub height: i32,
    /// The smallest and largest x of a move in the window.
    pub x: (i32, i32),
    /// The smallest and largest y of a move in the window.
    pub y: (i32, i32),
}

impl Reach {
    /// Returns whether the moves reached all four edges of the window, give or take two pixels.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.x.0 <= EDGE_SLACK
            && self.y.0 <= EDGE_SLACK
            && self.x.1 >= self.width - 1 - EDGE_SLACK
            && self.y.1 >= self.height - 1 - EDGE_SLACK
    }

    /// Returns the reach as text, such as `x 0…1023, y 0…767 of 1024×768`.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "x {}…{}, y {}…{} of {}×{}",
            self.x.0, self.x.1, self.y.0, self.y.1, self.width, self.height
        )
    }
}

/// Reads the probe's `size` line and its `move client` lines.
///
/// # Errors
///
/// A sentence for the report when the log has no size or no moves.
pub fn check(log: &str) -> Result<Reach, String> {
    let mut size = None;
    let mut x: Option<(i32, i32)> = None;
    let mut y: Option<(i32, i32)> = None;
    for line in log.lines() {
        let text = line.split_once(' ').map_or(line, |(_, rest)| rest);
        if let Some(rest) = text.strip_prefix("size ") {
            size = parse_pair(rest, 'x');
        } else if let Some(rest) = text.strip_prefix("move client ")
            && let Some((at_x, at_y)) = parse_pair(rest, ',')
        {
            x = Some(x.map_or((at_x, at_x), |(low, high)| (low.min(at_x), high.max(at_x))));
            y = Some(y.map_or((at_y, at_y), |(low, high)| (low.min(at_y), high.max(at_y))));
        }
    }
    let (width, height) = size.ok_or("the probe logged no window size")?;
    let (Some(x), Some(y)) = (x, y) else {
        return Err("the probe got no mouse moves".to_owned());
    };
    Ok(Reach {
        width,
        height,
        x,
        y,
    })
}

fn parse_pair(text: &str, separator: char) -> Option<(i32, i32)> {
    let (first, second) = text.split_once(separator)?;
    let second = second.split_whitespace().next()?;
    Some((first.trim().parse().ok()?, second.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_to_every_edge_pass() {
        let log = "\
0.001 probe screen ready, 32-bit, log C:\\probe-screen.log
0.400 size 1024x768
2.000 move client 512,384
4.000 move client 0,0
9.000 move client 1023,767
";
        let reach = check(log).unwrap();
        assert!(reach.ok());
        assert_eq!(reach.describe(), "x 0…1023, y 0…767 of 1024×768");
    }

    #[test]
    fn moves_that_stop_short_of_the_right_edge_fail() {
        let log = "\
0.400 size 1024x768
4.000 move client 0,0
9.000 move client 920,767
";
        let reach = check(log).unwrap();
        assert!(!reach.ok());
        assert_eq!(reach.x, (0, 920));
    }

    #[test]
    fn a_log_without_size_or_moves_is_an_error() {
        assert!(check("2.000 move client 1,1\n").is_err());
        assert!(check("0.400 size 1024x768\n").is_err());
    }

    #[test]
    fn edges_have_two_pixels_of_slack() {
        let reach = Reach {
            width: 1024,
            height: 768,
            x: (2, 1021),
            y: (1, 765),
        };
        assert!(reach.ok());
    }
}
