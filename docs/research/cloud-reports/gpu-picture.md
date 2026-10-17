# Eden on the PC's GPU: broken picture — files for the cloud session

All times are 2026-10-14. Run names are the bench's run folders (UTC).

## screenshots/ — first frame of OpenTTD, taken by the bench (ImageMagick `import` of Xvfb)

- `expected-software-10-20-11.png` — how it must look: software mode (Eden Vulkan on lavapipe). Text in the red
  dialog is white.
- `gpu-12-07-39.png` — GPU mode. Text in the red dialog is bluish; a thin black vertical line at x≈348 from y≈290 down.
- `gpu-12-18-38.png` — GPU mode. Text bluish.
- `gpu-12-25-59.png` — GPU mode. The bench's own screenshot caught a flicker: a large black rectangle.
- `gpu-14-21-42.png` — GPU mode, run by another session. Colours normal.

Mean RGB of the 300x40 area at (490,360) inside the dialog: software 211 24 24; 12-07-39 and 12-18-38 191 24 45;
14-21-42 211 24 24; 12-25-59 0 0 0.

## flicker-frames/ — grabs of the Xvfb screen during run 12-07-39 (bench run 1), seconds apart

- `xvfb-g1-mostly-black.png`, `xvfb-g6-black-rectangle.png` — broken frames.
- `xvfb-g3-whole.png` — a whole frame a few seconds later.

## logs/

`eden_log.txt`, `carafe-overlay.log`, `autorun_runtime.log` of launch 1 of a software run (10-20-11) and two GPU runs
(12-07-39, 14-21-42).

## previous-report/gpu.txt

The full final report of the previous cloud session (Mesa d3d12 query recursion, Qualcomm signal handler).

## fixes/ — what we applied from that report, for review

- `mesa-d3d12-query-recursion.patch` — applied after Ubuntu's 7 patches to Mesa 25.2.8.
- `carafe-sigchain.c` — the LD_PRELOAD shim (comments removed, logic as in the report).
- `build.sh` — how Mesa and the shim were built (in `carafe-eden-build`, installed to `/opt/mesa-d3d12` and
  `/opt/carafe`); the image took them as a tar (`ADD gpu-fix.tar /` in `docker/eden-patched/Dockerfile`).

## bench/ — the bench's files as they are now (the bench has no git)

`docker/eden/run-eden.sh` (with the GPU lines added), `docker/eden-patched/Dockerfile` (with `ADD gpu-fix.tar /`),
`docker/eden-build/`, `compose.yaml`, `src/eden.rs` (container arguments, `qt_config`), `patches/` (Eden v0.2.1
patches; `9999-temporary-counters.patch` is another session's temporary diagnostics).

## attempts/

Our scripts, logs and results of 2026-10-14, each marked reliable or not; see its `README.md`.

## tools/

- `white.c` — a GL program that clears its window every frame; used to test the d3d12 → X path.
- `xsample.c` — a draft XShm sampler of the X screen; never run, not validated.
