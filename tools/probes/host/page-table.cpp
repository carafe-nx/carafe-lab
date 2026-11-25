#include <cstdio>
#include <cstdlib>
#include "common/page_table.h"

namespace Common {
void* AllocateMemoryPages(std::size_t size) noexcept {
    return std::calloc(1, size);
}
void FreeMemoryPages(void* base, std::size_t) noexcept {
    std::free(base);
}
}

int main() {
    Common::PageTable table;
    table.Resize(32, 12);
    constexpr u64 address = 0xbbe00000;
    auto& page = table.entries[address / 4096];
    Common::PageTable::TraversalEntry entry;
    Common::PageTable::TraversalContext context;
    unsigned failures = 0;
    const auto check = [&](const char* name, Common::PageType type, u64 offset, bool expected) {
        page.ptr.Store(type == Common::PageType::Memory ? 0x1000 : 0, type);
        page.addr = offset;
        bool actual = table.BeginTraversal(&entry, &context, Common::ProcessAddress(address));
        bool ok = actual == expected && (!actual || entry.phys_addr == address + offset);
        std::printf("%s actual=%d expected=%d physical=%llx %s\n", name, actual, expected,
                    static_cast<unsigned long long>(entry.phys_addr), ok ? "PASS" : "FAIL");
        failures += !ok;
    };
    check("mapped-offset", Common::PageType::Memory, 4096, true);
    check("mapped-identity", Common::PageType::Memory, 0, true);
    check("debug-identity", Common::PageType::DebugMemory, 0, true);
    check("cached-identity", Common::PageType::RasterizerCachedMemory, 0, true);
    check("unmapped", Common::PageType::Unmapped, 0, false);
    for (u64 i = 0; i < 3; ++i) {
        auto& current = table.entries[address / 4096 + i];
        current.ptr.Store(i < 2 ? 0x1000 : 0,
                          i < 2 ? Common::PageType::Memory : Common::PageType::Unmapped);
        current.addr = 0;
        current.block = i < 2 ? address : 0;
    }
    for (u64 i = 0; i < 3; ++i) {
        bool actual = i == 0
            ? table.BeginTraversal(&entry, &context, Common::ProcessAddress(address))
            : table.ContinueTraversal(&entry, &context);
        bool expected = i < 2;
        bool ok = actual == expected && (!actual || entry.phys_addr == address + i * 4096);
        std::printf("identity-range page=%llu actual=%d expected=%d %s\n",
                    static_cast<unsigned long long>(i), actual, expected, ok ? "PASS" : "FAIL");
        failures += !ok;
    }
    bool outside = table.BeginTraversal(&entry, &context, Common::ProcessAddress(1ULL << 32));
    std::printf("out-of-range actual=%d expected=0 %s\n", outside, !outside ? "PASS" : "FAIL");
    failures += outside;
    std::printf("PAGE TABLE RESULT checks=9 failures=%u\n", failures);
    return failures ? 1 : 0;
}
