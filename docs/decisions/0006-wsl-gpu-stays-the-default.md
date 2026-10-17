# 6. The WSL GPU stays the default; software drawing confirms

Date: 2026-10-18.

## Context

[Decision 3](0003-carafe-lab.md) asked to run the set with software drawing and with the WSL GPU and keep the mode
that misreports less: a failure that is the emulator's and not the game's. Both modes work since the d3d12 fixes of
[decision 2](0002-wsl-gpu-patched-mesa.md) and [decision 4](0004-mesa-from-a-submodule.md).

OpenTTD ran five times in each mode with the standard scenario, no retries, one Eden for the run:

- Software: 4 with warnings, 1 failure. The failure was the runtime's known start-up stall at the loading screen,
  a real bug of the game's NSP, not of the emulator. First frame after 25–78 s; about 6 minutes a launch.
- WSL GPU: 4 with warnings, 1 failure. The failure was Eden itself: `SIGSEGV` early in the boot, next to the shader
  compiler's signal handler. First frame after 28–53 s; about 2.5 minutes a launch.

The warnings were the same in both: the registry rewritten after a restart and OpenTTD's title screen moving on its
own.

## Decision

- The WSL GPU stays the default in `lab.json`. It is 2.4 times faster, and its timing is closer to the console's.
- Its one misreport in five is not hidden: Lab reports an emulator crash as "the emulator crashed", apart from the
  game's own failures, and the failed game runs again with detailed logs.
- Software drawing (`--gpu software`) is for confirming a failure that only the WSL GPU shows.

## Rejected

- Software drawing as the default: it misreported nothing in five launches, but five launches cannot tell one
  failure in five from none, and a run takes more than twice as long while the slower start changes the timing
  the stalls depend on.
