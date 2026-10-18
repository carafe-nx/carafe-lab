# 2. Eden draws on the PC's GPU through a patched Mesa d3d12

Date: 2026-10-14.

## Context

[Decision 1](0001-emulator-test-bench.md) takes frames from the PC's GPU through WSL2. On the bench PC (Snapdragon X,
Adreno X1-85) the container sees the GPU only as Direct3D 12 through `/dev/dxg`. Ubuntu 24.04 does not build Dozen,
Mesa's Vulkan on D3D12, so Eden can use the GPU only through OpenGL on Mesa's d3d12 driver. Two things stopped it:

- Eden's "GPU" thread overflowed its stack about a minute in. The stack, symbolised with Ubuntu's debug symbols for
  `libgallium-25.2.8-0ubuntu0.24.04.4`, is an endless loop `begin_subquery` → `accumulate_subresult_gpu` →
  `d3d12_restore_compute_transform_state` → `d3d12_resume_queries` → `begin_subquery` in
  `src/gallium/drivers/d3d12/d3d12_query.cpp`. A query has 16 slots; every suspend takes one. When they run out,
  `begin_subquery` folds the results into the first slot, and restoring the state after the fold resumes the same
  query before the slot count is reset, so it folds again without end. The loop came with Mesa commit `cb019ff9266b`
  ("d3d12: GL4.6", first in 24.0.0) and is still in 26.2.4 and main (`aac5108bbfa`, 2026-10-14). Eden keeps its
  queries open all the time (`gl_query_cache.cpp`), which is valid GL. On the bench the slots run out on `glFlush`
  from `Tegra::DmaPusher::DispatchCalls` (`dma_pusher.cpp:45`).
- Qualcomm's shader compiler (`qcgpuarm64compilercore.so`) installs its own SIGSEGV handler for the whole process
  while it compiles (`llvm::sys::OverrideSignalHandler`). Fastmem faults of Eden's CPU threads then reach it instead of
  dynarmic, and Eden crashes. Turning fastmem off avoided it at the cost of CPU speed.

Found by a cloud session, checked on the bench PC.

## Decision

- The bench image carries its own Mesa 25.2.8: Ubuntu's sources and their 7 patches, only the d3d12 driver, plus a
  patch to `d3d12_query.cpp` that resets the slot count before the state is restored and does not begin a subquery
  that is already active. It is installed in `/opt/mesa-d3d12`; in GPU mode `run-eden.sh` puts it first in
  `LD_LIBRARY_PATH` and sets `__GLX_VENDOR_LIBRARY_NAME=mesa`.
- An `LD_PRELOAD` shim, `carafe-sigchain`, keeps the compiler's signal handlers to the compiler's own thread: its
  `sigaction` calls are remembered per thread, and the kernel always has a dispatcher that calls the compiler's
  handler on that thread and the program's handler (dynarmic) everywhere else. Fastmem stays on.
- Eden's renderer in GPU mode is OpenGL (`backend=0`); CPU accuracy stays at Auto.

Checked on 2026-10-14: our Mesa is loaded (`GL_VERSION … Mesa 25.2.8` without Ubuntu's suffix), the slots were folded
5,185 times in four minutes without a crash, and three bench runs of OpenTTD (six launches) reached the menu with no
crash, first frame 25–171 s after launch.

## Rejected

- A newer Mesa from a PPA (kisak, 26.2.4 for noble): the same loop.
- Dozen: Eden v0.2.1 needs features Dozen does not offer (tessellation, dual-source blending, logic ops,
  multi-viewport, variable pointers, transform feedback, extended dynamic state, push descriptors, robustness2);
  Dozen also stops at Vulkan 1.2.
- zink over Dozen: the same missing features.
- Closing Eden's queries between draws: breaks the guest's occlusion queries.
- Fastmem off with CPU accuracy "Debugging": works, but slows the CPU for every game.
