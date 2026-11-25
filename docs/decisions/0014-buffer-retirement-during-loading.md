# Buffer retirement during loading

## Context

Eden is pinned to `58c1e20ee58efa3900ba616207d460886214480b` and Mesa to
`f97d7dcca0e51d7e20ed4342bb5ea8afea7db7fa`. In upstream Eden,
`src/video_core/buffer_cache/buffer_cache.h` lines 1558–1579 create a replacement buffer, copy overlapping
buffers and retire the old objects. Lines 1822–1885 remove their cache bindings and registration, then transfer
ownership to an eight-frame destruction ring. `TickFrame` at lines 75–115 advances that ring.

During mission loading, buffer replacements can continue while presentation and `TickFrame` stop advancing.
A correlated capture found 26 retired buffers retaining 6,999,048,192 bytes alongside three active large buffers
with 376,111,104 logical bytes. Twenty-one retired buffers also belonged to the current unsubmitted Mesa batch.
The GPU had received no commands from that batch when the next allocation failed. Windows graphics commitment
closely matched the live D3D12 allocation footprint measured independently in Mesa.

Driver free-memory queries describe residency in their accounting domain. Buffer storage sizes describe the
resources Eden owns, including retired objects awaiting destruction. The existing pressure check used the driver
query and left those retired buffers alive throughout the loading interval.

## Decision

Patch 0083 checks buffer allocation pressure after overlap resolution, before allocating the full replacement.
It derives active storage from the existing cache slots and retired storage from the existing destruction ring.
The incoming buffer participates in the comparison against the configured renderer cache target. Byte
accounting is derived from ownership containers on demand. The trigger uses the storage owned by this cache.

When retired objects exist and the incoming allocation would exceed the target, the OpenGL runtime inserts a completion
fence and waits with `GL_SYNC_FLUSH_COMMANDS_BIT`. It drains the retirement ring only after a successful wait.
Fence creation failure and unsuccessful waits propagate an explicit error. Active buffers, dirty data and
outstanding staging leases retain their existing ownership paths. A live working set can exceed the soft target.

The wait submits the current command stream and establishes completion before resource destruction, including
buffers used through GPU addresses. It runs on the existing OpenGL context thread. In Mesa,
`src/mesa/main/syncobj.c` passes the pipe context to `fence_finish` and
`src/gallium/drivers/d3d12/d3d12_fence.cpp` lines 156–169 reclaim completed submitted batches with that context.
Releasing the retired GL objects then removes Eden's remaining ownership.

Normal frame-based retirement continues below pressure. The existing `Finish` operations and other renderer
backends retain their current paths.

The synchronization contract is defined by
[glFenceSync](https://registry.khronos.org/OpenGL-Refpages/gl4/html/glFenceSync.xhtml) and
[glClientWaitSync](https://registry.khronos.org/OpenGL-Refpages/gl4/html/glClientWaitSync.xhtml).

## Validation

Host tests cover exact budget boundaries, incoming storage, arithmetic overflow and retirement bytes across
frame buckets. The native OpenGL test performs 64 successive buffer expansions, clears and copies while the
retirement ring receives no frame ticks. With a 512 MiB target, it reclaimed retired storage ten times,
peaked at 495,910,912 owned bytes and preserved the copied data. The test also checks a missing completion fence.

An initial mission run completed the Iceland loading interval and continued presenting frames through a requested
shutdown, with no device loss or container OOM. Its scene image remained black. Four buffer reclamations followed
the ownership footprint. A further 227 waits were triggered solely by the driver estimate in the initial candidate.
The next candidate uses ownership-based pressure for buffer retirement. Scene rendering and game regressions
remain pending validation.

## Rejected options

- Immediate GL deletion alone leaves resources referenced by the current Mesa command batch.
- Cleanup restricted to completed submitted batches leaves the current recording batch and Eden's retired
  objects alive. That candidate passed a native replay and still failed the original mission.
- Increasing guest memory changes the emulated console's allocation behavior while the retained objects belong
  to the host renderer.
- Waiting on every buffer allocation serializes allocations even when the ownership footprint fits its target.
