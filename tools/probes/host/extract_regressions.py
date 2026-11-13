import pathlib
import re
import sys


def extract(text, signature, prefix=None):
    start = text.index(signature)
    if prefix:
        start = text.rindex(prefix, 0, start)
    opening = text.index("{", text.index(signature))
    depth = 0
    tokens = r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|//[^\n]*|/\*[\s\S]*?\*/|[{}]'
    for token in re.finditer(tokens, text[opening:]):
        if token.group() == "{":
            depth += 1
        elif token.group() == "}":
            depth -= 1
            if depth == 0:
                return text[start:opening + token.end()] + "\n"
    raise ValueError(signature)


root = pathlib.Path(sys.argv[1]) / "src"
out = pathlib.Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
buffer = (root / "video_core/buffer_cache/buffer_cache.h").read_text(encoding="utf-8")
nvmap = (root / "core/hle/service/nvdrv/core/nvmap.cpp").read_text(encoding="utf-8")
as_gpu = (root / "core/hle/service/nvdrv/devices/nvhost_as_gpu.cpp").read_text(encoding="utf-8")
as_header = (root / "core/hle/service/nvdrv/devices/nvhost_as_gpu.h").read_text(encoding="utf-8")
shader = (root / "shader_recompiler/ir_opt/global_memory_to_storage_buffer_pass.cpp").read_text(encoding="utf-8")
bfs = (root / "shader_recompiler/frontend/ir/breadth_first_search.h").read_text(encoding="utf-8")
outputs = {
    "indirect-diagnostic.inc": (
        "#define LAB_TEST_INDIRECT_DIAGNOSTIC 1\n" + extract(shader, "void LogWithoutBiasIR(")
        if "void LogWithoutBiasIR(" in shader else ""
    ),
    "indirect-types.inc": "\n".join(extract(shader, signature) + ";\n" for signature in (
        "struct StorageBufferAddr {",
        "struct Bias {",
        "struct LowAddrInfo {",
        "struct IndirectInfo {",
    )),
    "storage-binding.inc": extract(buffer, "Binding BufferCache<P>::StorageBufferBinding(", "template <class P>"),
    "nvmap-methods.inc": "\n".join(extract(nvmap, signature) for signature in (
        "void NvMap::UnmapHandle(",
        "bool NvMap::TryRemoveHandle(",
        "DAddr NvMap::PinHandle(",
        "void NvMap::UnpinHandle(",
        "std::optional<NvMap::FreeInfo> NvMap::FreeHandle(",
        "void NvMap::UnmapAllHandles(",
    )),
    "remap-handle.inc": extract(as_header, "struct RemapHandle {") + ";\n",
    "remap-method.inc": extract(as_gpu, "NvResult nvhost_as_gpu::Remap("),
    "as-cleanup.inc": "\n".join(extract(as_gpu, signature) for signature in (
        "nvhost_as_gpu::~nvhost_as_gpu(",
        "bool nvhost_as_gpu::FreeMappingLocked(",
        "NvResult nvhost_as_gpu::FreeSpace(",
        "NvResult nvhost_as_gpu::UnmapBuffer(",
        "NvResult nvhost_as_gpu::BindChannel(",
    )),
    "indirect-methods.inc": "\n".join(extract(shader, signature) for signature in (
        "std::optional<LowAddrInfo> TrackLowAddress(",
        "bool MeetsBias(",
        "std::optional<StorageBufferAddr> Track(",
        "std::optional<StorageBufferAddr> TrackDirectWords(",
        "bool IsCarryOf(",
        "std::optional<StorageBufferAddr> TrackDirect(",
        "bool IsGlobalPointerLoad(",
        "const IR::Inst* PointerWord(",
        "std::optional<std::vector<const IR::Inst*>> PointerWordsInSum(",
        "std::optional<IndirectInfo> TrackIndirect(",
    )),
    "ir-bfs.inc": extract(bfs, "auto BreadthFirstSearch(", "template <typename Pred>"),
}
for name, content in outputs.items():
    (out / name).write_text(content, encoding="utf-8", newline="\n")
