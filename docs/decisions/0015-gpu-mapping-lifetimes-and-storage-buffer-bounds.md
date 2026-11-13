# GPU mapping lifetimes and storage buffer bounds

## Context

Eden is pinned to `58c1e20ee58efa3900ba616207d460886214480b`. Source locations below refer to that
revision unless they name a Lab patch.

`src/video_core/buffer_cache/buffer_cache.h:1889` reads a storage buffer address and size. Patches
0023 and 0027 add indirect descriptors and a continuity limit, but explicit descriptor sizes and
the cbuf 0 size heuristic return before that limit. The cache represents a binding as one linear
device-address interval. A guessed size can therefore register unrelated memory and schedule
writeback into it.

`src/core/hle/service/nvdrv/core/nvmap.cpp:273` frees a guest handle by unmapping its SMMU range and
clearing its pin count, even while GMMU mappings retain pins. `TryRemoveHandle` at line 116 also
ignores pins. `devices/nvhost_as_gpu.cpp:255` pins sparse Remap entries without retaining ownership
that can be released when an interval is replaced or freed.

Patch 0078 searches through only one addition in a pointer's low word and two in its high word.
This can miss descriptor-loaded pointers and reach the without-bias search, which traverses the
descriptor load and finds the descriptor-set address in cbuf instead of the loaded buffer address.
The exact missed expression in the reported mission run is not verified without its IR.

## Decision

Keep patch 0084's address-space ownership on registered images. Address-space table storage outlives
channels in `src/video_core/control/channel_state_cache.inc:71`; unregistering an image must use its
recorded owner, including after channel erasure or reuse. The null Maxwell guard in `DeleteImage`
permits cleanup after the bound channel has been erased.

Patch 0085 makes every storage binding respect both the remaining layout size and
`MaxContinuousRange`. Address translation is validated before traversing that range. Alignment
prefixes must be contiguous, zero sizes produce a null binding, and wide size arithmetic must fit
the binding's u32 field. The 8 MiB ceiling remains for sizes inferred solely from the memory layout.
An explicit NVN size can legitimately exceed 8 MiB. The bits `{address, size, 0}` cannot establish
whether a cbuf 0 word belongs to NVN or to an NVK root table; mapping bounds are the sound limit
available here, rather than a new arbitrary ceiling on explicit sizes.

Patch 0086 retains the handle and SMMU mapping until the last GPU pin is released. Guest CPU unlock
eligibility from patch 0018 remains independent of that lifetime. Sparse Remap intervals share a
pin owner across splits; replacement, allocation release and address-space destruction release the
owner after removing its GMMU references. Remap operations use the existing address-space mutex.
Bound GPU devices retain the address-space owner until they close. ChannelState itself is retained
in Eden's GPU registry and is not a suitable owner for driver cleanup. Session closure retires its
user references and records that its heap mapper is being destroyed. A late unpin cannot use that
mapper or a replacement session at the same session index; closed handles cannot be pinned again.

No second cache invalidation is added. `src/core/device_memory_manager.inc:249` already invalidates
the renderer before clearing SMMU entries, and `NvMap::UnmapHandle` calls Unmap before Free. Both
renderers invalidate textures, buffers, shaders and queries. BufferCache `WriteMemory` at line 116
also cancels downloads for GPU-modified ranges through `ClearDownload` at line 184. The heap mapper
uses the same SMMU Unmap path when its final overlapping mapping disappears. These are source
contracts, not a completed asynchronous renderer integration test.

Patch 0087 searches bounded addition trees for matching words of the same pointer load and rejects
conflicting descriptor pairs. It handles nested constant IAdd64 offsets with checked arithmetic.
It also recognizes direct addresses built from two adjacent, aligned, constant cbuf words with
matching cbuf indices. This path proves both words rather than searching dependencies without bias.
For a computed 32-bit offset, it requires the upper word to add a zero-or-one selection of the carry
from the exact lower-word addition. The cbuf words must still form the same adjacent pair. Reordered
addition operands are supported; a carry from another instruction or an inverted selection is not.
Phi, arbitrary Select, masked or shifted bases, dynamic IAdd64 operands and commuted IAdd64 operands
remain unsupported; following them requires proving the pointer value and descriptor identity on
every path.

Patch 0090 is temporary diagnostic instrumentation. It records only cbuf 0 without-bias fallbacks
at offsets 56, 88, 1792 and 1800, with a process-wide atomic limit of 64 messages, 96 unique IR nodes and four
arguments per node. It identifies nodes and edges and marks truncation. Remove it once the remaining
misses have been identified; the permanent host tests also work without this patch.

## Rejected alternatives

- Clearing pins on guest release loses active GPU mappings and their balancing unpins.
- Suppressing CPU unlock while GPU pins exist would regress patch 0018's DeviceShared fix.
- Permanently pinning every Remap entry leaks mappings on replacement and sparse unmap.
- Treating every dataflow dependency as a transparent pointer transformation can bind a different
  buffer, particularly across a conditional or a mask.

## Validation

Host tests extract the patched methods rather than reimplementing the decisions under test. They
use small memory, GMMU and IR fixtures; the Remap interval test uses Eden's actual `Common::RangeMap`.
Local MSVC ARM64 checks reproduce failures with the old unregister, binding, nvmap and tracking
methods and pass with the patches. Tests cover large legitimate NVN sizes, zero and oversized
bindings, split Remap replacement, guest CPU unlock, deeper addition trees, conflicting pointers
and diagnostic filtering and bounds. The build script runs `host-tests` before compiling Eden.

The final local runs pass 10 texture-cache cases, 15 storage-binding cases, 51 nvmap lifetime
cases and 37 indirect-storage cases, plus the existing memory-budget checks and eight Python
file-lookup tests. All 84 patch files apply in order with a separate `git apply --check` before
each application onto a fresh detached checkout of the pinned Eden revision. The reconstructed
121 changed source files match the tested tree byte for byte. Added patch lines contain no code
comments, fit 120 columns and use LF endings.

The initial validation above did not include a Docker build or game run. The Linux host-test pass
on 2026-11-13 exposed undefined behavior in the existing Remap allocation lookup: its post-decrement
decremented `begin()` even when rejecting an empty allocation map. Patch 0086 now checks `begin()`
before decrementing. Additional cases reject addresses before the first allocation and past the last
without acquiring a pin or changing GMMU mappings.

After that correction, GCC 14 Linux ARM64 and MSVC ARM64 pass 10 texture-cache, 15 storage-binding,
55 nvmap lifetime and 37 indirect-storage checks. The nvmap tests also pass with libstdc++ checked
iterators. Linux additionally passes nine page-table checks, 33 memory-budget checks and eight Python
file-lookup tests. These tests do not establish that world geometry renders, that mission loading
time improves, or that every indirect IR pattern is handled; live verification remains separate.

The first instrumented Docker run on 2026-11-13 reported ten cbuf 0 fallbacks at offsets 56 and 88.
Their IR was a direct `LoadGlobal128` from a packed adjacent cbuf pair plus 16 bytes, not a nested
pointer loaded from memory. The direct-address path above handles this proven case. Its tests cover
both observed offsets, other indices, displacement preservation, alignment, mismatched or dynamic
words and opaque transformations. GCC 14 Linux ARM64 and MSVC ARM64 pass 60 storage IR checks after
this addition; the four regression groups total 140 checks, and the complete Linux suite totals 190.

A second diagnostic pass captured compute accesses at cbuf offsets 1792 and 1800. They add a computed
32-bit offset to the low pointer word and the exact low-word carry to the high pointer word. The carry
recognizer covers these expressions, including reads, writes and constant outer displacements.
Tests reject mismatched carry nodes, inverted or scaled carry selections and transformed pointer
bases. With the expanded diagnostic filter tests, both compilers pass 104 storage IR checks and 184
checks across the four regression groups. The complete Linux suite passes 234 checks.

The final 2026-11-13 Docker run compiled 180 graphics and nine compute pipelines from an empty Eden
pipeline cache. It recorded zero page-unregister warnings, pin imbalances, unmapped-device messages,
without-bias storage fallbacks and diagnostic IR records. The automated New Game / Easy route reached
Iceland. Plane geometry and textures rendered, but the scene after the jump remained black with HUD
elements, including after movement. This does not pass the world-rendering criterion. Download staging
storage still peaked at 520097792 bytes; this measures reserved storage, not readback traffic.
Patch 0090 remains because the complete live verification has not passed. Removing the observed IR
misses is not evidence that the remaining black-world defect is fixed.
