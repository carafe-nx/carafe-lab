# Why the lab loads and runs games slower than the console

Measured on 2026-10-30 on the lab PC (Snapdragon X, 12 cores, Adreno X1-85, WSL2 kernel 6.18), Eden `58c1e20` with
`patches/eden`, Carafe runtime #21, Need for Speed Underground 2, the WSL GPU, `dynarmic` unless said otherwise.

## The gap

| | Console | Lab |
|---|---|---|
| Lab work before Eden gets the game (NSP ready) | — | 38–48 s |
| NSP rebuilt (settings, diagnostics or runtime changed) | — | 64 s more |
| Eden's start to the game's first picture (`loading screen hidden after`) | 12 s | 37–54 s |
| Main menu, game thread a frame | 16.3 ms at 59.5 fps | 14.5 ms at 58 fps |
| Standing race start, game thread a frame | 16 ms (capped at 60 fps) | 27 ms at 31–40 fps |
| After the close request until the run ends | — | about 20 s |

The menu runs as fast as on the console; races and loading do not.

## Where a race spends its time

Host threads of Eden over 5-second windows (`/proc/<eden>/task/*/stat`), with the 3D engine of the PC's GPU from
Windows' `GPU Engine` counters (the WSL VM's `vmwp` process), during the standing race start:

| Thread | Load |
|---|---|
| `CPUCore_2`, the emulated core the game thread (`4w@2`, 91 % busy) runs on | 95–99 % |
| `CPUCore_0`, `CPUCore_1` | 67–82 % |
| `GPU`, Eden's GPU thread | 28–40 % |
| `eden:gdrv0`, Mesa d3d12's driver thread | 25–32 % |
| Adreno 3D engine | 40–52 % |

The race is held by the emulated CPU: the core with the game thread is saturated while the GPU path is half idle.

Stacks of Eden's threads sampled with gdb (22 snapshots in the race) agree: the emulated core of the game thread is in
generated code most of the time; Eden's GPU thread waits for commands or for the driver. When it waits, it is in
`glFlush` from `Tegra::DmaPusher::DispatchCalls`, which ends in `tc_flush` → `_tc_sync`: a full sync with Mesa's
threaded context on every pushbuffer dispatch, then `d3d12_fence_finish` and `D3DKMTWaitForSynchronizationObjectFromCpu`
(a wait for the GPU through `/dev/dxg`). This costs latency on the GPU thread but is not the limit today.

## How fast the emulated CPU is

The runtime's A57 benchmarks (`runtime/overlay/carafe_bench.c`, run with the profiler) on the console at 1785 MHz and
in Eden (which reports 1000 MHz, so its "cycles" are nanoseconds), converted to nanoseconds:

| Operation | Console | Lab | Lab slower |
|---|---|---|---|
| Dependent `add` | 0.56 | 3.4 | ×6 |
| Dependent `fadd` / `fmul` | 2.8 / 3.4 | 12.2 / 13.3 | ×4 |
| `cmp` + `csel` | 1.1 | 9.6 | ×8.6 |
| Indirect branch, one target | 0.56 | 3.4 | ×6 |
| Load from L1-sized data | 2.4 | 8.1 | ×3.4 |
| Load from DRAM-sized data | 126 | 122 | ×1 |
| Atomic increment alone / against another core | 16 / 58–64 | 124 / 681–860 | ×8 / ×11–15 |
| Line handed between cores by spinning | 78–144 | 272–664 | ×3.5–4.6 |
| Waking a sleeping thread on another core | 600–2 000 | 41 000–60 000 | ×20–100 |

So dynarmic on this host runs compute-bound guest code 4–8 times slower than the console's A57, atomics 8–15 times
and cross-core wake-ups 20–100 times slower; memory-bound code runs at the same speed. Translated x86 code (FEX's
output) is compute- and atomic-heavy, and DXVK, the effect thread and the orchestrator hand work between threads all
the time. The game thread's 27 ms against 16 ms in a race is the mild end of this; the shader compiler threads during
loading are the hard end (see below).

The benchmarks run about 30 s after the start, while the game still loads, so single numbers vary by up to two times
between runs; the ratios above hold in both runs taken.

## Loading

From Eden's start to the first picture NFSU2 takes 37–54 s against 12 s. During the first 25 s the game thread is only
22–30 % busy and waits for other Wine threads (gdb: `NtWaitForAlertByThreadId`); DXVK's three shader threads are
20–48 % busy each, against about 10 % for one of them on the console at the same point; a Vulkan thread waits for GPU
fences through `nvhost` (`nvFenceWait`). The caches are warm: every run of the day used the same Title ID, NVK's
cache on the virtual SD card (`sdmc:/.mesa/mesa_shader_cache_sf`) is kept and DXVK's own cache lives in the game's
data. The time goes to the same compile and unpack work as on the console, run on a CPU that is several times slower
for it.

Eden's own shader cache is lost after every launch: `SHARED_DIRS` (`app/crates/lab-eden/src/lib.rs:87-95`) does not
include `shader`, and the container is removed after each run, so Eden translates every Maxwell shader to GLSL and
Mesa d3d12 compiles it again. Eden's GPU thread was busy in only a few of the samples during loading, so this is
seconds, not the bulk; not measured separately.

## The lab's own time

Docker events with nanosecond times over one run (`docker events`), NSP already built:

- **NAND copy, 33–41 s.** Before every launch `push_nand` (`app/crates/lab-eden/src/lib.rs:428-460`) starts a
  container that clears the NAND volume and `cp -a`s `build/work/eden/user/nand` into it (`PUSH_NAND`, `:71-84`). That
  folder is 7.4 MB in 215 files but 1 456 directories, and it is read from the Windows drive through WSL's file
  sharing: copying it by hand took 33.1 s, clearing the volume 0.04 s.
- **NSP build, 64 s**, whenever the game's settings, a diagnostic that changes the NSP (`profiler`, `verbose`,
  `hud`) or the runtime change; an A/B run builds one per variant.
- **Logs and close, about 20 s**: copying logs and screenshots out of the container and the close.
- Eden itself is ready 0.8 s after its process starts.

## What was tried

- **NCE** (`emulatorCpu: nce`): NCE never ran. Eden turns it on only for a 39-bit address space
  (`src/common/settings.cpp:198-214`), and Carafe gives Need for Speed Underground 2 a 32-bit one, so Eden logged
  "Program does not utilize 39-bit address space, unable to natively execute code" and used dynarmic. The slower run
  (15–19 fps, first picture after 82 s) was dynarmic stopped by the gdb sampler, not NCE. What NCE would take for
  Carafe is in [lab-nce](../lab-nce/ANALYSIS.md).
- **CPU accuracy Unsafe** (`Cpu/cpu_accuracy = 2`): the game crashed with `0xc0000005` 149 s in. Unsafe turns on
  `Unsafe_IgnoreGlobalMonitor` (`src/core/arm/dynarmic/arm_dynarmic_64.cpp:334-335`, on by default under Unsafe in
  `src/common/settings.h:328`), which breaks the exclusive accesses FEX builds x86 `lock` instructions from.
- Earlier work (ROADMAP) ruled out FEX TSO, half resolution, GPU accuracy, asynchronous shaders and huge pages for the
  race gap of Need for Speed Underground.

## What can be done

| Change | Gain | Cost |
|---|---|---|
| Copy the NAND only when it changed (a stamp, as the firmware already has), or as one archive | 33–41 s every launch | small, lab only |
| Apply A/B settings as the overlay's console-side changes on the virtual SD card instead of a new NSP per variant | 64 s per variant and setting | medium: the overlay already lays changes made on the console over the settings files |
| Keep `user/shader` (Eden's shader cache) and the container's Mesa cache between runs | seconds of loading | small |
| Keep one Eden container across runs, not only across the launches of one run | container start and NAND every run | medium |
| Run the guest's code natively with NCE ([lab-nce](../lab-nce/ANALYSIS.md)) | possibly the console's speed or more: the guest's code would run natively on a much faster core | large: several Eden changes, with correctness risk |

Done on 2026-10-30, checked with a short NFSU2 A/B (launch, first picture, close):

- The NAND goes to Eden's volume as one archive made with `tar` on Windows (0.11 s) and unpacked in the container:
  the container that pushes it lived 1.1 s instead of 41 s.
- An A/B run whose settings are only `options` runs both variants from A's NSP; B's settings are the game's
  `changes.txt` in its device save data (`user/save/device/<Title ID>/0/switch/wine/carafe/changes.txt`), written
  through `docker exec` before each B boot and removed before each A boot. B's overlay reported "1 changes made on the
  console laid over 1 files" and DXVK read the key; A's had none. B's NSP was reused, so a variant no longer costs a
  64–72 s build, and the launch began 3–9 s after the game's turn instead of 38–48 s.
- Eden's `user/shader` and the container's `/root/.cache` are Docker volumes. Eden wrote its cache for NFSU2 there; Mesa
  d3d12 wrote nothing to its cache folder, so that half only helps if a later Mesa caches.
- Not done: one Eden container across runs (after the NAND fix it saves a container start, a few seconds, and risks
  state carried between runs) and NCE (Eden changes of their own).

Speed numbers from the lab compare two variants under the same slowdown; they do not predict the console's numbers,
and gains that depend on cross-core hand-offs (DXVK's threads, the orchestrator) look different in the lab.

## Method and limits

- gdb sampling stops Eden for 1.4–5 s per snapshot; the first attempt that loaded symbols on attach stopped it long
  enough for the lab to give up on the run. Symbols were then loaded before attaching.
- Thread loads are from `/proc` over 5-second windows; GPU loads from Windows' performance counters every 2.5 s.
- One run for Unsafe: it differs from dynarmic by far more than the run-to-run spread.
