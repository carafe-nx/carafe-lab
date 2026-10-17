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
2. The submodules: `git submodule update --init external/eden external/carafe`. Carafe's own submodules are not
   needed for Lab's crates.
3. The runtime and the packer: from the Carafe checkout's `app/`, `npm run runtime` and `npm run hacbrewpack`.
4. The images, from this folder:
   - `docker compose --profile build build eden-build` — the image Eden is built in;
   - `docker compose --profile build run --rm eden-build` — Eden v0.2.1 with `patches/eden/*.patch` in the volume
     `carafe-lab-eden-src`, the program in `build/eden/qt/` (the first build takes about an hour; `LAB_BUILD_CPUS`,
     `LAB_BUILD_JOBS` and `LAB_BUILD_MEMORY` raise its limits of 2 CPUs, 2 jobs and 4 GB);
   - `docker compose --profile build build mesa-d3d12` — Mesa 25.2.8 with the patched d3d12 driver and the
     `carafe-sigchain` shim for the PC's graphics card ([decision 2](decisions/0002-wsl-gpu-patched-mesa.md)); about
     47 MB of downloads and 5 minutes;
   - `docker compose --profile build build eden` — the image `carafe-lab-eden` that runs games;
   - `docker compose --profile build run --rm probes` — the probe programs in `tools/probes/build/`.
   `external/eden` is never edited: Eden's changes are the patches.
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

From `app/`: `cargo run --release -p lab-cli -- run [set]` runs the enabled games of `lab.json` or the games of a set;
`cargo run --release -p lab-cli -- help` lists the rest.

Lab packs and runs games with the console files chosen in Carafe's settings. Eden gets them read-only in a folder that
exists only inside the container and goes away with it: nothing of them lands in the work folder, and their location
is cut out of every message, log and report Lab writes.

## Games

`cargo run --release -p lab-cli -- run <folder>` takes every folder inside `<folder>` as a game, the way the build wizard would: the
largest game `.exe`, its product name, default settings. A file `<game folder>.lab.json` next to the game folder
changes that; every field is optional:

```json
{
  "executable": "bin/game.exe",
  "title": "Game",
  "arch": "x86",
  "arguments": ["-windowed"],
  "settings": { "system": { "cpu": "box64" }, "input": { "mode": "keyboardMouse" } },
  "scenario": { "steps": [ { "step": "launch" }, { "step": "waitFor", "milestone": "firstFrame", "timeout": 600 } ] },
  "skip": false
}
```

`settings` is merged over the defaults in the shape of `carafe.json`'s `settings`. A game always gets the same
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
- `expectChange` — `from`, `to`: two screenshots that must differ; `soft` makes it a warning.
- `expectPattern` — `name`: a screenshot of a graphics probe.
- `expectInput` — the input probe's log against the bindings of the build.
- `expectSave` — the game has written save data; remembered for `expectSaveKept`.
- `expectSaveKept` — the remembered save data is still there and unchanged.
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

Each run gets `$CARAFE_BENCH/runs/<date>/`:

- `report.html` — games or probes, every step, screenshots, the button table of the input probe, the log lines that
  matter, what the emulator lacks;
- `report.json` — the same for tools and for comparing runs;
- a folder per game: screenshots and the logs of every launch (`launch-1/`, `launch-2/`).

The exit code is 0 when nothing failed, 3 when something did.

A step marked "for the console" failed because the emulator's log shows it does not implement something the
runtime needs; check that game on the console.

Other folders: `nsp/` — the built NSPs; `eden/user/` — Eden's data: SD card, NAND, logs; `probes/` — the probe game
folders.
