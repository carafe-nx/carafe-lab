# Console memory and host renderer pressure

## Context

Lab must reproduce the console's memory constraints without confusing the emulated machine with the PC resources
needed to emulate it. Eden is pinned to `58c1e20ee58efa3900ba616207d460886214480b`; Mesa to
`f97d7dcca0e51d7e20ed4342bb5ea8afea7db7fa` (25.2.8).

In Eden, `src/core/hle/kernel/board/nintendo/nx/k_system_control.cpp:52–159` maps `Memory_4Gb` to 4 GiB of
physical memory and a 3285 MiB application pool. `src/core/device_memory.cpp` sizes the backing buffer from that
physical size. A large NCE address reservation is virtual address space, not additional guest RAM.

In the pinned OpenGL backend, `src/video_core/renderer_opengl/gl_device.cpp:345` queried
`GL_GPU_MEMORY_INFO_TOTAL_AVAILABLE_MEMORY_NVX` as current free memory. The buffer and texture runtimes snapshot
this value plus 512 MiB and subtract subsequent readings (`gl_buffer_cache.cpp:142,162`,
`gl_texture_cache.cpp:545,571`). Subtracting capacity from capacity masks growth and commonly leaves the estimate
at 512 MiB. Mesa's `src/mesa/main/get.c:1290` distinguishes capacity from current available memory. The
[NVX specification](https://registry.khronos.org/OpenGL/extensions/NVX/NVX_gpu_memory_info.txt) defines these as
different queries; neither guarantees that a future allocation will succeed.

## Decision

The guest stays at 4 GiB. Lab rejects extended memory settings and ignores direct attempts to override the layout
through its settings builder. The application pool remains Eden's console layout; it is not manually enlarged.
Patch 0082 also enforces the physical size and pool arrangement during kernel initialization when
`CARAFE_LAB_CONSOLE_MEMORY=1`, which Lab always supplies. This prevents a saved per-title override from bypassing
the global profile. Normal Eden launches without that environment variable retain upstream behavior.
Containers retain 4 CPUs and 6 GiB with no container swap. The WSL VM limit is a separate host setting.

`gpuCacheMb` configures a reproducible host renderer cache pressure target, default 4096 MiB, range 512–8192 MiB.
This is a soft cache target, not additional guest RAM or a hard cap on the Windows graphics driver. Reused
containers must have the same target. Patch 0080 corrects the free-memory query and saturates the baseline
subtraction when available memory grows. Cache thresholds use the smaller of the configured target and the
driver-derived baseline. Driver readings are estimates across the driver's accounting domain, not exact ownership
of individual Eden resources.

Under pressure, OpenGL waits for GPU completion before draining already-retired cache objects. Empty retirement
queues do not cause waits. Live objects and dirty guest data continue through the existing cache invalidation and
download paths. Download staging buffers become reusable after their fence completes and their data is copied to
the guest. Staging pools reclaim eligible idle allocations when another allocation would exceed one quarter of
the cache target per pool. Deferred downloads and unfinished transfers remain retained. Counted leases retain allocations
while any CPU mapping is borrowed, including copied mapping objects. Stable allocation slots preserve these
leases and outstanding fence pointers. A working set may exceed the target while resources remain live; it is
never truncated.
An upload fence is inserted when its last CPU mapping lease closes, so destroying an intermediate copy cannot
place the fence before subsequent uploads through another copy.

Buffer storage pressure and reclamation during loading also follow
[decision 14](0014-buffer-retirement-during-loading.md).

If a staging request would exceed its target, already-submitted transfers with no CPU borrower or deferred
consumer can be waited on, then reused or reclaimed. This applies backpressure to queued GPU work instead of
continually adding buffers. Uncompleted deferred downloads remain retained until their CPU copy is consumed;
waiting on those consumers inside an allocation would deadlock the same GPU thread.

The synchronization contract follows OpenGL 4.6 sections 2.6.1.2 and 5.1.3:
[object deletion and sync objects](https://registry.khronos.org/OpenGL/specs/gl/glspec46.core.pdf).
An unsuccessful staging mapping emits an explicit host graphics allocation failure before invalid CPU memory is
used. This improves attribution; it does not repair a lost host graphics device.

Reports keep the following independent measurements:

- Guest physical reservations against the application process resource limit, its limit, and total emulated RAM.
  Reservations are not Wine heap utilization. Patch 0081 publishes atomic telemetry snapshots while the resource
  lock is held; the UI reads them without entering the guest scheduler. The two values are approximate independent
  observations during a limit change. Their publication is diagnostic state and does not govern allocations.
- Container working memory (`anon + shmem`), total cgroup charge, file cache excluding shared memory, hard limit,
  and OOM-kill count delta.
- Host driver minimum available video memory, reported capacity, and configured renderer target.
- Upload and download staging allocated bytes. Diagnostic `deferred` bytes belong to outstanding CPU consumers;
  `fenced` bytes have sync objects, which may already be signaled; `borrowed` bytes have live CPU mapping leases.
  Retirement events count objects, not bytes.

These layers overlap, especially on UMA machines, and must not be summed. Sampled peaks can occur at different
times. New fields are optional when reading older reports. Missing or stale emulator readings remain unknown;
they never become zero. Emulator readings attach only to subsequent container samples and expire after five
seconds without a new reading. The original three-column resource log format remains readable.

## Validation

Host tests exercise budget boundaries, saturating accounting, retirement retention and draining. Rust tests cover
strict layout configuration, legacy reports, time correlation, unknown readings and separate memory layers.
The `memory` Windows probe logs `GlobalMemoryStatusEx`, distinguishes an untouched 6 GiB virtual reservation on
64-bit builds from committed/touched pages, and releases all allocations. Its default touches at most 256 MiB;
`stress` touches at most 2816 MiB and stops with 256 MiB reported physical headroom or on allocation failure.
The same NSP can run on Lab and the console. Probe results are observations of that runtime, not universal Windows
limits; the API's reported free-memory values are volatile.

API contracts: [VirtualAlloc](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc),
[GlobalMemoryStatusEx](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex).

Console equivalence and real-game memory stability require measured runs. This policy does not establish the
cause of the separately observed WSL D3D12 device loss or guarantee that it fixes it.

The same x64 NSP was checked on a console and in Lab with runtime hash `924a7915b90d`.
Both verified and released 256 MiB, and both accepted an untouched 6 GiB virtual reservation. The console reported
3,343,904,768 bytes of physical memory through `GlobalMemoryStatusEx`; Lab reported 3,444,568,064 bytes, exactly
96 MiB more. Wine obtains this total from Horizon's `InfoType_TotalMemorySize`. The owner of the additional console
reservation has not been established; the fixed 4 GiB profile alone does not establish equal application budgets.
No unexplained constant is subtracted to conceal this discrepancy.

Two successive bounded probe launches in one Eden process passed their data checks, screenshots and requested
shutdowns. File polling reads the current title's live save roots and SD logs inside the container instead of
copying every title's NAND save data on each poll. It preserves root precedence, ASCII-insensitive literal basename
matching, empty-file results and lossy UTF-8 decoding. Directory symlinks are not followed and file symlinks cannot
escape a lookup root. Images lacking the helper's Python interpreter retain the previous snapshot lookup.

A stress comparison used one NSP on the original Eden image and the patched image. Both verified more than
2 GiB of touched pages, released them, captured their final status and closed on request. The earlier close warning
did not recur. File checks support an explicit positive timeout while retaining the thirty-second default; the
stress scenario uses 120 seconds and waits for the actual completion marker rather than a fixed startup delay.

Stored Heroes IV and Carbon NSPs passed their intro, save, close and relaunch scenarios. Heroes IV also passed
with a 512 MiB cache target. These checks do not establish full gameplay. Double Agent still failed during the
Iceland mission's loading screen: a 64 MiB staging mapping failed after a new WSL allocation error, with no cgroup
OOM kill and a sampled container peak of 4562 MiB against its 6144 MiB limit. The memory policy does not resolve
that host graphics failure.

## Rejected options

- Limiting the entire emulator or WSL VM to 4 GiB: host emulation overhead would create failures absent on the guest.
- Increasing guest RAM to hide allocation failures: it would invalidate console memory testing.
- Hard-capping graphics resources by dropping active buffers, downloads or dirty textures: it would corrupt output.
- Treating `GL_OUT_OF_MEMORY` as proof that the guest exhausted its RAM: device loss can surface through the same
  high-level error, so the failing layer must be recorded.
- Adding cgroup, guest, GPU and VM totals: they have different accounting domains and overlapping allocations.
