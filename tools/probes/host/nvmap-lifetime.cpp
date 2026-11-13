#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <functional>
#include <limits>
#include <list>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <span>
#include <stdexcept>
#include <vector>
#include "common/range_map.h"

using GPUVAddr = u64;
using DAddr = u64;
using VAddr = u64;

void Require(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

unsigned warnings = 0;
#define LOG_WARNING(...) ++warnings
#define LOG_DEBUG(...)
#define LOG_CRITICAL(...) throw std::runtime_error("critical nvmap error")
constexpr size_t BIG_PAGE_SIZE = 65536;
namespace Common {
u64 AlignUp(u64 value, u64 alignment) { return (value + alignment - 1) & ~(alignment - 1); }
}

enum class NvResult { Success, BadValue, InsufficientMemory };
namespace Tegra {
enum class PTEKind { Pitch };
}
namespace NvCore {
struct SessionId { unsigned id{}; };
}

struct Memory {
    std::vector<int> events;
    std::function<void(u64, size_t)> before_unmap;
    u32 next_address{0x900000};
    u32 Allocate(size_t size) {
        const u32 address = next_address;
        next_address += static_cast<u32>(size);
        return address;
    }
    template <class... T>
    void Map(u64, u64, size_t, T...) {}
    void Unmap(u64 address, size_t size) {
        if (before_unmap) {
            before_unmap(address, size);
        }
        events.push_back(1);
    }
    void Free(u64, size_t) { events.push_back(2); }
};
struct Host {
    Memory memory;
    Memory gmmu_manager;
    Memory allocator;
    Memory& MemoryManager() { return memory; }
    Memory& Allocator() { return allocator; }
};
struct Mapper {
    unsigned unmaps = 0;
    void Unmap(u64, size_t) { ++unmaps; }
    bool IsInBounds(u64, size_t) { return true; }
    u64 Map(u64, size_t) { return 0x900000; }
};
struct Core {
    struct Session {
        std::unique_ptr<Mapper> mapper = std::make_unique<Mapper>();
        bool has_preallocated_area{};
        unsigned asid{};
    } session;
    Session* GetSession(NvCore::SessionId) { return &session; }
};

struct NvMap {
    struct Handle {
        using Id = u32;
        std::mutex mutex;
        Id id{4};
        s32 dupes{1};
        s32 internal_dupes{};
        s64 pins{};
        DAddr d_address{0x900000};
        VAddr address{0x800000};
        u64 size{0x100000};
        u64 aligned_size{0x100000};
        u32 pin_virt_address{};
        bool in_heap{};
        bool session_closed{};
        NvCore::SessionId session_id{};
        struct Flags {
            struct Field { unsigned Value() const { return 1; } } map_uncached;
        } flags;
        std::optional<std::list<std::shared_ptr<Handle>>::iterator> unmap_queue_entry;
    };
    struct FreeInfo { u64 address; u64 size; bool was_uncached; bool can_unlock; };
    std::map<u32, std::shared_ptr<Handle>> handles;
    std::mutex handles_lock;
    std::mutex unmap_queue_lock;
    std::list<std::shared_ptr<Handle>> unmap_queue;
    Host host1x;
    Core core;

    std::shared_ptr<Handle> GetHandle(u32 id) {
        auto it = handles.find(id);
        return it == handles.end() ? nullptr : it->second;
    }
    DAddr PinHandle(u32, bool);
    void UnmapHandle(Handle&);
    bool TryRemoveHandle(const Handle&);
    void UnpinHandle(u32);
    std::optional<FreeInfo> FreeHandle(u32, bool);
    void UnmapAllHandles(NvCore::SessionId);
    std::shared_ptr<Handle> Add(u32 id = 4) {
        auto handle = std::make_shared<Handle>();
        handle->id = id;
        handles[id] = handle;
        return handle;
    }
};

#include "nvmap-methods.inc"

namespace NvCore {
using NvMap = ::NvMap;
}

struct Gmmu {
    std::map<u64, DAddr> pages;
    void Unmap(u64 address, u64 size) {
        for (u64 page = address; page < address + size; page += 0x1000) {
            pages.erase(page);
        }
    }
    void MapSparse(u64 address, u64 size, bool) { Unmap(address, size); }
    void Map(u64 address, u64 device, u64 size, Tegra::PTEKind, bool) {
        for (u64 offset = 0; offset < size; offset += 0x1000) {
            pages[address + offset] = device + offset;
        }
    }
};

namespace Tegra {
using MemoryManager = ::Gmmu;
}
struct nvhost_gpu {
    struct Channel {
        std::shared_ptr<Gmmu> memory_manager;
    };
    std::unique_ptr<Channel> channel_state = std::make_unique<Channel>();
    std::shared_ptr<void> address_space_owner;
};
struct Module {
    std::shared_ptr<nvhost_gpu> gpu = std::make_shared<nvhost_gpu>();
    template <class T>
    std::shared_ptr<T> GetDevice(int) { return gpu; }
} test_module;

struct nvhost_as_gpu : std::enable_shared_from_this<nvhost_as_gpu> {
    explicit nvhost_as_gpu(NvMap& map) : nvmap{map} {}
    NvMap& nvmap;
    Module& module{test_module};
    std::mutex mutex;
    struct Allocator {
        void Free(u32, u32) {}
    };
    struct VM {
        static constexpr u32 YUZU_PAGESIZE = 4096;
        static constexpr u32 PAGE_SIZE_BITS = 12;
        bool initialised{true};
        u32 big_page_size_bits{12};
        u32 big_page_size{4096};
        std::optional<Allocator> big_page_allocator{Allocator{}};
        std::optional<Allocator> small_page_allocator{Allocator{}};
    } vm;
    struct Allocation {
        u64 size{0x10000};
        bool sparse{true};
        bool big_pages{true};
        u32 page_size{4096};
        std::vector<u64> mappings;
    };
    struct Mapping {
        u64 ptr;
        u64 offset;
        u64 size;
        u32 handle;
        bool fixed;
        bool big_page;
        bool sparse_alloc;
    };
    std::map<u64, Mapping> mapping_map;
    std::set<u64> map_buffer_offsets;
    std::map<u64, Allocation> allocation_map{{0x10000, {}}};
    std::shared_ptr<Gmmu> gmmu = std::make_shared<Gmmu>();
    struct IoctlRemapEntry {
        unsigned flags{};
        unsigned kind{};
        u32 handle{};
        u32 handle_offset_big_pages{};
        u32 as_offset_big_pages{0x10};
        u32 big_pages{4};
    };
#include "remap-handle.inc"
    Common::RangeMap<GPUVAddr, std::shared_ptr<RemapHandle>> remapped_handles{nullptr};
    struct IoctlFreeSpace { u64 offset; u32 pages; u32 page_size; };
    struct IoctlUnmapBuffer { u64 offset; };
    struct IoctlBindChannel { int fd; };
    ~nvhost_as_gpu();
    NvResult Remap(std::span<IoctlRemapEntry>);
    bool FreeMappingLocked(u64) noexcept;
    NvResult FreeSpace(IoctlFreeSpace&);
    NvResult UnmapBuffer(IoctlUnmapBuffer&);
    NvResult BindChannel(IoctlBindChannel&);
    void Map(u32 handle, u32 start = 0x10, u32 pages = 4, u32 offset = 0) {
        IoctlRemapEntry entry{0, 0, handle, offset, start, pages};
        Require(Remap(std::span{&entry, 1}) == NvResult::Success, "remap result");
    }
};

#include "remap-method.inc"
#include "as-cleanup.inc"

int main() try {
    unsigned checks = 0;
    for (bool heap : {false, true}) {
        NvMap map;
        auto handle = map.Add();
        map.core.session.has_preallocated_area = heap;
        handle->in_heap = heap;
        map.PinHandle(4, false);
        map.PinHandle(4, false);
        auto info = map.FreeHandle(4, false);
        Require(info && info->can_unlock && info->was_uncached, "0018 guest unlock");
        Require(handle->pins == 2 && handle->d_address && map.GetHandle(4), "live GMMU pins");
        map.UnpinHandle(4);
        Require(handle->pins == 1 && handle->d_address, "remaining pin");
        map.UnpinHandle(4);
        Require(handle->pins == 0 && !handle->d_address && !map.GetHandle(4), "last pin cleanup");
        Require(map.unmap_queue.empty(), "released handle queued");
        Require(heap ? map.core.session.mapper->unmaps == 1
                     : map.host1x.memory.events == std::vector<int>{1, 2}, "unmap before free");
        checks += 6;
    }
    {
        NvMap map;
        auto handle = map.Add();
        nvhost_as_gpu as{map};
        as.Map(4);
        map.UnmapAllHandles(NvCore::SessionId{0});
        nvhost_as_gpu::IoctlRemapEntry closed{0, 0, 4, 0, 0x10, 4};
        Require(as.Remap(std::span{&closed, 1}) == NvResult::InsufficientMemory, "closed Remap result");
        Require(handle->pins == 1 && handle->d_address, "failed pin cannot unpin an existing owner");
        checks += 2;
    }
    {
        NvMap map;
        auto handle = map.Add();
        handle->internal_dupes = 1;
        map.PinHandle(4, false);
        auto info = map.FreeHandle(4, false);
        Require(info->can_unlock, "0018 internal owner unlock");
        map.UnpinHandle(4);
        Require(map.GetHandle(4) && !handle->d_address, "internal owner retained");
        handle.reset();
        info = map.FreeHandle(4, true);
        Require(info->can_unlock && !map.GetHandle(4), "internal owner released");
        checks += 3;
    }
    {
        NvMap map;
        auto handle = map.Add();
        handle->dupes = 2;
        Require(!map.FreeHandle(4, false)->can_unlock, "user duplicate unlock");
        map.PinHandle(4, false);
        map.UnpinHandle(4);
        Require(map.unmap_queue.size() == 1, "idle handle reuse queue");
        Require(map.FreeHandle(4, false)->can_unlock, "last user unlock");
        Require(!handle->d_address && map.unmap_queue.empty() && !map.GetHandle(4), "idle handle free");
        checks += 4;
    }
    {
        NvMap map;
        auto old_handle = map.Add();
        auto new_handle = map.Add(8);
        new_handle->d_address += 0x100000;
        nvhost_as_gpu as{map};
        map.host1x.memory.before_unmap = [&](u64 address, size_t size) {
            const bool referenced = std::ranges::any_of(as.gmmu->pages, [&](const auto& page) {
                return page.second >= address && page.second < address + size;
            });
            Require(!referenced, "SMMU unmap preceded GMMU release");
        };
        as.Map(4);
        map.FreeHandle(4, false);
        Require(old_handle->pins == 1 && old_handle->d_address, "remap guest free");
        as.Map(8, 0x11, 2);
        Require(old_handle->pins == 1 && new_handle->pins == 1, "split mapping ownership");
        as.Map(0, 0x10, 1);
        Require(old_handle->pins == 1, "split tail retained");
        as.Map(0, 0x13, 1);
        Require(old_handle->pins == 0 && !old_handle->d_address, "last split released");
        for (unsigned i = 0; i < 20; ++i) {
            as.Map(8, 0x11, 2);
        }
        Require(new_handle->pins == 1, "replacement pin leak");
        map.FreeHandle(8, false);
        nvhost_as_gpu::IoctlFreeSpace free_space{0x10000, 16, 4096};
        Require(as.FreeSpace(free_space) == NvResult::Success, "FreeSpace result");
        Require(new_handle->pins == 0 && !new_handle->d_address && !map.GetHandle(8), "allocation free");
        nvhost_as_gpu::IoctlRemapEntry invalid{0, 0, 0, 0, 0x40, 4};
        Require(as.Remap(std::span{&invalid, 1}) == NvResult::BadValue, "invalid allocation");
        map.host1x.memory.before_unmap = {};
        checks += 8;
    }
    for (bool explicit_unmap : {false, true}) {
        NvMap map;
        auto handle = map.Add();
        nvhost_as_gpu as{map};
        as.mapping_map.emplace(0x10000, nvhost_as_gpu::Mapping{handle->d_address, 0x10000, 4096, 4, true, true, true});
        as.allocation_map.at(0x10000).mappings.push_back(0x10000);
        as.map_buffer_offsets.insert(0x10000);
        as.gmmu->Map(0x10000, map.PinHandle(4, false), 4096, Tegra::PTEKind::Pitch, true);
        map.FreeHandle(4, false);
        map.host1x.memory.before_unmap = [&](u64, size_t) {
            Require(as.gmmu->pages.empty(), "GMMU cleanup order");
        };
        if (explicit_unmap) {
            nvhost_as_gpu::IoctlUnmapBuffer unmap{0x10000};
            Require(as.UnmapBuffer(unmap) == NvResult::Success, "UnmapBuffer result");
            Require(as.allocation_map.at(0x10000).mappings.empty(), "0011 fixed mapping list");
        } else {
            nvhost_as_gpu::IoctlFreeSpace free_space{0x10000, 16, 4096};
            Require(as.FreeSpace(free_space) == NvResult::Success, "fixed FreeSpace result");
        }
        Require(!map.GetHandle(4) && !handle->d_address && handle->pins == 0, "fixed mapping final unpin");
        map.host1x.memory.before_unmap = {};
        checks += explicit_unmap ? 3 : 2;
    }
    {
        NvMap map;
        auto handle = map.Add();
        {
            nvhost_as_gpu as{map};
            as.Map(4);
            map.FreeHandle(4, false);
            map.host1x.memory.before_unmap = [&](u64, size_t) {
                Require(as.gmmu->pages.empty(), "destruction cleanup order");
            };
        }
        map.host1x.memory.before_unmap = {};
        Require(!map.GetHandle(4) && handle->pins == 0 && !handle->d_address, "address space destruction");
        ++checks;
    }
    Require(warnings == 1, "pin or duplicate imbalance");
    for (u32 start : {0x0fu, 0x20u}) {
        NvMap map;
        auto handle = map.Add();
        nvhost_as_gpu as{map};
        nvhost_as_gpu::IoctlRemapEntry invalid{0, 0, 4, 0, start, 1};
        Require(as.Remap(std::span{&invalid, 1}) == NvResult::BadValue, "outside allocation rejected");
        Require(handle->pins == 0 && as.gmmu->pages.empty(), "invalid remap leaves mappings untouched");
        checks += 2;
    }
    {
        NvMap map;
        auto handle = map.Add();
        auto as = std::make_shared<nvhost_as_gpu>(map);
        as->Map(4);
        nvhost_as_gpu::IoctlBindChannel bind{1};
        Require(as->BindChannel(bind) == NvResult::Success, "BindChannel result");
        std::weak_ptr<nvhost_as_gpu> owner = as;
        map.FreeHandle(4, false);
        as.reset();
        Require(!owner.expired() && handle->pins == 1 && handle->d_address, "bound channel retains address space");
        test_module.gpu->address_space_owner.reset();
        Require(owner.expired() && handle->pins == 0 && !handle->d_address, "last channel releases address space");
        test_module.gpu->channel_state->memory_manager.reset();
        checks += 3;
    }
    for (bool heap : {false, true}) {
        NvMap map;
        map.core.session.has_preallocated_area = heap;
        auto handle = map.Add();
        auto other = map.Add(8);
        other->session_id.id = 1;
        map.PinHandle(4, false);
        handle->internal_dupes = 1;
        map.FreeHandle(4, false);
        map.UnmapAllHandles(NvCore::SessionId{0});
        Require(handle->session_closed && !other->session_closed, "session closure scope");
        Require(handle->pins == 1 && handle->d_address, "session retains live pin");
        Require(map.PinHandle(4, false) == 0 && handle->pins == 1, "closed session repin");
        map.core.session.mapper = std::make_unique<Mapper>();
        map.UnpinHandle(4);
        Require(!handle->d_address && handle->pins == 0, "late session unpin");
        Require(map.core.session.mapper->unmaps == 0, "replacement mapper lifetime");
        map.FreeHandle(4, true);
        Require(!map.GetHandle(4), "closed session internal release");
        checks += 6;
    }
    std::printf("NVMAP LIFETIME RESULT checks=%u failures=0\n", checks + 1);
} catch (const std::exception& error) {
    std::printf("NVMAP LIFETIME FAIL %s\n", error.what());
    return 1;
}
