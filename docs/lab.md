# Carafe Lab

Carafe Lab builds games the way Carafe does, runs them in the [Eden](https://git.eden-emu.dev/eden-emu/eden) emulator
in Docker, drives them with button presses, and writes an HTML report. It does not replace the console: the console
stays the last check before a release. Why Eden and what it can and cannot show:
[decision 1](decisions/0001-emulator-test-bench.md) and [research](research/automated-testing/REPORT.md); what Lab
grows into: [decision 3](decisions/0003-carafe-lab.md).

Lab packs with a Carafe checkout: the `external/carafe` submodule by default, or the folder `carafe` in `lab.json`
names, such as `../carafe` for a working copy next to this one. Its crates come from the submodule; the runtime and
hacBrewPack come from that checkout's `app/crates/carafe-desktop/` (`resources/carafe-runtime.bin`,
`binaries/hacbrewpack-*`).

## Once

1. Docker Desktop runs. On an ARM64 PC the containers are ARM64 too, and Eden is built for `aarch64`.
2. The submodules: `git submodule update --init external/eden external/mesa external/carafe` (Mesa is shallow:
   only its release tag, about 70 MB). Carafe's own submodules are not needed for Lab's crates.
3. The runtime and the packer: from the Carafe checkout's `app/`, `npm run runtime` and `npm run hacbrewpack`.
4. The images, from this folder:
   - `docker compose --profile build build eden-build` — the image Eden is built in;
   - `docker compose --profile build run --rm eden-build` — Eden v0.2.1 with `patches/eden/*.patch` in the volume
     `carafe-lab-eden-src`, the program in `build/eden/qt/` (the first build takes about an hour; `LAB_BUILD_CPUS`,
     `LAB_BUILD_JOBS` and `LAB_BUILD_MEMORY` raise its limits of 2 CPUs, 2 jobs and 4 GB);
   - `docker compose --profile build build mesa-build`, then `docker compose --profile build run --rm mesa-build` —
     Mesa 25.2.8 from `external/mesa` with `patches/mesa/*.patch`, only the d3d12 driver, and the `carafe-sigchain`
     shim, built in the volume `carafe-lab-mesa-src` into `build/mesa-d3d12/` ([decision 2](decisions/0002-wsl-gpu-patched-mesa.md),
     [decision 4](decisions/0004-mesa-from-a-submodule.md)); then `docker compose --profile build build mesa-d3d12`
     makes the image `carafe-lab-mesa-d3d12` of the built files;
   - `docker compose --profile build build eden` — the image `carafe-lab-eden` that runs games;
   - `docker compose --profile build run --rm probes` — the probe programs in `tools/probes/build/`.
   `external/eden` and `external/mesa` are never edited: their changes are the patches.
5. Copy `lab.example.json` to `lab.json` and put your paths in. Keep `work` in a folder Windows Defender skips: the
   runtime's Wine DLLs are unpacked there for every build.
6. `gpu: wsl` runs Eden on the PC's graphics card: OpenGL on Mesa's d3d12 driver through WSL2. `gpu: software` is the
   fallback: Vulkan on llvmpipe, everything on the CPU, and the only way to check Eden's Vulkan renderer. Ubuntu does
   not build Dozen, so there is no Vulkan on the graphics card. `carafe-lab gpu` shows what the container sees; on a
   Snapdragon it does not return, because Qualcomm's shader compiler hangs when a program exits.
7. Optional in `lab.json`: `emulatorCpu` (`dynarmic` or `nce`), `edenImage` — another Eden image, such as one built
   with diagnostic patches — and `edenSettings`, settings written over Eden's `qt-config.ini`:
   `{ "Renderer/gpu_accuracy": "0", "Renderer/debug": "true" }`. `Renderer/debug` makes Eden log the compile and
   link errors of its OpenGL shaders, which it keeps quiet otherwise.
8. `oneEden` (`true` by default) keeps one container and one Eden for the whole run and loads each game into it
   through Eden's control channel (`patches/eden/0046`): a one-line `command` file — `boot <file>`, `stop`,
   `screenshot <file>`, `press <ms> <keys>` (`patches/eden/0049`), `quit` — and a numbered `replies` file in
   `<work>/eden/user/control/`. Button presses go through the channel, inside Eden, instead of `xdotool`. An Eden that
   crashed, stopped answering or wrote no status line for 45 s is replaced at the next launch, and the reason with the
   last lines of the container goes into the launch's `container-end.log`. Before each boot Lab reads the container's
   memory and starts a new Eden when less than 5 GB of its 6 GB is left; the launch's `session.log` has the reading
   and the choice. `--clean`, or the Plans screen's "Clean start", gives every launch of one run a new Eden, still
   driven through the control channel. `false` starts a container for every launch the old way, closing games through
   `SIGUSR1` and pressing through `xdotool`.

From `app/`: `npm run dev` opens the window — the library, sets to run, the run going on, past runs and what the
emulator needs to start. `cargo run --release -p lab-cli -- run [set]` runs the enabled games of `lab.json` or the
games of a set from a terminal; `cargo run --release -p lab-cli -- help` lists the rest. Both go through the same
queue, so a run started in one is seen in the other.

Lab packs and runs games with the console files chosen in the window's Emulator screen, kept in `lab.local.json` next
to `lab.json` (git ignores both), or with the ones chosen in Carafe's settings when none are chosen there; `--keys`
overrides both. Eden gets them read-only in a folder that
exists only inside the container and goes away with it: nothing of them lands in the work folder, and their location
is cut out of every message, log and report Lab writes.

Eden is set up like the console: 4 cores, the 4 GB memory layout, handheld at 1280×720
([decision 7](decisions/0007-handheld-720p.md)). The firmware is optional: a folder of `.nca`
files chosen in the same screen and kept in `lab.local.json`; Lab copies it into Eden's NAND once and again only when
the folder changes ([decision 5](decisions/0005-firmware-chosen-in-lab.md)). Without it, Eden runs without firmware.

## Games

Every game is an entry of `games` in `lab.json`. Only `name` and `folder` are needed; the rest is taken the way the
build wizard would take it: the largest game `.exe`, its product name, default settings.

```json
{
  "name": "Game",
  "folder": "D:\\Games\\Game",
  "enabled": true,
  "executable": "bin/game.exe",
  "title": "Game",
  "arch": "x86",
  "arguments": ["-windowed"],
  "settings": { "system": { "cpu": "box64" }, "input": { "mode": "keyboardMouse" } },
  "scenario": "scenarios/game.json"
}
```

`scenario` is a file relative to the work folder or the steps themselves: `{ "steps": [ … ] }`. Scenario files are
your own, like the runs, so they live in the work folder, outside the repository; the Scenarios screen saves them to
`<work>/scenarios/`. `sets` names lists of
games; `run <set>` runs a set's games even when they are disabled. `settings` is merged over the defaults in the shape
of `carafe.json`'s `settings`. A game always gets the same
Title ID from its folder name, so its data in the emulator is found again; its save data is removed before each run.

Without a scenario a game runs the standard one: launch and wait for the first frame, press A and look for a change
on the screen (a warning only), close it the way the HOME Menu does, check the save data, launch it again, check the
save data is still there, close it.

### Steps

- `launch` — starts the game.
- `waitFor` — `milestone`: `wine`, `loadingScreen`, `firstFrame` or `commit`; `timeout` in seconds. Fails at once
  when the game crashes, the loading screen gives up or the game ends.
- `wait` — `seconds`.
- `press` — `buttons`: Switch buttons as in `carafe.json` (`A`, `ZR`, `LUP`…), held together; `hold` in seconds.
- `screenshot` — `name`.
- `expectChange` — `from`, `to`: two screenshots that must differ; `soft` makes it a warning. `quiet` lists pairs
  of screenshots taken as far apart as `from` and `to` with no input between: when the screen changed about as much
  on its own in any of them, the step warns that the pictures cannot tell. The standard scenario has one pair before
  the press and one after. A screen that jumps now and then, as OpenTTD's title does every ~17 s, can still slip
  between the pairs.
- `expectPattern` — `name`: a screenshot of a graphics probe.
- `expectInput` — the input probe's log against the bindings of the build.
- `expectSave` — the game has written save data; remembered for `expectSaveKept`.
- `expectSaveKept` — the remembered save data is still there and unchanged.
- `expectPerformance` — the first launch's time to the first frame and median FPS against the game at the newest
  other Carafe commit in History: fails when either is worse by more than `threshold` percent, or by the game's
  `threshold` in `lab.json` (20 by default). Passes with a note when there is no other commit to compare with. The
  standard scenario has it after the first close.
- `expectScreen` — `name`, `reference`, `tolerance`: takes the screenshot `name` and compares it with the reference
  picture `reference`, a path in `<work>/references/`, by the mean brightness difference of 64×36 cells; more than
  `tolerance` (12 by default, of 255) fails. Without the reference picture yet it warns. A screenshot in History
  becomes a reference with "Make it a reference"; game pictures stay in the work folder, outside the repository.
- `randomPresses` — `seconds`, `seed`: presses random buttons for that long, each held 50–500 ms with 0.1–1 s
  between; fails when the game crashes or ends. The same seed presses the same buttons again.
- `repeat` — `times`, `steps`: runs the steps that many times; screenshots taken inside get `-1`, `-2`… after
  their names, and comparisons inside follow them. A failure stops the repeat.
- `ifScreen` — `reference`, `tolerance`, `then`, `otherwise`: takes a picture of the screen (`if-<n>.png`) and runs
  `then` when it shows the reference picture, `otherwise` when it does not or there is no reference yet; the step
  itself does not fail. Blocks nest, need a running game and neither launch nor close it. The Scenarios screen edits
  the blocks in place and draws the scenario as a flowchart under the steps.
- `close` — asks the game to close and waits `timeout` seconds; then stops it. Lab sends Eden SIGUSR1, and
  Eden closes the software the way the HOME Menu does, without asking (`patches/eden/0025`): a hotkey cannot do
  this, because without a window manager Qt matches no shortcut.

## Probes

`cargo run --release -p lab-cli -- probes` checks the build settings with small programs from `tools/probes/` instead of games:

- the input probe logs what Wine hands it — Xbox controller state, keys, mouse — and Lab presses every Switch
  button and compares the log with the bindings: the controller with default bindings, keyboard and mouse with a
  chord, and the controller with A and B swapped;
- the graphics probes draw four coloured quadrants with GDI, Direct3D 9, 11, 12 or OpenGL, 32 and 64 bit, under a
  pairwise set of graphics and system settings: every pair of values of any two settings appears at least once
  (25 runs instead of 746,496 combinations). `--random <n>` adds random combinations for a long run.

## Results

Each run gets `<work>/runs/<date>/`:

- `report.html` — games or probes, every step, screenshots, the button table of the input probe, the log lines that
  matter, what the emulator lacks;
- `report.json` — the same for tools and for comparing runs;
- `summary.json` — the run in short, rewritten after every game; `finished` is `false` while it goes;
- `events.jsonl` — every event of the run, one JSON line each, the same as `run --json` prints;
- a folder per game: screenshots and the logs of every launch (`launch-1/`, `launch-2/`). Each launch folder has
  Eden's log of that launch only, `frametimes.csv` (the time of every frame in milliseconds, without the waits for
  the display, from Eden's `record_frame_times`; the loading screen counts too), `resources.log` (the container's
  memory without the file cache and its processor time, once a second), `timeline/` (a 640×360 picture of the
  screen every 10 s while a step waits, named by the second of the launch) and `problems.json`: the error and
  warning lines of Eden, the runtime, Wine, DXVK and Mesa, lines that differ only in numbers counted as one.
  `summary.json` keeps the peak memory, the processor use and the counts for the History screen, which lists a
  game's problems and highlights those its run before did not have.

`carafe-lab tidy` shows what keeping the runs folder small would do and changes nothing; `tidy --apply` does it.
The 10 newest runs stay whole. Older failed or unfinished runs get their logs compressed into `.gz` next to them,
their pictures kept. Older runs where nothing failed lose their raw logs and timeline pictures and keep their
summaries, reports, events, screenshots and `problems.json`, which History reads. NSPs in `work/nsp` that no game
of the 10 newest runs uses go, with their build keys, and so do Eden's crash dumps in `%TEMP%\wsl-crashes` (WSL
writes 10–13 GB per crash) and stopped `carafe-lab` containers. The title bar warns when the work drive or the
temporary drive has less than 15 GB free.

Together they are the run's dataset: every press in `events.jsonl` has its buttons, hold and time, every screenshot
step its file, time and outcome, and every check the pictures it compared and its verdict.

The exit code is 0 when nothing failed, 3 when something did, 4 when the run was stopped while it waited for the
emulator.

## Plans, repeats and A/B

- `run --games A,B` runs these games in this order, whatever sets and `enabled` say; `--repeat <n>` runs each of them
  n times in a row. A game that fails only sometimes shows its rate in the window's library ("failed 2 of 10").
- `--scenario standard` or `--scenario scenarios/<file>.json`, a file in the work folder, runs every game of the plan
  with that scenario instead of its own; the Plans screen has the same choice.
- Every run keeps what it ran with: the runtime archive as `<work>/runtimes/<hash>.bin`, once for each hash, and the
  Eden image under the tag `carafe-lab-eden-kept:<id>`. `run --like <run folder>`, or History's "Repeat exactly",
  runs that run's games again with them instead of the current ones; the games' settings come from `lab.json`.
- A ready NSP made by Carafe carries its build record, `carafe.json`, unencrypted next to its NCAs, so the lab reads
  its runtime version, settings and game folder without keys: `carafe-lab nsp <file>` prints them. `run --nsp <file>`
  runs it as it is, as `<name> · NSP`: it is linked into `<work>/nsp/ready/<Title ID>.nsp`, since Eden names a game
  by its file, and stays out of performance by version, which compares the lab's own builds. `--rebuild` builds it
  again from its game folder with the lab's runtime and the NSP's own settings, keeping its Title ID. The game takes
  the name, scenario and threshold of the `lab.json` game with the same folder, if there is one. The Plans screen has
  the same under "A ready NSP".
- `run --build-only`, or "Only build for the console" in the Plans screen's own plan, builds the NSPs without running
  them and links each into its game's folder in the run as `<Title ID>.nsp`, so a later build does not replace the
  one under test. Such games, like games with a step the emulator cannot show, end "on the console", and History
  lists them under "Check on the console". The verdict brought back from the Switch, works or fails with a note, goes
  into `console.json` in the run's folder; from then on the game passes or fails by it everywhere History is read.
  Tidying removes these NSPs from runs older than the 10 newest.
- The Versions screen shows, under the pass/fail matrix, each game's performance at each Carafe commit: the median
  and half the spread of its repeats for the time to the first frame, the median FPS, the worst hundredth of frames
  and the peak memory. A measure is marked better or worse than the commit before only when the difference is
  larger than the spread of both; with a single repeat the spread is unknown and nothing is marked.
- Diagnostics are named, not temporary patches: `--diag verbose,profiler,hud,stacks,eden-debug`, or the
  Diagnostics choices of the Plans screen. `verbose` turns on the runtime's verbose traces (they add `stdout.txt`,
  `stderr.txt` and `stdin.txt` to the launch's logs), `profiler` its profiler (`[PROF]` lines), `hud` DXVK's full
  HUD; these three change the NSP, so a ready NSP run as it is takes only the other two. `stacks` starts Eden with
  its gdb stub and a gdb inside the container that lets the game run on: when a launch misses its milestone, gdb
  stops the game for a moment and `backtraces.txt` gets the module map, every thread with its state and every
  thread's stack, each unnamed frame marked with its module and offset. `eden-debug` sets Eden's log to the debug
  level. Eden's diagnostics need an Eden started with them, so changing them starts a new one.
- A game that fails runs once more right after, as `<name> · detailed`, with the diagnostics that suit how it
  failed: `verbose` always, with `profiler` and `stacks` when it never reached its first frame and `hud` when a
  picture check failed. `--no-retry` turns it off; A/B runs and probes never retry. The detailed run is left out of
  performance by version.
- When the game crashes, Eden logs the fault, the registers and the stack with the module of every frame; the
  launch's `crash.txt` keeps that part of the log, and the game's failure names the fault and its innermost frame,
  such as `Unhandled read of unmapped memory at 0x3fff8dcd00 in wine-nx-runtime+0xEF8B52D0`.
- Every launch's `timeline.txt` puts all its sources on one clock, seconds since the lab launched the game: the
  lab's steps, Eden's log (from its `Control: booting` line), the overlay and the runtime (from Eden's
  `Booting game` line, their own clock's zero) and the container's memory and processor every 5 s. A line without a
  time keeps the time of the line before it.
- `--video low` sends the live picture at about five frames a second, `--video off` sends none (`CARAFE_VIDEO` in
  the container), so the video takes little or nothing from the game while it is measured; A/B runs from the window
  use `low`.
- The Run screen draws the live picture itself, on a canvas in the Switch frame, with the noVNC client connected to
  the container's websockify on port 6080. The window serves the client under its own `novnc` scheme from a copy of
  `/usr/share/novnc/core` and `vendor` taken out of the Eden image the first time into `<work>/novnc`, so no package
  is downloaded and the client is the same version as the server. "Take control" switches the same connection
  out of view-only and holds the scenario: while a person plays, no step starts and a milestone's wait does not count
  (`<work>/queue/hold`); "Hand the game back" lets it go on. A new run, or leaving the Run screen, lifts the hold.
- While the game is in hand, "Record a scenario" scouts it: the window writes down every key of Eden's layout as the
  Switch button it presses, with how long it was held and the pause before it, and "Mark a stage" adds a screenshot
  there. Buttons pressed within 80 ms of each other become one press. "Save as the game's scenario" writes launch,
  first frame, the recorded steps and close as the game's scenario file.
- `run --games G --repeat <n> --ab '<settings>'` is an A/B run: G as it is (`G · A`) and with the settings merged over
  its own (`G · B`, in the shape of `carafe.json`'s `settings`), alternating. Each variant is its own NSP with its own
  Title ID. `summary.json` ends with a verdict: runs passed and the median and spread of the time to the first frame
  of each variant; a difference counts only when it is larger than the spread of either variant.
- `--ab-runtime <r>` instead of `--ab` keeps the settings and changes the runtime: `G · B` is built with `r`, a kept
  release (`release:v0.2.2`, `release:latest`) or any runtime archive, into `<work>/nsp/alternate`. The Plans
  screen's A/B card offers it as "another runtime", with the releases whose runtime is kept: main against a release.
  `run --runtime <file>` packs a whole run with another runtime archive.
- `carafe-lab release <tag>` builds the runtime of a Carafe release once and keeps it as
  `<work>/runtimes/release-<tag>.bin`. All of it runs in Linux containers with the build limits (2 CPUs, 2 jobs,
  4 GB), so Git for Windows never touches line endings: the tag is checked out in one clone of the Carafe checkout,
  `<work>/releases/carafe`, with the submodules taken from the checkout's own and Autorun's vendored Box64 and
  FFmpeg copied, so nothing is downloaded. A revision whose submodule commits or build image the checkout does not
  have stops and says so instead of downloading. The first time, the checkout's `runtime/build` is copied too, every
  file dated long ago except those the revision changes, so only the changed part is compiled; later revisions build
  on what the one before left.
- `carafe-lab bisect <game> --good <rev> --bad <rev> [--repeat n]` finds the Carafe commit that broke a game: it
  builds the runtime of the middle commit of Carafe's main line between them the same way
  (`<work>/runtimes/commit-<hash>.bin`), runs the game n times with it through the queue, and halves the range: any
  failure makes the commit bad. A commit that cannot be built is skipped. `<work>/bisect/<date>-<game>.json` keeps
  the commits, the verdicts and where the search stands. The Versions screen offers it under a measure that got worse
  between two commits ("Find the commit that broke it", three runs a commit).
- Every run's `summary.json` and `report.json` carry a stamp: the Carafe commit (`+changes` when the checkout has
  uncommitted changes), the first 12 hex digits of the runtime archive's SHA-256, the lab's commit and the Eden image.
  The window compares two runs side by side, their screenshots of the same steps included.

## Hands on

For Claude and for quick looks, one call each:

- `play <game>` builds the game if needed, launches it and keeps it running for `--minutes` (30 by default) or until
  `stop`; it holds the emulator in the queue like any run;
- `press <A+ZR>` presses buttons together in the running game, `--hold <s>` long;
- `screen <file.png>` saves the running game's screen;
- `perf` prints the median and worst-tenth FPS and the speed over the last 10 seconds; `--json` prints the readings.
- `logs` prints how far the running game got (loading screen, first frame, stalls) and the error and warning lines of
  Eden's log since its boot, the overlay's and the runtime's, grouped; `--json` prints the milestones and every group.

## The queue

One run holds the emulator at a time; the others wait in `<work>/queue/` in the order they came, whoever started them:
the window, Claude or a terminal. `--who <name>` names the run in the queue.

- `status` — who holds the emulator and who waits, the Carafe checkout and its commit;
- `games` — the games and sets of `lab.json`;
- `runs` — past runs and the one going, newest first;
- `stop [id]` — stops the run holding the emulator between steps, or takes a waiting run out of the queue by its id.

`--json` makes `status`, `games` and `runs` print one JSON document and `run` print its events as JSON lines. A run
whose process dies frees the emulator after 30 seconds.

A step marked "for the console" failed because the emulator's log shows it does not implement something the
runtime needs; check that game on the console.

Other folders: `nsp/` — the built NSPs; `eden/user/` — Eden's data: SD card, NAND, logs; `probes/` — the probe game
folders.
