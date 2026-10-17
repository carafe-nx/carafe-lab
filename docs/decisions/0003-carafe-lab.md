# 3. The bench becomes Carafe Lab, a desktop app

Date: 2026-10-16.

## Context

The bench is a command-line program with a live page in the browser. It runs one list of games from `bench.json`, starts
a new Eden container for every launch, and compares runs by hand: A/B checks such as the lock-wait fix (3 of 3 against
0 of 3) were planned, run and counted in conversation. Games, emulator settings and scenarios are edited as JSON. The
user wants to manage the games to run, run many games in one pass, set up the emulator and every game, and set up the
testing itself, in an app with a good interface.

The mockups of the screens are the artifact "Carafe Lab" (https://claude.ai/artifact/6P9n7bwerzZjxRAcsVJUaz).

## Decision

### Name

- The app is called Carafe Lab; the project, its folder and its command are `carafe-lab`. The rename from
  `carafe-bench` happens when work on the app starts, together with the paths in docs and Docker files.

### Shape

- A Tauri app with a React interface in Carafe's style: the same tokens, controls and settings form as the Carafe app.
- Today's crate becomes the engine, a library. The window and the command line are two front ends over it and read the
  same files (`games/`, `scenarios/`, `plans/`, `runs/`), so what one sets up the other sees.
- The command line stays first-class: Claude and cloud sessions start runs and read `report.json` through it.
- Runs go through a queue in a separate process. Closing the window does not stop a run; a second session that needs
  the emulator waits in the queue, and the window shows who holds it.

### Screens

- Library — games with tags, notes, last result and a link to their ROADMAP item. A game folder is added by dropping
  it; the executable and architecture are guessed as Carafe's wizard does.
- Scenarios — a list of blocks with nesting (actions, waits, checks, repeat / if / fail), read top to bottom and
  stored as JSON. A flowchart of the same scenario is drawn from the list, for viewing only.
- Plans — games × scenario × repeats, with A/B as a switch; the plan shows how many launches and how long.
- Run — the queue, the game's screen in a Switch frame with pressed buttons lit, milestones, an FPS line, the log
  tail, and "take control" to play by hand and hand the game back.
- History — every run stamped with the Carafe commit, the runtime hash and the Eden patch set; "repeat exactly";
  trends per game; two runs compared; A/B ends with a verdict.
- Emulator — the one emulator's settings and status.

### Games

- The main way in is a game folder: the lab packs it with the current checkout, so every runtime change is tested,
  A/B can build two variants, and a run can be repeated.
- A ready NSP is the second way: it runs as it is, marked as a ready build, with the runtime version and settings read
  from inside and nothing editable. If the library has the same game's folder, it offers to rebuild with the current
  runtime.

### One emulator, like the console

- The goal is an Eden that behaves like the Switch: 4 cores, 4 GB, docked, the firmware chosen in Carafe's settings.
  Its settings change only in the Emulator section, between runs.
- One emulator serves the whole run; games follow one another in it, as on the console. There are no emulator
  profiles: A/B varies the runtime build, a runtime flag or a game setting, never the emulator.
- A watchdog restarts Eden when a game crashes, hangs or does not close from HOME, and the report says so. A "clean
  start for every launch" switch exists for investigations.
- A control channel, an Eden patch, loads and stops games and takes button presses and screenshots, instead of
  starting Eden with the game and going through `xdotool` and the X screen.
- Which renderer the one emulator uses (software or WSL GPU) is chosen by measurement: the one that misreports games
  less often.

### Video of the game

- The Run screen shows live video, as the live page does now. With Eden in Docker the chain stays Xvfb → x11vnc →
  VNC; the noVNC client library is embedded in the window as a canvas instead of a separate page, and the Switch frame
  with lit buttons is drawn around it.
- Screenshots for checks do not come from the video: Eden gives the exact frame, uncompressed, through the control
  channel.
- Capturing and encoding video takes resources, so during performance runs the video is turned down or off.

### Open: Eden on Windows

- Eden built for Windows ARM64 with our patches would give Vulkan on the Adreno through Qualcomm's driver instead of
  OpenGL on Mesa's d3d12, no wait for Docker Desktop and no WSL crash dumps on drive C. Against it: the 24 patches are
  written and checked on Linux, some of them in exception handling and fastmem; Eden calls its Windows ARM64 builds
  very experimental; cloud sessions work on Linux x86. To be decided at the end, after a spike: build it, run OpenTTD
  and NFSU on Vulkan, compare. If it wins, it becomes the main emulator on this PC and the Linux build stays for the
  cloud and as a fallback.
- Video in that case: the Eden window is captured by Windows' own screen capture and encoded by the Snapdragon's
  hardware H.264 encoder into the app's window.
- The lab does not depend on where the emulator lives: the control channel is the same for Docker and Windows.

### Loader

- The window opens at once; the library and scenarios can be edited while Docker starts. The emulator's state is a
  status in the title bar; the Run screen shows the start-up stages (Docker, image, console files, Eden and firmware,
  control channel) with times and the log tail in place of the game's screen. "Run" waits for the emulator by itself.
- Each failing stage says what to do: start Docker, build the image, choose the console files in Carafe.

### For Claude

- Tools, one call each: screenshot, press, wait, logs, milestones. Claude checks games from a conversation, and the
  same tools are the bridge for a model that plays.
- `report.json` carries milestones, step times, the FPS series and the reason for a failure (code, last log lines,
  "for the console").
- Diagnostics are named flags (counters, log levels, A/B switches), not temporary patches such as
  `9999-temporary-counters`.
- The lab cleans up after itself: Eden crash dumps on drive C, stopped containers, old NSPs; it warns when space runs
  low.

### Logs

The lab exists to find errors, so it collects far more than today, in two levels. Detailed logging slows a game and
changes its timing, and a timing bug can vanish under it, so the detailed level is used only where it is needed.

- Every launch, always: the Eden log, the Carafe runtime log, Wine's errors, DXVK and Mesa warnings; the container's
  CPU and memory every second; the time of every frame, not only FPS; button presses, milestones, a screenshot every
  few seconds.
- After a failure, automatically: the lab runs the game again with detailed logs — Wine channels chosen by the
  symptom, the runtime's counters, Eden's debug level.
- On a hang, before the watchdog stops the game: the backtraces of all its threads through Eden's gdb stub.
- On a crash: registers, stack and module map, without Eden's full crash dump.
- All sources go into one timeline with one clock, so what Eden, the runtime and Wine did in the same second reads
  together.
- Error lines are compared with the previous version: lines that did not appear before are highlighted.
- Full logs are kept compressed for failures and recent runs; older passing runs keep a summary.

### Versions and history

Compatibility and performance between versions are among the lab's main jobs.

- Every run is tied to its version: the Carafe commit, the runtime hash, the Eden patch set and the game's settings.
- Releases are reference points. The runtime of a version is built once from its tag and kept, so a plan can run
  "main against the last release".
- Compatibility by version: which games pass in each version, and where something broke.
- Performance by version: time to the first frame, median FPS, the worst 1 % of frames, peak memory. Always several
  repeats and the median; only changes larger than the spread between repeats are flagged.
- Thresholds: a game that got slower beyond its threshold (20 % by default) fails even if it reached its menu.
- A regression between two versions starts a bisection over Carafe's commits that names the first bad one.
- A game that fails only sometimes gets a rate ("hangs in 3 of 10"), not a pass or a fail.

### Data from day one

- Every screenshot with its verdict and every button press is kept with the run. This is the dataset a screen
  classifier or a playing model would need later.

### Later: a model that plays

- The aim is at least ten minutes of live play in a run. It is shelved: other work comes first.
- First Claude plays through the lab's tools, with no training: as "eyes" on screenshots (menu, loading, black,
  wrong colours), then through menus and turn-based play.
- Real-time play: the emulator can pause, so a model can look, release the game for a fraction of a second with
  buttons held, and pause again. Whether the guest's time really stops while Eden pauses is not verified.
- An own model (a small Qwen-VL with LoRA) only if runs with Claude turn out too slow or too costly; the screen
  classifier first. Training needs a rented GPU: the Snapdragon PC has no CUDA.

## Rejected

- The browser live page as the main interface: no library, no editing, no history.
- A new container for every launch: seconds lost per launch, and unlike the console, which runs games one after
  another.
- Emulator profiles in a plan (software and WSL GPU side by side): the goal is one console-like emulator, and the A/B
  checks that matter vary the runtime.
- A flowchart as the scenario editor: most scenarios are linear, arranging arrows is busywork, and a graph is hard to
  compare between versions.
- A loader that covers the whole window: Docker alone takes up to a minute, and the library is usable meanwhile.
- Detailed logs in every launch: they change timing and can hide the bugs being looked for; they come with the
  automatic rerun after a failure.
- Comparing FPS from single launches: the spread between launches is larger than many real changes.
- Training a model now: the data does not exist yet; the lab collects it.
- Ready-made services: nunu.ai and Razer QA Companion work on studios' own builds, not an emulator in Docker; Cradle
  is a research framework, a source of ideas rather than a tool.
