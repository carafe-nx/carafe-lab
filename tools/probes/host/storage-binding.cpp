#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <limits>
#include <map>
#include <optional>
#include <stdexcept>

using u32 = std::uint32_t;
using u64 = std::uint64_t;
using GPUVAddr = u64;
using DAddr = u64;
using BufferId = unsigned;
constexpr u64 operator""_MiB(unsigned long long value) { return value * 1024 * 1024; }

void Require(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

#define ASSERT_MSG(condition, ...) Require(static_cast<bool>(condition), "address assertion")
#define LOG_DEBUG(...)

namespace Common {
u64 AlignDown(u64 value, u64 alignment) { return value & ~(alignment - 1); }
u64 AlignUp(u64 value, u64 alignment) { return AlignDown(value + alignment - 1, alignment); }
}
namespace Core {
constexpr u64 DEVICE_PAGESIZE = 4096;
}

struct Binding {
    DAddr device_addr{};
    u32 size{};
    BufferId buffer_id{};
};
constexpr Binding NULL_BINDING{0, 0, 99};

struct Memory {
    GPUVAddr start{0x100000};
    u64 layout{0x10000};
    u64 contiguous{0x10000};
    std::map<GPUVAddr, u64> words;

    template <class T>
    T Read(GPUVAddr address) { return static_cast<T>(words.at(address)); }
    u64 GetMemoryLayoutSize(GPUVAddr) { return layout; }
    u64 MaxContinuousRange(GPUVAddr address, u64 size) {
        if (address < start || address >= start + contiguous) {
            return 0;
        }
        return (std::min)(size, start + contiguous - address);
    }
    std::optional<DAddr> GpuToCpuAddress(GPUVAddr address) {
        return MaxContinuousRange(address, 1) ? std::optional{address + 0x800000} : std::nullopt;
    }
};

template <class P>
struct BufferCache {
    Memory* gpu_memory;
    struct Runtime {
        u32 alignment{256};
        u32 GetStorageBufferAlignment() const { return alignment; }
    } runtime;
    Binding StorageBufferBinding(GPUVAddr, u32, bool, bool) const;
};

#include "storage-binding.inc"

int main() try {
    unsigned checks = 0;
    auto check = [&](u64 requested, u64 layout, u64 contiguous, bool descriptor, u32 index,
                     u64 expected, u64 displacement = 0, bool written = true) {
        Memory memory;
        memory.layout = layout;
        memory.contiguous = contiguous;
        memory.words = {{0x10, memory.start + displacement}, {0x18, requested}};
        BufferCache<void> cache{&memory, {}};
        const auto result = cache.StorageBufferBinding(0x10, index, written, descriptor);
        if (result.size != expected) {
            std::printf("requested=%llu layout=%llu contiguous=%llu descriptor=%d index=%u actual=%u expected=%llu\n",
                        static_cast<unsigned long long>(requested), static_cast<unsigned long long>(layout),
                        static_cast<unsigned long long>(contiguous), descriptor, index, result.size,
                        static_cast<unsigned long long>(expected));
        }
        Require(result.size == expected, "storage binding size");
        Require((result.buffer_id == NULL_BINDING.buffer_id) == (expected == 0), "null binding");
        ++checks;
    };
    check(0x20141000, 0x2000000, 0x10000, false, 0, 0x10000);
    check(0x20141000, 0x2000000, 0x10000, true, 0, 0x10000);
    check(0x10000, 0x2000, 0x10000, true, 0, 0x2000);
    check(0x10000, 0x10000, 0, true, 0, 0);
    check(0, 0x10000, 0x10000, false, 0, 0);
    check(0, 0x10000, 0x10000, true, 0, 0x10000);
    check(16_MiB, 32_MiB, 32_MiB, false, 0, 16_MiB);
    check(16_MiB, 32_MiB, 32_MiB, true, 0, 16_MiB);
    check(0x100000000, 32_MiB, 32_MiB, false, 0, 8_MiB);
    check(0, 0x100010000, 32_MiB, false, 1, 8_MiB);
    check(0x10000, 0x10000, 0x10000, false, 0, 0xff00, 0x123);
    check(0x1200, 0x10000, 0x10000, true, 0, 0x1f00, 0x123, false);
    check(0xffffffff, 0x100000000, 0x100001000, true, 0, 0, 0x123);
    Memory memory;
    memory.start += 0x80;
    memory.words = {{0x10, memory.start}, {0x18, 0x1000}};
    BufferCache<void> cache{&memory, {}};
    Require(cache.StorageBufferBinding(0x10, 0, true, true).size == 0, "discontinuous alignment prefix");
    ++checks;
    memory.words[0x10] = 0;
    Require(cache.StorageBufferBinding(0x10, 0, true, true).size == 0, "null GPU address");
    ++checks;
    std::printf("STORAGE BINDING RESULT checks=%u failures=0\n", checks);
} catch (const std::exception& error) {
    std::printf("STORAGE BINDING FAIL %s\n", error.what());
    return 1;
}
