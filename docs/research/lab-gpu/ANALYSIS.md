# The lab's GPU path after NCE

Code reading and data already collected, 2026-10-30: Eden `58c1e20` with `patches/eden`, Mesa 25.2.8 with
`patches/mesa`, NFSU2 on the WSL GPU. Nothing here was run for this analysis.

## Where the race stands

Under NCE ([lab-nce](../lab-nce/ANALYSIS.md)) the race start of `scenarios/sarek-soak.json` holds 24–26 fps, as
under dynarmic (20–27). The runtime's `[THREADS]` lines show the game thread 19–31 % busy (about 91 % under dynarmic)
and all guest threads together about one core. The guest's code is no longer the limit: a frame of about 40 ms is
mostly the guest waiting for the GPU path. The console holds 60 fps there (16 ms a frame).

## The chain a frame goes through

1. The game thread and DXVK's threads record and submit Vulkan work; NVK turns it into pushbuffers and submits them
   through `nvhost` (an IPC call into Eden on a CPU core thread).
2. Eden's GPU thread takes each submission (`Tegra::DmaPusher::DispatchCalls`), translates Maxwell commands into
   OpenGL calls and ends every submission with `glFlush` (`dma_pusher.cpp:45` → `RasterizerOpenGL::FlushCommands`,
   `gl_rasterizer.cpp:679-693`).
3. Mesa's threaded context hands the GL work to its driver thread (`eden:gdrv0`), which records D3D12 command lists
   and submits them through `/dev/dxg` to the Windows driver of the Adreno.
4. When the guest queues a finished frame, Eden's composition thread calls `GPU::RequestComposite`
   (`gpu.cpp:291-327`) and waits until the GPU thread has drawn the frame into the window: `RendererOpenGL::Composite`
   (`renderer_opengl.cpp:137-154`) blits it and calls `SwapBuffers`.
5. The window lives on Xvfb, which has no DRI3, so Mesa presents through `drisw`: `d3d12_flush_frontbuffer`
   (`d3d12_screen.cpp:680-735`) maps the back buffer for reading, which flushes and waits until the GPU has finished
   everything (`d3d12_flush_cmdlist_and_wait`), copies 1280×720×4 bytes to an X image and sends it with
   `XShmPutImage` and `XSync`. x11vnc then reads the Xvfb screen for the lab's video.

## Where it waits

Stacks of Eden's GPU thread from 22 gdb snapshots during a dynarmic race (`lab-speed`):

| Snapshots | Where |
|---|---|
| 8 | idle, waiting for the next submission |
| 7 | `glFlush` or `glFenceSync` → `tc_flush` → `_tc_sync`: waiting for Mesa's driver thread to drain |
| 2 | the present: `d3d12_flush_cmdlist_and_wait` in `d3d12_flush_frontbuffer`, and `XSync` after `XShmPutImage` |
| 2 | Mesa waiting for an older batch's D3D12 fence before reusing it (`d3d12_reset_batch` → `d3d12_fence_finish`) |
| 2 | Eden's cache locks (`InvalidateRegion`) |
| 1 | Windows' fence wait through `/dev/dxg` |

Three causes stand out in the code:

- **Every flush is synchronous.** Mesa's threaded context makes `glFlush` asynchronous only when the driver gives it
  a `create_fence` callback (`u_threaded_context.c:3626`); d3d12 creates its threaded context without one
  (`d3d12_context_common.cpp:510`). So every `glFlush`, every `glFenceSync` and every Eden fence waits until the driver
  thread has translated and submitted all queued work, and the two threads take turns instead of overlapping. Eden
  flushes once per guest submission.
- **Every present waits for the GPU to go idle.** The `drisw` present maps the back buffer synchronously, so the GPU
  thread blocks for the whole frame's GPU time, then for the copy and the round trip to the X server, and the GPU gets
  no new work meanwhile. Eden's composition thread waits for all of it (`WaitForSyncOperation`), and the guest's
  buffer is released only after. Eden's asynchronous presentation exists only in its Vulkan renderer
  (`vk_present_manager.cpp:113`).
- **Mesa keeps eight batches.** `struct d3d12_batch batches[8]` (`d3d12_context.h:174`): each flush starts a new one,
  and with flushes per submission, per fence and per present a ninth waits for the GPU to finish the oldest.

Two costs come with NCE and are not measured yet:

- With reactive flushing (Eden's default, on in the lab) pages the GPU caches are inaccessible to the CPU; a CPU write
  to one is a signal and an invalidation, a read is a signal and `GPU::OnCPURead` (`gpu.cpp:262-275`), a round trip
  to the GPU thread that may download with `glFinish`.
- Wake-ups between guest threads still go through Eden's scheduler; the earlier NFSU research counted about 8 ms a
  frame of Wine server round trips under dynarmic.

## Options

| Change | What it removes | Size |
|---|---|---|
| Counters (a named diagnostic): per few seconds, the GPU thread's time in submissions, flushes, fences, composites and presents; `OnCPURead` calls and time; NCE fault counts | the guesswork: which wait is how many ms a frame | small Eden patch |
| `create_fence` for d3d12's threaded context, as zink and radeonsi have | the driver-thread sync on every flush and fence | medium Mesa patch |
| A present that does not wait: copy the back buffer into one of two readback buffers and send the previous one to X | the GPU-idle wait and copy on every frame; the video is one frame late | medium Mesa patch |
| Present to the window at most N times a second (lab only) | part of the present cost, with no Mesa change | small Eden patch |
| More batches in Mesa d3d12 | waits for old batches | one line, if the counters show them |

Rejected for now:

- Vulkan through Dozen or zink: Eden needs features Dozen lacks ([decision 2](../../decisions/0002-wsl-gpu-patched-mesa.md)).
- Eden on Windows with Qualcomm's Vulkan driver ([decision 3](../../decisions/0003-carafe-lab.md)): Eden's NCE is
  Linux and Android only, so the guest's code would go back to dynarmic and the CPU limit.
- Turning reactive flushing off to avoid NCE faults: CPU reads of GPU results would see stale memory.

No number for the gain can be given before the counters: the waits above overlap with the guest's own, and the GPU
thread was idle in 8 of 22 snapshots. If the flush and present waits are most of the frame, removing them lets the
guest, the GPU thread, Mesa's thread and the Adreno work on different frames at once.

## Measured

Patch `0056` logs a `[GPU timing]` line every 10 s. NFSU2's race start (`sarek-soak`) under NCE, per 10 s:

| | Before | Mesa `0003` | Mesa `0003` + `0004` |
|---|---|---|---|
| Race fps | 15–26 | 26–35 | 31–45 |
| GPU thread idle | 0–1 s | 0–0.05 s | 0.02–0.14 s |
| `glFlush` | 1.7–2.1 s | 0.01 s | 0.01–0.06 s |
| Composites (of them `SwapBuffers`) | 3.0–4.1 s (2.4) | 5.2–5.6 s (4.8–5.2) | 4.3–4.6 s (3.7–4.1) |
| NCE faults on GPU-cached pages, writes | 16–18 k, 2.1–2.6 s | 26–34 k, 2.5–2.7 s | 28–36 k, 2.5–3.2 s |

- `0003` ([patches/mesa](../../../patches/mesa)): `glFlush` without a fence no longer waits for Mesa's driver thread.
- `0004`: the present copies the back buffer into one of two staging buffers and shows the previous one, without
  waiting for the GPU.

The GPU thread is still busy all the time. The present still takes 11–12 ms of it per frame: `drisw` first syncs with
the driver thread (`threaded_context_unwrap_sync`), which now holds the whole frame's queued work, then copies 3.7 MB
and sends it to X. One run with both patches sat on the race's loading screen for over two minutes at 60 fps with the
game thread 92–96 % busy; the repeat loaded normally, so it is not tied to the patches yet.

## The black road

NFSU2's road is black in the lab's race and fine on the console. It was black under dynarmic on 2026-10-30 07:16,
before NCE and the Mesa patches, so it is an older emulator bug. Checked so far, one change at a time:

- Eden's SPIR-V shaders instead of GLSL on the same OpenGL: still black (and red and blue swapped, since the lab's
  BGRA fixes are GLSL-only), so it is not Eden's GLSL emitter.
- The game's road reflections off (`g_RoadReflectionEnable` 0 in `Software\Electronic Arts\EA Games\Need for Speed
  Underground 2`, given as the run's save, `saves/nfsu2-noreflect`): the road ahead shows its asphalt; an area just in
  front of the car stays black, likely the car's shadow (`g_CarShadowEnable` 2), also drawn into a texture first.
- Software drawing: too slow to reach a race in 10 minutes.

So the textures the game draws into and samples later (the road reflection, likely the shadow) come out black.

Patch `0057` logs an image the GPU drew that a new image overlaps but cannot join: 34 in a race, all of one shape.
The game draws a 256×256, 128×128, 64×512 or 64×64 `B8G8R8A8_UNORM` image with one level, and the same memory is then
used as `A8B8G8R8_UINT` with 1 or 4–6 levels (the bit-exact views NVK uses for the mip chain DXVK generates). The
texture cache refused every one in `IsLayerStrideCompatible`: a single-level image's layer stride is one level, the
mipmapped one's is the whole chain, and they differ although neither has a second layer. Patch `0058` lets two
single-layer, non-3D images match whatever their strides. With it the race shows its asphalt, road markings, the
speedometer's dial and needle and a fuller minimap, and `0057` logs nothing.

The race then ran at 17–22 fps: the effects now cost their copies and mip chains, and the present takes about 20 ms
of the GPU thread a frame, most of it the wait for Mesa's driver thread before the copy.

The lab empties a game's save before every run, so registry settings have to be given as the run's `save`.

## The loading stall

With Mesa `0004`, two runs of four sat on the race's loading screen for minutes at 50–60 fps, the game thread
50–96 % busy and a thread `0s` on the same emulated core, and did not close; runs without `0004` loaded. The cause is
not known; the game's thread stacks are needed.

## Plan

1. The counters patch, one Eden build, one race run under NCE: the breakdown of a frame.
2. The change that removes the largest wait, built and checked with the same race.
3. The next one, and so on, while the race gains.
