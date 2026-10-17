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

## Not verified

- Which caller ends the query: the frame-pointer chain could not be read inside the signal handler. A query deleted
  while active during GL context teardown is a guess.
- Whether `mesa-d3d12-query-recursion.patch` takes part: it changes `begin_subquery` and the accumulation path of the
  same file.
- Mesa's source was not at hand; reading `d3d12_query.cpp` needs the source download `docker/mesa-d3d12/build.sh`
  makes (about 47 MB).

## Effect on Lab

One Eden is replaced after every game on the WSL GPU; the run's results are the same as with a container per launch,
and each launch's `container-end.log` keeps the crash.
