//! A game's crash as Eden logs it: the fault, the registers, and the stack with the module of every frame.

/// The file in a launch's folder with the crash Eden logged, as Eden wrote it.
pub const FILE: &str = "crash.txt";

const BACKTRACE: &str = "LogBacktrace: Backtrace @ PC=";
const MODULE_TABLE: &str = "Module ";

/// A crash found in Eden's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crash {
    /// What went wrong, as Eden says it, such as `Unhandled read of unmapped memory at 0x3fff8dcd00`.
    pub fault: String,
    /// The stack, innermost frame first.
    pub frames: Vec<Frame>,
    /// The fault, the registers and the stack as Eden logged them.
    pub text: String,
}

/// A frame of a crashed thread's stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The module the frame's code belongs to.
    pub module: String,
    /// The frame's offset in that module, in hex.
    pub offset: String,
    /// The symbol Eden found for it; empty when none.
    pub symbol: String,
}

impl Crash {
    /// Returns the crash in one line: the fault and the innermost frame.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.frames.first() {
            Some(frame) if frame.symbol.is_empty() => {
                format!("{} in {}+0x{}", self.fault, frame.module, frame.offset)
            }
            Some(frame) => format!("{} in {} ({})", self.fault, frame.module, frame.symbol),
            None => self.fault.clone(),
        }
    }
}

/// Returns the first crash in the Eden log `log`; `None` when it logged no backtrace.
#[must_use]
pub fn find(log: &str) -> Option<Crash> {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.iter().position(|line| line.contains(BACKTRACE))?;
    let fault_line = start
        .checked_sub(1)
        .map(|index| lines[index])
        .filter(|line| line.contains("<Critical>"));
    let fault = fault_line
        .and_then(|line| line.rsplit_once(": "))
        .map_or_else(
            || "the game crashed".to_owned(),
            |(_, text)| text.trim().to_owned(),
        );
    let mut end = start + 1;
    let mut frames = Vec::new();
    let mut in_table = false;
    while end < lines.len() {
        let line = lines[end];
        if line.trim().is_empty() || line.starts_with('[') {
            break;
        }
        if line.starts_with(MODULE_TABLE) {
            in_table = true;
        } else if in_table {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let [module, _address, _original, offset, rest @ ..] = parts.as_slice() {
                frames.push(Frame {
                    module: (*module).to_owned(),
                    offset: offset.trim_start_matches('0').to_owned(),
                    symbol: rest.join(" "),
                });
            }
        }
        end += 1;
    }
    let first = if fault_line.is_some() {
        start - 1
    } else {
        start
    };
    Some(Crash {
        fault,
        frames,
        text: lines[first..end].join("\n") + "\n",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
[  61.700000] Core.ARM <Info> something before
[  61.707755] Core.ARM <Critical> core/hle/kernel/physical_core.cpp:188:RunThread: Unhandled read of unmapped memory at 0x3fff8dcd00
[  61.708973] Core.ARM <Error> core/arm/arm_interface.cpp:43:LogBacktrace: Backtrace @ PC=0000001CA2EF42D0
R00=0000003FFF8DCD00 R01=000000000022CD00 R02=00000000FFFFFFFF R03=0000000000000000
PSTATE=20000000 FPCR=00000000 FPSR=08000010 TPIDR=0000000000000000
Module              Address             Original Address    Offset              Symbol
wine-nx-runtime     00000071EF8B52D0    0000001CA2EF42D0    00000000EF8B52D0
wine-nx-runtime     00000071EF8C3B34    0000001CA2F02B34    00000000EF8C3B34

[  61.800000] Core <Info> after
";

    #[test]
    fn a_crash_tells_its_fault_and_the_module_of_every_frame() {
        let crash = find(LOG).unwrap();
        assert_eq!(
            crash.fault,
            "Unhandled read of unmapped memory at 0x3fff8dcd00"
        );
        assert_eq!(crash.frames.len(), 2);
        assert_eq!(crash.frames[0].module, "wine-nx-runtime");
        assert_eq!(crash.frames[0].offset, "EF8B52D0");
        assert_eq!(
            crash.summary(),
            "Unhandled read of unmapped memory at 0x3fff8dcd00 in wine-nx-runtime+0xEF8B52D0"
        );
        assert!(crash.text.starts_with("[  61.707755]"));
        assert!(crash.text.contains("PSTATE=20000000"));
        assert!(!crash.text.contains("after"));
    }

    #[test]
    fn a_log_without_a_backtrace_has_no_crash() {
        assert_eq!(find("[ 1.0] Core <Info> fine\n"), None);
    }
}
