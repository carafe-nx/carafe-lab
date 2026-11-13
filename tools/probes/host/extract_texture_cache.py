import pathlib
import sys


source = pathlib.Path(sys.argv[1]) / "src/video_core/texture_cache/texture_cache.h"
text = source.read_text(encoding="utf-8")
methods = []
for name in ("RegisterImage", "UnregisterImage"):
    signature = text.index(f"void TextureCache<P>::{name}(")
    start = text.rindex("template <class P>", 0, signature)
    end = text.index("\ntemplate <class P>", signature)
    methods.append(text[start:end])
signature = text.index("void TextureCache<P>::DeleteImage(")
start = text.index("\n", text.index("// Mark render targets as dirty", signature)) + 1
end = text.index("    const std::span<const ImageViewId>", start)
methods.append("template <class P>\nvoid TextureCache<P>::MarkRenderTargetsDirty() {\n" + text[start:end] + "}\n")
pathlib.Path(sys.argv[2]).write_text("\n".join(methods), encoding="utf-8", newline="\n")
