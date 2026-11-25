# Page traversal uses the mapping type

## Context

At Eden revision `58c1e20ee58efa3900ba616207d460886214480b`, `src/core/memory.cpp:552`
stores physical address minus virtual address in a process page-table entry. The difference is zero
for an identity mapping. `src/common/page_table.cpp:31` incorrectly treats that value as an unmapped page
when traversing the table.

`MapCodeMemory` in `src/core/hle/kernel/k_page_table_base.cpp:1174` validates the source and destination
before constructing a physical page group. A valid identity-mapped source then fails traversal with
`InvalidCurrentMemory`. Carbon's ASI loader reports error 8 when this prevents loading its widescreen module.

## Decision

Patch 0079 determines whether an entry is mapped from its existing `PageType`. The physical address
continues to be reconstructed from the stored difference and virtual address. Source permissions,
attributes, destination checks and allocation policy remain enforced by the kernel.

The host regression test links the patched traversal implementation and checks ordinary, debug and
GPU-cached identity mappings, consecutive pages, unmapped entries and the table boundary. A local allocation
backend supplies storage for the table; the test exercises Eden's traversal code directly.

## Alternatives

Moving the heap to avoid identity mappings would leave valid address layouts unsupported. Increasing
the guest address limit would change application behavior without correcting the erroneous mapping test.

## Validation

The nine-case test has five failures before the fix and none after it. With the same Carbon NSP,
runtime and heap placement, both ASI modules load successfully after the fix and Vulkan rendering starts.
The subsequent race regression encounters a separate GPU-cached write fault; successful module loading
does not establish that the race works or that the release is ready.
