# Eden crashes when software stops on the WSL GPU

Found on 2026-10-17 while bringing up one Eden for a whole run (`patches/eden/0046`).

## What happens

- With `gpu: wsl`, Eden v0.2.1 with `patches/eden` ends with SIGSEGV (exit code 139) when software is stopped through
  the control channel's `stop`, which takes the same path as the HOME menu's close (`patches/eden/0025`).
- It happened at every stop of OpenTTD and of Need for Speed Underground in four runs, always at the same offset.
- Before one Eden served the whole run, the container was stopped by force right after the close request, so the
  crash was never seen; it is the likely reason the bench's "OpenTTD does not close on the WSL GPU" never closed.

## Where

- dynarmic's handler (`src/dynarmic/src/dynarmic/backend/exception_handler_posix.cpp`, Eden 58c1e20) prints
  `Unhandled SIGSEGV at pc …`: the fault is not a fastmem access.
- The fault address is `0xa8`. The pc is in `/opt/mesa-d3d12/lib/libgallium-25.2.8.so` (Mesa 25.2.8 with Ubuntu's
  patches and `docker/mesa-d3d12/mesa-d3d12-query-recursion.patch`, build ID `6ac69d18388f995e879f84a1c3a98a5cf6ef50fb`)
  at offset `0x6a6f98`, found by matching the pc against `/proc/<eden>/maps` taken during the run.
- `addr2line` on that offset: `end_subquery` (`src/gallium/drivers/d3d12/d3d12_query.cpp:532`, inlined
  `d3d12_current_batch` from `d3d12_context.h:311`) called from `end_query` (`d3d12_query.cpp:567`).
- The disassembly loads a pointer from the sub-query (`ldr x24, [x2, #64]`, line 533) and then reads `[x24, #168]`,
  inlined from `d3d12_resource.h:92`: the sub-query's result buffer is null when the query is ended.

## Cause and fix

With Mesa's source in `external/mesa` ([decision 4](../decisions/0004-mesa-from-a-submodule.md)):

- `d3d12_release_query` (`d3d12_query.cpp`) frees a query without unlinking it from `ctx->active_queries`.
  `d3d12_destroy_query` nulls the sub-queries' buffers and frees the query; the next suspend or end of the active
  queries walks into it and `end_subquery` reads the null buffer.
- `patches/mesa/0002` ends a still-active query and unlinks it before it is freed.
- Verified on 2026-10-17 with OpenTTD on the WSL GPU: every stop answered `stopped exited`, and the next launch booted
  in the same Eden.

## Effect on Lab

One Eden now serves a whole run on the WSL GPU. Before the fix, it was replaced after every game; each launch's
`container-end.log` kept the crash.

Keeping Eden alive across games showed the next limit: the emulated Switch's memory (a shared-memory file of about
4 GB) kept the pages the stopped game had touched, so the second launch of OpenTTD ran the 6 GB container out of
memory. `patches/eden/0048` hands those pages back when software stops, and trims the heap Eden's threads freed. Eden
still keeps 0.3–0.5 GB more after each game, so before booting a game Lab checks the container's memory and starts a
new Eden when less than 5 GB is left; each launch's `session.log` says which it chose. OpenTTD ×3 (six launches) passed
with four launches in the first Eden. A threshold of 4.5 GB was too low: a fifth boot with 1.6 GB in use and 4.85 GB
left ran out of memory while loading, so one OpenTTD launch takes about 4.4 GB above what Eden already holds.
