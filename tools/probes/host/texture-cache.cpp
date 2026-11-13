#include <algorithm>
#include <array>
#include <cstdint>
#include <cstdio>
#include <deque>
#include <map>
#include <optional>
#include <stdexcept>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

using u64 = std::uint64_t;
using GPUVAddr = u64;
using DAddr = u64;
using ImageId = unsigned;
using ImageMapId = unsigned;
using ImageViewId = unsigned;
constexpr unsigned NUM_RT = 8;
struct Dirty {
    enum { RenderTargets, ZetaBuffer, ColorBuffer0 };
};

#define ASSERT_MSG(condition, message, ...) Require(condition, message)
#define ASSERT(condition) Require(static_cast<bool>(condition), #condition)

void Require(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

namespace Common {
template <typename T>
using IdentityHash = std::hash<T>;

u64 AlignUp(u64 value, u64 alignment) {
    return (value + alignment - 1) & ~(alignment - 1);
}
}

namespace ankerl::unordered_dense {
template <typename K, typename V, typename H = std::hash<K>>
using map = std::unordered_map<K, V, H>;
}

namespace boost::container {
template <typename T, std::size_t N>
using small_vector = std::vector<T>;
}

enum ImageFlagBits : unsigned {
    Registered = 1,
    Sparse = 2,
    BadOverlap = 4,
    AcceleratedUpload = 8,
    Converted = 16,
};

bool True(unsigned flags) { return flags != 0; }
bool False(unsigned flags) { return flags == 0; }
bool IsPixelFormatASTC(unsigned) { return false; }
u64 TranscodedAstcSize(u64 size, unsigned) { return size; }

struct ImageBase {
    unsigned flags{};
    GPUVAddr gpu_addr{0x1ff000};
    DAddr cpu_addr{0x9ff000};
    unsigned guest_size_bytes{0x102000};
    unsigned unswizzled_size_bytes{};
    std::size_t address_space_id{};
    std::size_t lru_index{};
    ImageMapId map_view_id{};
    struct { unsigned format{}; } info;
};

struct ImageMapView {
    GPUVAddr gpu_addr;
    DAddr cpu_addr;
    std::size_t size;
    ImageId image_id;
    bool picked{};
};

struct MapViews : std::map<ImageMapId, ImageMapView> {
    ImageMapId next_id{1};

    ImageMapId insert(GPUVAddr gpu, DAddr cpu, std::size_t size, ImageId image_id) {
        const auto id = next_id++;
        emplace(id, ImageMapView{gpu, cpu, size, image_id});
        return id;
    }
};

using TextureCacheGPUMap = ankerl::unordered_dense::map<u64, std::vector<ImageId>, Common::IdentityHash<u64>>;

template <class P>
struct TextureCache {
    using Image = ImageBase;
    static constexpr u64 YUZU_PAGEBITS = 20;

    struct Channel {
        std::size_t address_space;
        TextureCacheGPUMap* gpu_page_table;
        TextureCacheGPUMap* sparse_page_table;
    };

    std::deque<TextureCacheGPUMap> gpu_page_table_storage;
    std::map<std::size_t, std::size_t> address_spaces;
    std::map<unsigned, Channel> channels;
    Channel* channel_state{};
    std::size_t current_address_space{};
    struct Maxwell {
        struct { std::array<bool, NUM_RT + 2> flags{}; } dirty;
    };
    Maxwell* maxwell3d{};
    std::map<ImageId, Image> slot_images;
    MapViews slot_map_views;
    TextureCacheGPUMap page_table;
    std::map<ImageId, std::vector<ImageViewId>> sparse_views;
    u64 total_used_memory{};
    u64 frame_tick{};
    struct {
        std::size_t Insert(ImageId id, u64) { return id; }
        void Free(std::size_t) {}
    } lru_cache;

    std::optional<std::size_t> getStorageID(std::size_t as_id) const {
        const auto it = address_spaces.find(as_id);
        return it == address_spaces.end() ? std::nullopt : std::optional{it->second};
    }

    void CreateChannel(unsigned id, std::size_t as_id) {
        auto [it, inserted] = address_spaces.try_emplace(as_id, address_spaces.size());
        if (inserted) {
            gpu_page_table_storage.emplace_back();
            gpu_page_table_storage.emplace_back();
        }
        const auto index = it->second * 2;
        channels.emplace(id, Channel{as_id, &gpu_page_table_storage[index],
                                     &gpu_page_table_storage[index + 1]});
    }

    void BindToChannel(unsigned id) {
        channel_state = &channels.at(id);
        current_address_space = channel_state->address_space;
    }

    void EraseChannel(unsigned id) {
        if (channel_state == &channels.at(id)) {
            channel_state = nullptr;
        }
        channels.erase(id);
    }

    template <typename Func>
    static void ForEachGPUPage(u64 address, std::size_t size, Func&& func) {
        for (u64 page = address >> YUZU_PAGEBITS; page <= (address + size - 1) >> YUZU_PAGEBITS; ++page) {
            func(page);
        }
    }

    template <typename Func>
    static void ForEachCPUPage(u64 address, std::size_t size, Func&& func) {
        ForEachGPUPage(address, size, std::forward<Func>(func));
    }

    template <typename Func>
    void ForEachSparseSegment(ImageBase& image, Func&& func) {
        func(image.gpu_addr, image.cpu_addr, 0x1000);
        func(image.gpu_addr + 0x101000, image.cpu_addr + 0x401000, 0x1000);
    }

    void RegisterImage(ImageId image_id);
    void UnregisterImage(ImageId image_id);
    void MarkRenderTargetsDirty();
};

#include "texture-cache-methods.inc"

void Run(bool sparse, unsigned scenario) {
    TextureCache<void> cache;
    cache.CreateChannel(1, 41);
    cache.CreateChannel(2, 97);
    cache.CreateChannel(3, 41);
    cache.BindToChannel(1);
    cache.slot_images[1].flags = sparse ? ImageFlagBits::Sparse : 0;
    cache.RegisterImage(1);
    Require(cache.gpu_page_table_storage[0].size() == 3, "GPU registration");
    Require(cache.slot_map_views.size() == (sparse ? 2 : 1), "CPU registration");
    Require(cache.gpu_page_table_storage[1].size() == (sparse ? 3 : 0), "sparse registration");

    cache.BindToChannel(2);
    cache.slot_images[2].flags = sparse ? ImageFlagBits::Sparse : 0;
    cache.RegisterImage(2);
    const auto other_gpu = cache.gpu_page_table_storage[2];
    const auto other_sparse = cache.gpu_page_table_storage[3];
    if (scenario == 1) {
        cache.BindToChannel(1);
        cache.EraseChannel(1);
        cache.EraseChannel(3);
        Require(cache.channel_state == nullptr, "erased bound channel");
    } else if (scenario == 2) {
        cache.EraseChannel(1);
        cache.EraseChannel(3);
        cache.CreateChannel(1, 123);
        cache.BindToChannel(1);
    } else if (scenario == 3) {
        cache.EraseChannel(1);
        cache.BindToChannel(3);
        Require(cache.channel_state->gpu_page_table == &cache.gpu_page_table_storage[0], "shared AS");
        cache.slot_images[3].flags = sparse ? ImageFlagBits::Sparse : 0;
        cache.RegisterImage(3);
    }

    auto expected_gpu = cache.gpu_page_table_storage[0];
    auto expected_sparse = cache.gpu_page_table_storage[1];
    for (auto& [page, ids] : expected_gpu) {
        std::erase(ids, ImageId{1});
    }
    for (auto& [page, ids] : expected_sparse) {
        std::erase(ids, ImageId{1});
    }
    cache.UnregisterImage(1);
    Require(cache.gpu_page_table_storage[0] == expected_gpu, "owner GPU entries");
    Require(cache.gpu_page_table_storage[1] == expected_sparse, "owner sparse entries");
    Require(cache.gpu_page_table_storage[2] == other_gpu, "foreign GPU table changed");
    Require(cache.gpu_page_table_storage[3] == other_sparse, "foreign sparse table changed");
    for (const auto& [page, ids] : cache.page_table) {
        for (auto id : ids) {
            Require(cache.slot_map_views.at(id).image_id != 1, "stale CPU mapping");
        }
    }
    const unsigned remaining_images = scenario == 3 ? 2 : 1;
    Require(cache.slot_map_views.size() == remaining_images * (sparse ? 2 : 1), "remaining CPU mappings");
    Require(!cache.sparse_views.contains(1), "stale sparse views");
    Require(False(cache.slot_images[1].flags & ImageFlagBits::Registered), "registered flag");
    if (scenario == 3) {
        cache.UnregisterImage(3);
    }
    cache.BindToChannel(2);
    cache.UnregisterImage(2);
    Require(cache.slot_map_views.empty() && cache.sparse_views.empty(), "final cleanup");
}

int main(int argc, char** argv) {
    unsigned failures = 0;
    const unsigned scenarios = argc > 1 && std::string_view(argv[1]) == "--cross-as-only" ? 1 : 4;
    unsigned checks = scenarios * 2;
    for (bool sparse : {false, true}) {
        for (unsigned scenario = 0; scenario < scenarios; ++scenario) {
            try {
                Run(sparse, scenario);
                std::printf("sparse=%d scenario=%u PASS\n", sparse, scenario);
            } catch (const std::exception& error) {
                std::printf("sparse=%d scenario=%u FAIL %s\n", sparse, scenario, error.what());
                ++failures;
            }
        }
    }
    if (scenarios != 1) {
        TextureCache<void> cache;
        cache.MarkRenderTargetsDirty();
        ++checks;
        TextureCache<void>::Maxwell maxwell;
        cache.maxwell3d = &maxwell;
        cache.MarkRenderTargetsDirty();
        Require(std::ranges::all_of(maxwell.dirty.flags, [](bool dirty) { return dirty; }), "render target dirty flags");
        ++checks;
    }
    std::printf("TEXTURE CACHE RESULT checks=%u failures=%u\n", checks, failures);
    return failures ? 1 : 0;
}
