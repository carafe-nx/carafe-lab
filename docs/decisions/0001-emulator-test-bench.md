# 1. Games are tested automatically in Eden

Date: 2026-10-10. Was decision 20 of Carafe; moved here with the bench.

## Context

Every change is checked on a console by hand ([testing.md](../../../carafe/docs/testing.md)): build, install, launch,
look, fetch logs.
It is slow and covers few games and few settings. Research ([automated-testing](../research/automated-testing/REPORT.md))
found no emulator known to run the runtime, but none ruled out by the kernel side; the GPU path and JIT speed are open.

## Decision

- The test bench emulates Horizon OS with **Eden**, run headless in Docker; a time-boxed spike comes first and decides
  how far the bench goes (section 7 of the report).
- Frames come from the PC's GPU through WSL2; software rendering stays the fallback.
- Each game runs the full scenario: launch, loading screen, input reaches the game, save survives a restart, exit and
  relaunch. Pass or fail comes from the log milestones first, screenshots compared with golden images second.
- Build settings are checked with probe programs, not games: settings files by Rust tests for every value; input and
  graphics probes in the emulator over a pairwise set of settings, plus random combinations in a nightly run.
- The result is an HTML report: games, steps, screenshots, log excerpts; what the emulator cannot show is marked
  "check on the console".
- The console stays the final check before a release.

## How it is built

- The bench is its own project, `carafe-bench`, kept next to this repository rather than in it: Eden, its image and
  the probes stay out of the app's repository. It builds NSPs with `carafe-core` and `carafe-pack` of the Carafe
  checkout beside it, the same code as the app, and runs scenarios against Eden. How to use it: `docs/bench.md` in
  that project.
- Eden's Qt front end runs full screen on Xvfb in the `carafe-eden` image (`docker/eden/`), the `amd64` or `aarch64`
  AppImage of v0.2.1 checked by its SHA-1. Buttons go in as keys of Eden's default keyboard layout through
  `xdotool`, pictures come out through ImageMagick's `import`, and Eden's data folder is a host folder, so logs, the
  SD card and save data are read directly. Eden's TAS is not used: it plays a fixed timeline and cannot wait for a
  log line.
- The console files chosen in Carafe's settings reach Eden read-only, in a folder that exists only inside the
  container; nothing of them is copied to the bench folder, and their location never appears in its output.
- The probes (`tools/probes/`) are C programs built by llvm-mingw for i686 and x86_64: an input probe that logs the
  Xbox controller, keys and mouse, and graphics probes that draw four coloured quadrants with GDI, Direct3D 9, 11,
  12 and OpenGL. A Vulkan probe is left out: llvm-mingw has no Vulkan headers.
- The pairwise set reduces frame limits to none, 30, 60, 120 and FSR sharpness to 0, 40, 100: 746,496 combinations
  of the reduced values, 25 pairwise runs.
- A game's Title ID comes from its folder name, so the emulator finds its data again; its save data is removed
  before each scenario.

## Rejected

- Ryubing: no scripted input or HOME, several nvmap ioctls unsupported.
- Full-system emulation (tegra_qemu, mizu): no GPU, needs boot dumps, unmaintained.
- Remote control of a real console (sys-botbase): needs a sysmodule on the user's console.
- Every combination of settings: 746,496 runs even with the reduced values.
- Eden's TAS files for input: a fixed timeline, no waiting for the game; only the Qt front end plays them.
