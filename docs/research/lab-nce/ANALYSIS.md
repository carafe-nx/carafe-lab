# Running Carafe's guest code natively in the lab (NCE)

Code reading only, 2026-10-30: Eden `58c1e20` with `patches/eden`, Carafe runtime #21, FEX as Carafe builds it. Nothing
here was run; the checks it needs are listed at the end.

## Why it matters

In the lab every guest instruction is translated twice: FEX turns the game's x86 code into ARM64, and Eden's dynarmic
recompiles that ARM64 for the PC. The second step costs 4–8 times on compute, 8–15 times on atomics and 20–100 times on
cross-core wake-ups against the console's A57 ([lab-speed](../lab-speed/REPORT.md)). The PC's CPU is ARM64 too, so the
guest's code could run as it is: Eden's NCE does that for Switch games. A Snapdragon X core is several times faster
than an A57 at 1785 MHz, so with NCE the game thread and the shader compiles during loading would run at the console's
speed or faster, and the GPU path (Eden's GPU thread, Mesa d3d12, the Adreno) would become the limit.

## Why NCE never ran

Eden turns NCE on only for a 39-bit address space (`src/common/settings.cpp:198-214`, called from
`src/core/loader/deconstructed_rom_directory.cpp:183-186`). Carafe gives a 32-bit address space without alias region
to fixed-base 32-bit executables (`carafe-core` `pe.rs` `required_address_space`): Need for Speed Underground,
Underground 2 and, likely, Carbon. Eden logged "Program does not utilize 39-bit address space, unable to natively
execute code" and ran dynarmic. The NCE runs of `lab-speed` and of the Underground research were dynarmic.

Even a 39-bit Carafe game would not run under NCE as Eden is today; the sections below say why.

## What NCE does in Eden

- Guest addresses are host addresses. With NCE the loader calls `EnableDirectMappedAddress` and passes the host
  arena's base as `aslr_space_start` (`deconstructed_rom_directory.cpp:217-225`), so the process's regions start at a
  random host address between 2^36 and 2^39 (`src/common/host_memory.cpp:398-434`).
- Code the loader loads at boot is patched (`src/core/arm/nce/patcher.cpp:50-170`): each `svc`, `mrs`/`msr` of
  `TPIDR_EL0`/`TPIDRRO_EL0` and `mrs CNTPCT_EL0` becomes a branch to a trampoline in a patch section next to the module;
  exclusive accesses are made ordered. The return points go into the process's post handlers.
- A thread enters guest code through a trampoline when its pc is a post handler, otherwise through a signal to itself
  (`arm_nce.cpp:227-232`). Eden interrupts a core with `tkill` (`arm_nce.cpp:384`).
- A `SIGSEGV` in guest code goes to `Memory::InvalidateNCE` (`src/core/memory.cpp:961-982`): for a page the GPU caches it
  invalidates the GPU's copy and lifts the protection, for any other mapped page it returns "handled", and for an
  unmapped one the instruction is skipped (`arm_nce.cpp:114-140`). A `SIGBUS` goes to a one-instruction interpreter.
  No other signal from guest code is handled.

## What Carafe needs that Eden's NCE lacks

### 1. Guest addresses below 4 GiB

Every lab game is a 32-bit x86 program under WOW64. Its image and the memory Wine gives it must sit below 4 GiB of
guest address, and under NCE a guest address is a host address. Eden's 2^36–2^39 window has no room below 4 GiB, for a
32-bit address space or a 39-bit one: a 32-bit process would assert on its code address
(`k_process.cpp:1233` adds `aslr_space_start`), and a 39-bit one would have no guest memory below 4 GiB for
Carafe's low window (`low_window.c:93-96`).

Change: under NCE, map the guest's address space at host address 0, as on the console. The host side allows it: Eden is
a position-independent executable (`ET_DYN`) and the lab's samples show its libraries near 2^48, so 0x200000 up to 2^39
is free; Eden would reserve it with `MAP_FIXED_NOREPLACE` and keep `aslr_space_start` at 0. `HostMemory`'s direct-mapped
mode (`host_memory.cpp:615-617`, `645-664`) already maps by absolute address; only the reserved window changes.
`SetNceEnabled` then accepts 32-bit address spaces. Patch 0008's layout (39-bit code at 64 GiB) and 0051's heap placement
stay as they are.

### 2. Code the runtime maps itself

Carafe's loader is the program Eden loads and patches. The runtime (`wine-nx-runtime.nro`, 42 MB) is mapped later by the
loader with `svcMapProcessCodeMemory` and made executable with `svcSetProcessMemoryPermission`
(`runtime/loader/source/main.c:379,414`). Eden patches nothing at that point, so the runtime's `svc` instructions would
run as Linux system calls, with the number Linux reads from `x8`.

Change: when a code mapping starts with an NRO header, Eden patches its text before it becomes executable, places the
patch section in free guest address space within the ±128 MiB a branch reaches, registers the post handlers under a
lock (Eden's own TODO at `arm_nce.cpp:225-226`) and drops them when the mapping goes. Other executable mappings are left
alone: the game's x86 image and Wine's ARM64 PE modules are mapped the same way, and patching x86 bytes as ARM64 would
corrupt them. ARM64 PE code reaches Wine's Unix side through calls, not `svc`.

### 3. FEX's generated code

FEX writes its code through a read-write alias of code memory and runs it through an execute alias
(`fex_jit.c:81-91`); Eden maps the alias executable, so the code runs. It is never patched, which is fine for almost
all of it: it holds no `svc`, reads no `TPIDR` registers on Horizon, and its exclusive accesses are correct on real
hardware. The exception is `rdtsc`: Carafe's FEX reads `CNTPCT_EL0` (`horizon.patch`, `JIT/ALUOps.cpp:88-94`), which Linux
does not let user code read: in the lab's container (WSL kernel 6.18.33) the read raises `SIGILL`, and Eden has no
handler for it.

Change: the PC's counter runs at 19.2 MHz (`CNTFRQ_EL0` = 19 200 000), as the console's does, so on that trap Eden
rewrites the instruction into a read of `CNTVCT_EL0`, which user code may read: each site traps once, and the value
needs no scaling.

### 4. Exceptions the runtime handles

Lab patches 0005, 0009 and 0010 deliver exceptions to the process's handler under dynarmic, and the runtime depends on
them: it continues a context through `brk #0xfec0` (`fex_context.S:6`), commits reserved memory and swaps pages back in
from data aborts, and FEX's call-return stack has a guard page. Under NCE a `brk` is a `SIGTRAP` nobody handles, an
access to an unmapped page is skipped, and an access to a page the guest made inaccessible returns "handled" without a
change, so the thread faults on the same instruction forever.

Change: `SIGTRAP`, `SIGILL` and the `SIGSEGV`/`SIGBUS` that are not GPU tracking go to the same delivery as 0005/0009/
0010 (exception frame at 0x148 of the process local region, type in `x0`), with the fault address and syndrome, and
`InvalidateNCE` checks the guest permissions that 0010 keeps per page.

## What NCE still costs

- **Writes to pages the GPU reads.** Eden write-protects them (`memory.cpp:447-460`) and each first write after the GPU
  used a page is a signal, an `mprotect` and an invalidation. DXVK rewrites its upload and dynamic buffers every frame,
  so this is one signal per written page per frame: about a thousand pages would cost a few milliseconds a frame,
  against the tens of milliseconds dynarmic loses. If it shows up, Linux's asynchronous userfaultfd write protection with
  `PAGEMAP_SCAN` (kernel 6.7+, the lab's WSL kernel is 6.18) gives dirty pages without a signal, read when the GPU
  uses the memory.
- **Entries by signal.** A thread resumed after preemption or an exception enters through a signal, not a trampoline:
  microseconds, at the rate of preemptions and exceptions.
- **Eden's kernel.** Wake-ups between cores still go through Eden's scheduler and host condition variables; NCE makes
  the code between them fast, not the hand-off.

## Fidelity

The code that runs is the console's code: FEX on Horizon fixes its CPU features to the A57 (`MIDR 0x411fd071`), so it
emits no instruction the console lacks. What changes is speed: the lab's CPU becomes faster than the console's, so a
CPU-bound slowdown that shows on the console can hide in the lab. Dynarmic stays available for runs that should keep
the console's proportions.

## Without NCE

- **Unsafe CPU accuracy without ignoring the global monitor.** `cpuopt_unsafe_ignore_global_monitor` is a setting of its
  own (`src/common/settings.h:328`); with it off, Unsafe keeps exclusive accesses correct and adds reduced-error and
  inaccurate-NaN floating point on top of Auto. A configuration change; a gain only on floating-point-heavy code.
- **Instruction cache invalidation in batches.** Patch 0006 sends each `ic ivau` (one per 64-byte line, so dozens per
  FEX block) to all four cores, and each stops its JIT (`arm_dynarmic_64.cpp:104-122`). Collecting the lines and
  invalidating once at the core's next `isb` (dynarmic's `hook_isb`), `svc` or halt would cut this many times during
  loading, when FEX compiles the most. How much loading time it takes is not measured; a counter first.
- **Container CPU limit.** `--cpus 4` is a quota: Eden's threads may run on all twelve cores and then wait for the
  next period. Pinning to four cores with `--cpuset-cpus` keeps the same budget without the stalls, if `cpu.stat`
  shows throttling.

None of these closes a 4–8 times gap; NCE is the only change of that size.

## Plan

| Step | What | Size | Risk |
|---|---|---|---|
| 1 | Done: `mrs CNTPCT_EL0` raises `SIGILL` in the lab's container; the PC's counter runs at 19.2 MHz | minutes | none |
| 2 | Written: `patches/eden/0052`–`0055` (below); the first build ran the loader natively, the fixes since are not built yet | several patches | high: kernel and signal paths |
| 3 | Lab: Need for Speed Underground 2 to the first picture and a race start, NCE against dynarmic, then every lab game | runs | — |
| 4 | Count GPU-tracking faults per frame; asynchronous write protection only if they cost more than a few milliseconds | patch | medium |
| 5 | If NCE is faster and every lab game passes: `emulatorCpu: nce` by default and a decision | small | — |

Dynarmic's batched invalidation and the Unsafe accuracy variant are a fallback if NCE stalls.

## The patches

- `0052-nce-guest-addresses-are-host-addresses`: `SetNceEnabled` accepts 32-bit address spaces; under NCE
  `HostMemory` drops its random window and reserves the guest address space at host 0x200000 and up with
  `PROT_NONE` and `MAP_FIXED_NOREPLACE`; the loaders pass 0 as `aslr_space_start`.
- `0053-nce-patch-code-the-runtime-maps`: after `svcSetProcessMemoryPermission` makes a range read-execute and the
  range starts with an NRO header, `KProcess::PatchRuntimeCode` patches its text, maps the patch section as
  `AliasCode` in the first free range within branch reach, hides it from the guest and makes it executable on the
  host, and adds the return points; `svcUnmapProcessCodeMemory` drops them with the section. The patcher gets
  `SetPatchSectionDistance` so its post-data layout can point anywhere after the text. Post handlers are read under
  a shared lock. Eden's own boot-module path set the patch section's guest permission to none after making it
  executable on the host, and the guest permission change also reached the host mapping, leaving the section
  inaccessible; the two steps are swapped.
- `0054-nce-counter-reads-and-breakpoints-in-guest-code`: Eden's patcher stopped (`UNREACHABLE`) on a read of
  `CNTFRQ_EL0`; such a read now gets 19.2 MHz from a trampoline. `SIGILL` and `SIGTRAP` from guest code go to
  `HandleGuestTrap`: `mrs CNTPCT_EL0` is rewritten into `mrs CNTVCT_EL0` in place, anything else stops the thread as
  an instruction breakpoint, which `physical_core.cpp` (patch 0005) hands to the process's handler. The cores'
  threads block process-wide signals (`SIGUSR1`, `SIGINT`, `SIGTERM` and others), so Qt's handlers, the lab's close
  among them, never run on a guest stack with the guest's `TPIDR_EL0`.
- `0055-nce-access-faults-go-to-the-process`: a fault that the guest's own page permissions (patch 0010) forbid, or
  on an unmapped page, is a data abort with its address and the write bit from the fault's syndrome, handed to the
  process like the JIT's; an unaligned exclusive access that Eden's interpreter cannot run is an alignment fault
  (DFSC 0x21), which the runtime turns into a misaligned-access exception. Only faults the guest may make reach
  `InvalidateNCE`, and a read there first downloads what the GPU wrote (`HandleRasterizerDownload`), as the JIT's
  reads do since 0012: with reactive flushing, Eden's default, pages the GPU caches are inaccessible to the CPU, and
  Eden only dropped the GPU's copy, so queries and read-backs would have read stale memory.

## The other lab games

OpenTTD, Need for Speed Underground and Heroes of Might and Magic IV passed their standard scenarios under NCE with
the same warnings as under dynarmic and faster than before; NCE became the lab's default
([decision 8](../../decisions/0008-nce-is-the-default.md)).

## Third run: NFSU2 runs under NCE

With the TLS-free `carafe-sigchain`, NFSU2's standard scenario passed under NCE (run `2026-10-30_13-04-12`): launch,
first picture, A changes the screen, close on request, second launch. The loading screen was hidden after 30 s on the
first launch and 21 s on the second, against 47–55 s and 54–68 s in six dynarmic runs of 2026-10-28 and 12 s on the
console. The menu ran at a median 60 fps (low 55), as under dynarmic.

A race start (`scenarios/sarek-soak.json`, run `2026-10-30_13-16-27`) reached the standing start and held 24–26 fps,
as dynarmic did in the same scenario (20–27 fps). The runtime's `[THREADS]` lines show why: the game thread (`4w`) was
19–31 % busy instead of about 91 %, and all guest threads together used about one core; Eden used about 2.5 cores
instead of 3.2–3.5. The guest's code now takes about a third of the time, and the frame waits for the GPU path
(Eden's GPU thread, Mesa d3d12, the WSL GPU), which is the limit of the race now. That run's loading screen was hidden
only after 83 s: the runtime's `[PROGRESS]` counters show 34 s spent in writes to the virtual SD card
(`sd_write_ms`), against 0.4 s in the 30-second run, so the slow load was the lab's storage, not the CPU. Eden crashed
at the close on its GPU thread with a call through a null pointer, the known stop crash in Mesa's d3d12
([eden-stop-crash](../eden-stop-crash.md)).

## Second run and what it showed

With the fixes built, NFSU2 went further: the runtime was patched (`NCE: patched code at 0x26600000 (0x19b4000
bytes), 53 entries`), Wine started and its ARM64 PE modules ran natively, and the loading screen was drawn. Eden then
died on `CPUCore_0` with a `SIGSEGV` the kernel called fatal; its report showed a shared library's pc, a nearly
empty signal stack and the guest's registers still in place (`x18` the TEB, `x22` ntdll's base). The library is the
lab's own `carafe-sigchain`, preloaded so the Adreno shader compiler keeps its fault handler to its thread: its
`SIGSEGV` dispatcher read `__thread` variables, whose address comes from `TPIDR_EL0`, and under NCE that register holds
Eden's thread block while guest code runs. The dispatcher now finds a thread by `gettid` in a table and uses no
thread-local storage; the first shader compile, when the compiler installs its handler, is where the run died.
FEX itself is a PE module (`libwow64fex.dll`) that Eden does not patch: its 57 `CNTPCT_EL0` reads take the trap
rewrite, its `CNTFRQ_EL0` reads are allowed by Linux and return 19.2 MHz, and its one `TPIDRRO_EL0` read is only for
a CPU-index feature that is off under Wine.

## First run and what it showed

NFSU2 with `emulatorCpu: nce`, 2026-10-30: Eden's log shows the loader running natively (its supervisor calls
reached Eden, `svcBreak` announced the runtime), so 0052 and Eden's boot-module patching work. Eden then stopped on
`patcher.cpp:148` `UNREACHABLE` while patching the runtime: one `mrs CNTFRQ_EL0` in its text. A scan of the runtime's
text (`wine-nx-runtime.nro`, 27 MB of code) found 70 `svc`, 429 `TPIDRRO_EL0`, 12 `TPIDR_EL0` and 274 `CNTPCT_EL0`
reads, one `CNTFRQ_EL0` read, and system registers outside the patcher (`TPIDR2_EL0`, `GCSPR_EL0`,
`PMCCNTR_EL0`) only in feature-checked or benchmarking code of libraries; cache maintenance is `dc zva`, `dc cvac`,
`dc cvau`, `dc civac` and `ic ivau`, which Linux allows. The code ends about 83 MB after its start with its bss, so a
patch section placed after it is within branch range. The other changes since that run came from reading the code:
alignment faults, process-wide signals and reads of GPU-cached pages.

## Checks that need the PC

- Eden builds at 2 CPUs and 2 jobs for each step of 2.
- Short lab runs for each step; full runs over every lab game at the end.
