# 10. Software-window presentation shows the current image

Date: 2026-11-07

## Context

Mesa patch `0004` queues the current image in a staging buffer and displays the copy queued by the
previous presentation. The current copy has no independent completion or display path. An application
that publishes one image and waits for input therefore leaves its last image undisplayed.

The DirectDraw input-idle fixture reproduces this on WSL/Adreno. The display texture contains the
expected colors, `cc3355ff` and `33cc55ff`, while the window still shows the loading screen. Removing
only Mesa `0004` makes the same fixture display its checkerboard. The guest remains on DirectDraw,
DXVK and Vulkan, and the host remains on OpenGL/D3D12.

The earlier [GPU investigation](../research/lab-gpu/ANALYSIS.md) measured a throughput gain with
`0004` in continuously redrawn scenes. It did not establish correct presentation when drawing stops.

## Decision

Remove Mesa `0004`. Restore Mesa's synchronous copy of the image being presented. Keep the independent
flush change in `0003`. Retain an input-idle presentation regression check alongside active-rendering tests.

A future asynchronous presentation design must independently complete the final queued image, own
its resource and window lifetimes, and drain or cancel work correctly during resize and teardown.
It must pass the input-idle contract before replacing the synchronous path.

## Rejected

- Requesting another game frame to release the last staging copy: presentation must work when no
  later game update occurs.
- Changing loading-screen conditions: the correct game image already exists in the GPU texture.
- Keeping the delayed copy as the default based only on continuous-rendering throughput: that leaves
  event-driven applications with stale output.

## Validation

The unchanged diagnostic fixture fails visibly in run `2026-11-07_06-39-57` and displays correctly in
`2026-11-07_06-47-20` with the replacement Mesa image. Both include the same synchronized texture
diagnostic; it does not repair the failing variant. With renderer diagnostics disabled, the same automatic
comparison fails on the previous Mesa image in `2026-11-07_06-49-23` (difference 116.3, tolerance 1)
and passes on the replacement in `2026-11-07_06-51-33` (difference 0.2). HoMM IV run
`2026-11-07_07-30-29` completes the Lab scenario with original DXVK defaults, including three hero moves,
the next day and save/load restoring the saved position. This pass uses the investigation runtime and
replacement Mesa image. With temporary runtime and Eden diagnostics removed, the static fixture passes again
in `2026-11-07_08-57-55` (difference 0.2), and the console HoMM IV NSP completes the full scenario again in
`2026-11-07_09-18-43`, including visually confirmed save/load restoration. The ordinary local run image now
contains corrected Mesa and clean Eden.
