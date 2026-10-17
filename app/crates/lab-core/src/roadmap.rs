//! The open items of `ROADMAP.md` that mention a game.

use serde::Serialize;
use ts_rs::TS;

/// An open item of the roadmap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RoadmapItem {
    /// Its tag: `bug`, `game`, `feature` or `research`.
    pub tag: String,
    /// Its text without the tag and the bold marks.
    pub text: String,
    /// The item is in progress.
    pub doing: bool,
}

/// Returns the open items of `roadmap` whose text has any of `names` in it, ignoring case.
///
/// # Arguments
///
/// * `roadmap` — the text of `ROADMAP.md`.
/// * `names` — the words that stand for the game; empty ones are skipped.
///
/// # Returns
///
/// The items in file order; empty when none mentions the game.
#[must_use]
pub fn open_items_mentioning(roadmap: &str, names: &[&str]) -> Vec<RoadmapItem> {
    let names: Vec<String> = names
        .iter()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    roadmap
        .lines()
        .filter_map(open_item)
        .filter(|item| {
            let text = item.text.to_lowercase();
            names.iter().any(|name| text.contains(name.as_str()))
        })
        .collect()
}

fn open_item(line: &str) -> Option<RoadmapItem> {
    let rest = line.trim_start().strip_prefix("- [ ] ")?;
    let doing = rest.starts_with("**");
    let rest = rest.replace("**", "");
    let rest = rest.trim();
    let (tag, text) = rest
        .strip_prefix('`')
        .and_then(|tail| tail.split_once('`'))
        .map_or(("", rest), |(tag, text)| (tag, text.trim()));
    Some(RoadmapItem {
        tag: tag.to_owned(),
        text: text.to_owned(),
        doing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROADMAP: &str = "\
## Planned

- [ ] `feature` The one Eden set up like the console
- [ ] **`bug` WSL GPU: OpenTTD's picture flickers**
- [ ] `game` Need for Speed Underground crashes in EA's stream ring buffer

## Done

- [x] `bug` OpenTTD hangs at start
";

    #[test]
    fn open_items_with_the_name_are_found_and_done_ones_left_out() {
        let items = open_items_mentioning(ROADMAP, &["openttd"]);
        assert_eq!(
            items,
            [RoadmapItem {
                tag: "bug".to_owned(),
                text: "WSL GPU: OpenTTD's picture flickers".to_owned(),
                doing: true,
            }]
        );
    }

    #[test]
    fn any_of_the_names_is_enough_and_empty_names_match_nothing() {
        let items =
            open_items_mentioning(ROADMAP, &["", "NFSU menu", "Need for Speed Underground"]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].tag, "game");
        assert!(!items[0].doing);
        assert!(open_items_mentioning(ROADMAP, &[" "]).is_empty());
    }
}
