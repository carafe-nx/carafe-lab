# Roadmap

What's planned and what's done for Carafe Lab, without dates: the app's plan of
[decision 3](docs/decisions/0003-carafe-lab.md) in sections 1–5, then research and the emulator's and the games' own
bugs, which are not the app. Open items are in priority order; bold ones are in progress.

- `bug` — Lab or its Eden is broken
- `game` — a specific game misbehaves in Lab
- `feature` — something new
- `research` — a question without an answer yet

## Planned

### 1. The window and a run of many games

- [ ] `feature` Live video of the game in the window: the noVNC client library embedded as a canvas, not a separate page
- [ ] `research` Software or WSL GPU for the one emulator: run the set in both and keep the one that misreports less

### 2. Logs

- [ ] `feature` After a failure the game runs again with detailed logs chosen by the symptom
- [ ] `feature` On a hang: backtraces of all the game's threads through the gdb stub before the watchdog stops it
- [ ] `feature` On a crash: registers, stack and module map, without Eden's full crash dump
- [ ] `feature` One timeline of all log sources with one clock
- [ ] `feature` Diagnostics as named flags instead of temporary patches

### 3. Versions and history

- [ ] `feature` The runtime of every release built once from its tag and kept; plans can run "main against the last release"
- [ ] `feature` A regression between two versions starts a bisection over Carafe's commits

### 4. Scenarios and A/B

- [ ] `feature` Scenarios with nested blocks (repeat, "if the screen shows …"), with a flowchart drawn from them for viewing
- [ ] `feature` Reference screenshots for "the screen shows …" checks
- [ ] `feature` Random presses for a set time, to catch crashes and hangs without a scenario
- [ ] `feature` A/B between two runtime builds or a runtime flag, not only a game setting
- [ ] `feature` "Take control": play by hand during a run and hand the game back
- [ ] `feature` Scouting: play by hand, the lab records presses and milestones into a scenario
- [ ] `feature` Clean-up: Eden crash dumps, stopped containers, old NSPs; a warning when space runs low

### 5. Ready NSPs and the console

- [ ] `feature` A ready NSP runs as it is, with the runtime version and settings read from inside
- [ ] `feature` "Rebuild with the current runtime" for a ready NSP whose game folder is in the library
- [ ] `feature` The lab builds NSPs and a "check on the console" list; console verdicts go into the same history

### Research and later ideas (not the app)

- [ ] `research` Eden with a real firmware folder: does every game still pass, what changes
- [ ] `research` Eden on Windows ARM64 with our patches and Vulkan on the Adreno: build, run OpenTTD and NFSU, compare with Docker
- [ ] `feature` If Eden on Windows wins: video through Windows screen capture and the hardware H.264 encoder
- [ ] `research` Does the guest's time stop while Eden is paused
- [ ] `research` Claude as "eyes": a verdict on a screenshot (menu, loading, black, wrong colours)
- [ ] `research` Claude takes a game from the title screen to play through menus with the lab's tools
- [ ] `research` Ten minutes of live play in a run: pause, look, release the game with buttons held, pause again
- [ ] `research` A small Qwen-VL as screen classifier, only if runs with Claude are too slow or too costly

### Emulator and games (not the app)

- [ ] `bug` The input check still passes on OpenTTD: its title jumps between `first-frame` and `after-a` in every run
- [ ] **`game` OpenTTD stands still for 20–110 s at start-up in 8 launches of 14, with both renderers and on the console too, so the cause is in the runtime (Carafe's roadmap). With the lock-wait fix 3 launches of 3 had no stall (first frame at 24–54 s), without it 3 of 3 stood still (80–111 s); threads spun in Wine's `spin_lock` at the addresses the cloud named, and the holder waited for newlib's malloc lock. Still unexplained: the counted malloc waits are short (at most 119 ms)**
- [ ] `bug` Eden: OpenGL draws 9–15 fps; keeping a core's read area until the GPU writes to it (`0017`) cuts slow `OnCPURead` checks ~500 times and gave a peak of 25 fps; the temporary counters that measured it are archived
- [ ] `bug` When the bench's timeout cuts a launch short, OpenTTD starts the next one without hardware acceleration (`hwaccel.dat`), so the second launch no longer measures OpenGL; a draft that clears it only after a launch the bench cut short before the first frame is in the cloud's `carafe-bench-fixes.zip` (`renderer_marker.rs`), not wired in
- [ ] **`bug` OpenTTD without hardware acceleration crashed once with 0xc0000005 about 58 s in, after 57 data aborts on memory of threads that had exited (pass 31); reached the menu in pass 32**
- [ ] **`bug` WSL GPU: OpenTTD's picture flickers — 0–3 of 20 frames partly or almost all black, at random; a GL program that fills the window through the same Mesa d3d12 and X shows none, so it is between Eden's OpenGL renderer and d3d12. It comes in bursts: 100 frames in a row can be whole with and without Mesa's threaded context (`GALLIUM_THREAD=0`), so short samples prove nothing**
- [ ] **`game` Need for Speed Underground crashes with 0xc0000005 in EA's stream ring buffer (`Speed.exe+0x12e6d`/`+0x12ebb`, a packet pointer of 0xaaaaaaaa): 23 s after the first picture on the WSL GPU, ~4 min in with software rendering; the screen stays black at 5–8 fps after the menu music starts**
- [ ] **`research` Need for Speed Underground races at 10–22 fps on the WSL GPU against 33–40 on the console (menus 50–55 against 60). Not the cause: NCE or dynarmic, FEX TSO, half resolution, GPU accuracy, asynchronous shaders, `d3d9.maxFrameLatency`, a flush per command list, shader builds, huge pages, CFS throttling, NCE faults, CPU reads of GPU memory. The game's main thread is 88% busy as on the console (98%), the GPU thread and the Adreno are a third busy, Wine server round trips cost ~8 ms a frame**
- [ ] `game` Need for Speed Underground runs at 13–27 fps with software rendering (27 with 12 CPUs, which needs more than 6 GB)
- [ ] `game` Need for Speed Underground sometimes stops right after its swapchain is made: no present for 150–300 s
- [ ] `game` Once in 2 launches DXVK's `dxvk-cs` thread crashed with 0xc0000005 in `d3d9.dll` (before `0027`–`0030`)
- [ ] `bug` Eden: DXVK's HUD text stays invisible with DXVK's descriptor buffer in the cloud's synthetic test (`d3d9clear`)
- [ ] `feature` Eden: lay out a 39-bit process with ASLR as the console's loader does, instead of code at 64 GiB (`0008`)
- [ ] `bug` "Save data survived the restart" fails when the game itself deletes a file (OpenTTD's `hwaccel.dat`); compare the save at the close with the save at the next start; a draft with tests is in `carafe-bench-fixes.zip` (`save_kept.rs`), it needs a save snapshot taken before Wine rewrites the registry
- [ ] `bug` Eden's RenameFile fails when the target exists; Wine's registry save retries every 3 s
- [ ] `bug` WSL GPU: Eden crashes with SIGSEGV after Need for Speed Underground ends itself; with software rendering it just stops
- [ ] `bug` WSL GPU: Qualcomm's shader compiler hangs when a program exits (`llvm_shutdown` in its `fini` waits on a mutex), so `carafe-bench gpu` never returns
- [ ] `feature` Offer the patches to Eden upstream, with the `eden-cli -c` option bug found on the way
- [ ] `feature` Report the d3d12 query recursion to Mesa with the patch (decision 2)

## Done

- [x] `feature` Performance by version: first frame, median FPS, worst 1 % of frames, peak memory, over repeats
- [x] `feature` Only changes larger than the spread between repeats are flagged
- [x] `feature` Thresholds: a game slower beyond its threshold (20 % by default) fails (`expectPerformance`)
- [x] `feature` "Repeat exactly": a past run again with its own runtime and Eden image, not the current ones
- [x] `bug` Closing as from the HOME menu ends OpenTTD and it writes its save: the hang was Mesa's stop crash (`patches/mesa/0002`)
- [x] `feature` Live page: the emulator's screen in a Switch frame, buttons light up when pressed (the Run screen)
- [x] `feature` Full logs compressed for failures and recent runs; a summary for older passing runs (`tidy`)
- [x] `feature` `report.json` with milestones, step times, frame times and the reason for a failure
- [x] `feature` A screenshot every few seconds of a launch: a small picture every 10 s in `timeline/`
- [x] `feature` Frame times of every frame, not only FPS: mean, worst hundredth and stutters per launch
- [x] `feature` Error lines that the game's run before did not have are highlighted in History
- [x] `feature` Tools for Claude, one call each: the running game's logs and milestones (`logs`)
- [x] `feature` Every launch: Eden, runtime, Wine errors, DXVK and Mesa warnings; CPU and memory every second
- [x] `feature` Every screenshot with its verdict and every button press is kept with the run (the future dataset)
- [x] `feature` Video turned down (`--video low`, A/B from the window) or off during performance runs
- [x] `feature` Plans: a scenario chosen for the whole plan instead of each game's own
- [x] `feature` Screenshots are the game's own frame from Eden through the control channel, not a capture of the X screen
- [x] `feature` Watchdog: an Eden silent for 45 s during a game is replaced; "clean start" runs a new Eden per launch
- [x] `feature` Button presses through the control channel instead of xdotool
- [x] `feature` The one Eden set up like the console: 4 cores, 4 GB, docked; firmware optional, chosen in the window
- [x] `feature` Library: tags, notes and the open ROADMAP items that name the game; search by name, tag or note
- [x] `bug` One Eden runs game after game: stopped games give their memory back, a new Eden starts when memory runs short
- [x] `bug` Eden no longer crashes in Mesa's d3d12 `end_query` when software stops on the WSL GPU ([research](docs/research/eden-stop-crash.md))
- [x] `feature` Tools for Claude, one call each: `play`, `press`, `screen`, `perf`
- [x] `feature` Run screen: the scenario's button presses light up on the Switch frame's Joy-Con
- [x] `feature` Compatibility by version: games × Carafe commits with the last result, failures and FPS; where a game broke is marked
- [x] `feature` FPS from Eden (`0047`): a live chart on the Run screen, median and worst-tenth FPS of each launch in the report and history
- [x] `feature` A/B of a game setting: variants alternate, the run ends with a verdict on the first frame that counts only differences beyond the spread
- [x] `feature` Loader: each launch's start-up stages live on the Run screen, and what to look at when a launch stops at one
- [x] `feature` Scenario editor in the window: every step with its settings, reorder, add, remove; saved to the game's file in `scenarios/`
- [x] `feature` Two runs compared side by side: outcomes, times, what differs in their stamps, screenshots of the same steps
- [x] `feature` Games that fail only sometimes get a rate in the library ("failed 2 of 10"), build-only runs left out
- [x] `feature` Every run stamped with the Carafe commit, runtime hash, lab commit and Eden image; settings per game; "run these games again"
- [x] `feature` Plans: games ticked by hand × repeats, with launch count and time from past runs; `--games` and `--repeat` on the command line
- [x] `feature` Library: a game is added by dropping its folder or with a button; each game shows its last result
- [x] `feature` Live picture of the game in the Switch frame of the Run screen, view only or with keys sent to the game
- [x] `feature` Run screen: queue with stop, the run's games, the current game's steps, the event log; title bar shows who holds the emulator
- [x] `feature` One Eden for the whole run through a control channel (`0046`: boot, stop, screenshot, quit); a crashed or hung Eden is replaced and its end recorded
- [x] `feature` Console files chosen in the window's Emulator screen, kept in `lab.local.json` outside git; Carafe's choice when none
- [x] `feature` Tauri window in Carafe's style: Library, Plans, Run, History, Emulator, Scenarios
- [x] `feature` A queue of runs in its own process: closing the window does not stop a run, a second session waits
- [x] `feature` The command line answers in JSON: `status`, `games`, `runs`, `stop`, `run --json`; every run keeps `summary.json` and `events.jsonl`
- [x] `feature` The repository `carafe-lab` replaces the bench: Carafe's layout, crates `lab-core`, `lab-run`, `lab-eden` and `lab-cli`, Eden and Carafe as submodules, `lab.json` with scenario files in `scenarios/`
- [x] `feature` The image builds Mesa for the PC's graphics card itself (`docker/mesa-d3d12`) instead of taking a tar
- [x] `feature` The bench runs on the WSL GPU by default; `edenImage` and `edenSettings` in `bench.json` choose another Eden image and change Eden's settings without a patch
- [x] `feature` Scenarios for Need for Speed Underground in `bench.json`: `NFSU menu` (set `nfsu`) reaches the 3D menu, `NFSU race` (set `race`) drives from the title to a race with a keyboard mapping, the cursor moved off the menus and spare Enter presses
- [x] `feature` Eden reports other threads' CPU time (`0045`), so the runtime's `[THREADS]` lines and its sampling profiler work in the bench
- [x] `game` Need for Speed Underground shows its picture and right colours on the WSL GPU, 54–60 fps in menus: OpenGL clear rectangles and BGRA vertex attributes (`0040`), typeless float images declared as float (`0042`), vertex inputs past the 16 attributes of Mesa's d3d12 read as defaults (`0043`; DXVK's fixed-function shader failed to link and the colour pass painted black), red/blue-swizzled typeless stores swap in the shader (`0044`)
- [x] `game` Need for Speed Underground shows its title, videos and 3D menus with the reference's colours with software rendering: the sampler heap index NAK builds from two bytes (`0035`), remapped DMA copies counted in bytes (`0036`), clears limited to the clear rectangle (`0037`; the four letterbox clears wiped the frame), BGRA vertex attributes (`0038`), typeless stores to red/blue-swizzled views (`0039`)
- [x] `bug` Eden plays audio through SDL's dummy driver instead of its null sink: the null sink never consumed a buffer, so audout released none after the first 25 ms and Need for Speed Underground, which waits for its menu music to play, froze for good 30–45 s after its first picture (330 s of frames and file reads with the dummy driver)
- [x] `game` Need for Speed Underground shows its picture through DXVK and NVK: Eden binds subchannel 4 to the copy engine (`0019`), translates IMUL and PRMT (`0020`), maps legacy topology overrides such as the triangle fan (`0021`), tracks storage buffers loaded through a pointer (`0023`), signals syncpoint events after their state (`0024`), keeps guessed storage buffer sizes inside contiguous device memory so CPU writes to NVK's root table are seen (`0027`), stores surfaces with a channel mask (`0028`), takes BAR.SYNC with RZ (`0029`) and reads samplers from a constant-indexed heap with forced unnormalized coordinates (`0030`). Before, every frame read back right before present was zero
- [x] `game` Need for Speed Underground no longer ends itself: Eden unlocks nvmap memory when the guest frees its last handle (`0018`); it kept the DeviceShared attribute, libnx mapped thread stacks from that heap, svcMapMemory refused (0xd401) and the game quit at `TIMER_init`. 32-bit processes get ASLR as on the console (`0016`), so the runtime's thread-local pages no longer run out at 15 threads. 4 launches of 4 with and without mods reach Vulkan frames; OpenTTD unchanged
- [x] `feature` The Eden container is limited to 4 CPUs and 6 GB (4 GB of Switch memory plus Eden)
- [x] `game` OpenTTD reaches its main menu on the PC's graphics card (OpenGL on Adreno X1-85 through WSL2): 3 runs, 6 launches, first frame 25–171 s after launch, no crash
- [x] `feature` WSL GPU on a Snapdragon keeps fastmem on: the `carafe-sigchain` shim keeps the SIGSEGV handler of Qualcomm's shader compiler to the compiler's own thread (decision 2)
- [x] `bug` WSL GPU: Mesa's d3d12 driver no longer recurses until the "GPU" thread's stack overflows; the image carries Mesa 25.2.8 with the query slot count reset before the state is restored (decision 2); the loop is in every Mesa since 24.0.0
- [x] `bug` Eden: the software blitter lists A8R8G8B8 and A8B8G8R8 components from the low byte (`0015`); Mesa's 2D copy of OpenTTD's palette turned every colour into (B, 255, R)
- [x] `bug` Eden: a texture buffer with a BGRA swizzle is viewed as B8G8R8A8 (`0014`); Vulkan buffer views have no swizzle, so Mesa's BGRA pixel-buffer uploads swapped red and blue
- [x] `feature` Eden rebuilds incrementally: patched files whose content did not change keep their time, so a one-file patch builds in under a minute instead of 20
- [x] `game` OpenTTD reaches its main menu with hardware acceleration (OpenGL), 28–76 s after launch, and takes button presses
- [x] `bug` Eden: a shader with no self-branch after its end, as Mesa writes them, is sized from after its program header (`0013`); the decoder read the header as code
- [x] `bug` Eden: a CPU read of memory the GPU changed downloads it every time (`0012`); glReadPixels returned zeros, so the loading screen never left with OpenGL
- [x] `bug` Eden: unmapping a fixed buffer removes it from its allocation, so FreeSpace succeeds (`0011`); Mesa's GPU address space leaked until the game ran out of memory 4–7 min in
- [x] `bug` Eden: an access to unmapped memory reaches the process's handler as a data abort with its address (`0009`); Wine commits reserved memory from it, and without it OpenTTD hung about 25 s in
- [x] `bug` Eden: an instruction cache invalidation also drops each core's copy of the last code page (`0006`)
- [x] `feature` Eden's gdb stub can watch a process that runs FEX: the process's handler gets exceptions first (`0005`), `tools/eden-gdb.sh` attaches once per run
- [x] `game` OpenTTD reaches its main menu in the bench, 34 s after launch, with hardware acceleration off
- [x] `bug` Eden: a 39-bit process's code starts at 64 GiB, so thread local pages land above the 32-bit guest window (`0008`); thread creation failed with OutOfMemory after 23 threads
- [x] `bug` Eden: svcInvalidateProcessDataCache and svcStoreProcessDataCache work instead of failing (`0007`)
- [x] `bug` Eden: ic ivau and ic iallu invalidate the JIT of every core, not only the calling one (`0006`); FEX code ran stale on other cores
- [x] `bug` Slow start: `nand` lives in a Docker volume; through the host folder (9p) every file check took 1–5 ms and Eden makes thousands
- [x] `feature` The live page reconnects to the emulator's screen on every launch by itself
- [x] `feature` The bench runs games on the patched Eden from `external/eden` + `patches/eden`, with software rendering
- [x] `bug` Eden: user exceptions reach the process's handler and `svcReturnFromException` returns from it (`0005`); the runtime enters FEX through them
- [x] `bug` Eden on ARM64 hosts: the guest's FP trap enables stay off as on the Cortex-A57 (`0004`)
- [x] `bug` Eden: allocating the 3D object context binds subchannel 0 to the 3D engine, as nvgpu does (`0003`)
- [x] `bug` Eden: the compositor drops the last frame of a layer whose producer disconnected (`0002`)
- [x] `bug` Eden: methods on a subchannel with no engine are logged and skipped instead of crashing (`0001`)
- [x] `feature` Eden v0.2.1 lives in `external/eden` and builds with `patches/eden` in a container (`docker/eden-build`)
- [x] `bug` Eden crash dumps no longer fill drive C: every Eden container runs with a 1-byte core limit
- [x] `feature` The bench is its own project next to the Carafe checkout and packs with its crates
