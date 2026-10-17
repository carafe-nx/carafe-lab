# What we tried on 2026-10-14, and what of it is reliable

Everything that measured flicker here used a slow sampler (ImageMagick `import` + `convert`, about one frame per
1.3 s, or 20 frames in ~6 s for `flicker.sh`). A person watching the noVNC page saw flicker in runs where these
samplers counted 0 black frames, so every "no flicker" below proves nothing. Positive catches are real.

## Bringing the fix up

- `run.sh` — eden-cli (built 2026-10-12) under gdb with our Mesa and the shim, fastmem on; dprintf on
  `accumulate_subresult_gpu`. Result (reliable): 5,185 folds in 4 min, no recursion, no SIGSEGV; first backtrace
  `accumulate_subresult_gpu ← begin_subquery ← begin_query ← d3d12_resume_queries ← d3d12_start_batch ←
  d3d12_flush_cmdlist ← d3d12_flush ← tc_flush ← st_glFlush ← Tegra::DmaPusher::DispatchCalls (dma_pusher.cpp:45)`.
  That old eden-cli never reached the menu (guest wrote below its stack, 177k "Unmapped" log lines), unlike the current
  Qt build.
- `run-qt.sh` — the bench image's Eden Qt with Mesa and shim from Docker volumes, OpenTTD, 300 s. Result (reliable):
  menu reached, OpenGL in the guest, no SIGSEGV.
- `series.sh` + `logs/series-*.log` — three bench runs `--gpu wsl --timeout 300` with the fix in the image. Result
  (reliable): 6 launches, first frame at 25–171 s, no crash, input reached the game.
- `glx-hang.sh` — `glxinfo -B` in GPU mode never exits, with and without the shim; gdb: `fini()` →
  `cleanupLLVM()` → `llvm37::llvm_shutdown()` → `MutexImpl::acquire()` in `qcgpuarm64compilercore.so`.

## Flicker

- `flicker.sh` — 20 grabs, % of pure black pixels below the top bar. Bench run 3 at the menu: `1 83 1 1 1 1 1 88 1 1 1
  1 1 94 1 1 1 1 1 1` (3 of 20 black). Bench re-check run (`logs/recheck.log`, new runtime): 3 of 60.
- `compare.sh` — tried to compare Eden's own screenshot (Ctrl+P) with the X screen; Eden's screenshots were never
  written (the key did not reach Eden at first; later no file appeared). First 15 X grabs: `1 1 1 1 1 61 1 1 1 82 30
  1 1 1 1`.
- `white.sh` + `../tools/white.c` — a GL program clearing a 1280x720 window every frame through d3d12 → Xvfb; 60
  grabs, none black. Weak: slow sampler.
- `exp.ps1` + `flicker-long.sh` — manual container runs, 2–3 min of grabs each: normal, `GALLIUM_THREAD=0`, noVNC
  viewer attached in Edge, pressing A. All 0 black of 86–144 frames. Unreliable (slow sampler).
- `flicker-stream.sh` + `bench-flicker.json` + `logs/flicker-run.log` — bench run with a long scenario, grabs streamed
  with timestamps: 0 black of 245 frames, while the person saw flicker on the live page. This is the proof that the
  sampler is too slow.
- `9998-temporary-glfinish.patch` — `glFinish()` before `SwapBuffers()` in `RendererOpenGL::Composite` when
  `CARAFE_GL_FINISH` is set. Written, never built or run.

## Decision we wrote

`0002-wsl-gpu-patched-mesa.md` — the bench's decision record for the fix, written before the flicker was understood.
