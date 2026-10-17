//! The thread stacks Eden's gdb stub gives: the addresses gdb cannot name get the module and offset from the module
//! map the stub printed with them.

/// Returns `text`, gdb's answer to `monitor get info` and `thread apply all bt`, without its prompts and with every
/// unnamed frame followed by `[module+0xoffset]`.
#[must_use]
pub fn annotate(text: &str) -> String {
    let modules = modules(text);
    text.lines()
        .map(|line| line.replace("(gdb) ", ""))
        .map(|line| {
            let address = line
                .split_whitespace()
                .find(|word| word.starts_with("0x"))
                .filter(|_| line.contains(" in ?? ()"))
                .and_then(|word| u64::from_str_radix(word.trim_start_matches("0x"), 16).ok());
            match address.and_then(|address| place(&modules, address)) {
                Some(place) => format!("{line} [{place}]"),
                None => line,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn modules(text: &str) -> Vec<(u64, u64, String)> {
    text.lines()
        .filter_map(|line| {
            let (range, name) = line
                .trim()
                .split_once(' ')
                .map(|(start, rest)| (start, rest.trim()))?;
            let (end, name) = name.strip_prefix("- ")?.split_once(' ')?;
            let parse = |hex: &str| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok();
            Some((parse(range)?, parse(end)?, name.trim().to_owned()))
        })
        .collect()
}

fn place(modules: &[(u64, u64, String)], address: u64) -> Option<String> {
    modules
        .iter()
        .find(|(start, end, _)| (*start..=*end).contains(&address))
        .map(|(start, _, name)| format!("{name}+0x{:x}", address - start))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
(gdb) Process:     0x51 (Application)
Modules:
  0x00ca929000 - 0x00cc286fff wine-nx-runtime
  0x1000ebb000 - 0x1000ed8fff carafe-loader
(gdb)   Id   Target Id                                           Frame
* 1    Thread 76 \"Thread 76\" (Runnable)                    0x00000000cc21a56c in ?? ()
Thread 27 (Thread 195 \"Thread 195\" (Waiting (ConditionVar))):
#0  0x00000000cc21a56c in ?? ()
#1  0x000000308901fbd0 in ?? ()
";

    #[test]
    fn unnamed_frames_get_their_module_and_offset() {
        let annotated = annotate(TEXT);
        assert!(!annotated.contains("(gdb)"));
        assert!(annotated.contains("#0  0x00000000cc21a56c in ?? () [wine-nx-runtime+0x18f156c]"));
        assert!(annotated.contains("#1  0x000000308901fbd0 in ?? ()\n"));
        assert!(annotated.contains(
            "(Runnable)                    0x00000000cc21a56c in ?? () [wine-nx-runtime+0x18f156c]"
        ));
    }
}
