# 8. NCE runs the guest's code by default

Date: 2026-10-30.

## Context

Under dynarmic Eden translates the ARM64 code FEX makes from the game's x86 code a second time, and the lab ran it 4–8
times slower than the console's Cortex-A57 ([lab-speed](../research/lab-speed/REPORT.md)). Eden's NCE runs the guest's
ARM64 code natively, but it never ran for a Carafe game: Eden enabled it only for 39-bit address spaces, and it lacked
what Carafe's runtime needs. Patches `0052`–`0055` add that ([lab-nce](../research/lab-nce/ANALYSIS.md)): guest
addresses equal host addresses from 0x200000 up, code the runtime maps itself is patched, FEX's `CNTPCT_EL0` reads and
`brk` traps are handled, and faults reach the runtime's exception handler. The lab's `carafe-sigchain` became free of
thread-local storage, which NCE's guest `TPIDR_EL0` broke.

Every enabled lab game was run under NCE on 2026-10-30 with its standard scenario, on a PC held to a 22 W power limit:

| Game | First picture, NCE | First picture, earlier dynarmic runs | Median fps, NCE | Earlier dynarmic |
|---|---|---|---|---|
| OpenTTD | 29 s | 27–55 s | 41 | 8–24 |
| Need for Speed Underground | 30 s | 47 s | 55 | 25 |
| Heroes of Might and Magic IV | 7 s | 12–16 s | 13 | 15 (the game's own cap) |
| Need for Speed Underground 2 | 20–37 s | 47–68 s | race start 17–29 under throttling | 20–27 |

All passed with the same warnings as under dynarmic (the registry rewritten after a restart, HoMM IV's screen moving on
its own), and the pictures were right.

## Decision

- `emulatorCpu` defaults to `nce`; `dynarmic` stays available.
- A speed comparison that has to keep the console's proportions between CPU and GPU work uses `dynarmic`: under NCE the
  lab's CPU is faster than the console's, so a CPU-bound slowdown that shows on the console can hide in the lab.

## Open

- Two NFSU2 runs of about ten sat on the race's loading screen for minutes with the game thread busy and did not close;
  the cause is not known.
- Need for Speed Carbon, Dead Space and the probes were not run under NCE: they are disabled in `lab.json`.
- Writes to pages the GPU caches cost a signal each, about 90–180 µs, roughly 90 a frame in a race.

## Rejected

- Eden on Windows with Qualcomm's Vulkan driver: Eden's NCE is Linux and Android only, so it would bring back dynarmic.
- Keeping dynarmic the default until every disabled game is checked: the enabled set is what every run uses, and each
  of its games runs faster and passes under NCE.
