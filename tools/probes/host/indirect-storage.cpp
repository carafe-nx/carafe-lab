#include <algorithm>
#include <atomic>
#include <compare>
#include <cstdint>
#include <cstdio>
#include <format>
#include <iterator>
#include <limits>
#include <memory>
#include <optional>
#include <queue>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <vector>

using u32 = std::uint32_t;
using u64 = std::uint64_t;
using s32 = std::int32_t;
using s64 = std::int64_t;

void Require(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

namespace Common {
bool IsAligned(u32 value, u32 alignment) { return value % alignment == 0; }
}
namespace boost::container {
template <class T, size_t N>
using small_vector = std::vector<T>;
}
namespace IR {
enum class Type { U32, U64 };
enum class Opcode {
#define OPCODE(name, ...) name,
#include "shader_recompiler/frontend/ir/opcodes.inc"
#undef OPCODE
};

struct Inst;
struct Value {
    Inst* node{};
    u64 immediate{};
    bool wide{};
    Value() = default;
    explicit Value(Inst* value) : node{value} {}
    explicit Value(u32 value) : immediate{value} {}
    explicit Value(u64 value) : immediate{value}, wide{true} {}
    bool IsImmediate() const { return !node; }
    Inst* InstRecursive() const;
    u32 U32() const { return static_cast<u32>(immediate); }
    u64 U64() const { return immediate; }
    IR::Type Type() const { return wide ? IR::Type::U64 : IR::Type::U32; }
};
using U32 = Value;
using U64 = Value;

struct Inst {
    Opcode opcode;
    std::vector<Value> args;
    Opcode GetOpcode() const { return opcode; }
    Value Arg(size_t index) const { return args.at(index); }
    size_t NumArgs() const { return args.size(); }
};
Inst* Value::InstRecursive() const {
    Inst* result = node;
    while (result && result->opcode == Opcode::Identity) {
        result = result->Arg(0).node;
    }
    return result;
}
#include "ir-bfs.inc"
std::string_view NameOf(Opcode opcode) {
    switch (opcode) {
#define OPCODE(name, ...) case Opcode::name: return #name;
#include "shader_recompiler/frontend/ir/opcodes.inc"
#undef OPCODE
    }
    throw std::runtime_error("opcode name");
}
}

#include "indirect-types.inc"
#include "indirect-methods.inc"

namespace fmt {
using std::format_to;
}
std::vector<std::string> diagnostics;
std::vector<u32> diagnostic_offsets;
bool diagnostic_truncated = false;
void CaptureLog(u32 offset, bool truncated, const std::string& chain) {
    Require(offset == 56 || offset == 88 || offset == 1792 || offset == 1800, "diagnostic offset gate");
    diagnostics.push_back(chain);
    diagnostic_offsets.push_back(offset);
    diagnostic_truncated |= truncated;
}
#define LOG_WARNING(category, format, ...) CaptureLog(__VA_ARGS__)
#include "indirect-diagnostic.inc"

struct Fixture {
    std::vector<std::unique_ptr<IR::Inst>> nodes;
    IR::Value Make(IR::Opcode opcode, std::initializer_list<IR::Value> args) {
        auto node = std::make_unique<IR::Inst>(IR::Inst{opcode, args});
        auto* result = node.get();
        nodes.push_back(std::move(node));
        return IR::Value{result};
    }
    IR::Value Imm(u32 value) { return IR::Value{value}; }
    IR::Value Pack(IR::Value low, IR::Value high) {
        return Make(IR::Opcode::PackUint2x32, {Make(IR::Opcode::CompositeConstructU32x2, {low, high})});
    }
    IR::Value Cbuf(u32 offset) { return Make(IR::Opcode::GetCbufU32, {Imm(0), Imm(offset)}); }
    IR::Value Pointer(u32 offset = 88, u64 displacement = 16) {
        auto address = Pack(Cbuf(offset), Cbuf(offset + 4));
        address = Make(IR::Opcode::IAdd64, {address, IR::Value{displacement}});
        return Make(IR::Opcode::LoadGlobal128, {address});
    }
    IR::Value Word(IR::Value pointer, u32 element) {
        return Make(IR::Opcode::CompositeExtractU32x4, {pointer, Imm(element)});
    }
    IR::Value Carry(IR::Value addition) {
        const auto carry = Make(IR::Opcode::GetCarryFromOp, {addition});
        return Make(IR::Opcode::SelectU32, {carry, Imm(1), Imm(0)});
    }
    IR::Value Sum(IR::Value word, unsigned depth, bool reverse = false) {
        for (unsigned i = 0; i < depth; ++i) {
            const auto offset = Make(IR::Opcode::GetAttribute, {Imm(i)});
            word = reverse ? Make(IR::Opcode::IAdd32, {offset, word})
                           : Make(IR::Opcode::IAdd32, {word, offset});
        }
        return word;
    }
    std::optional<IndirectInfo> Track(IR::Value low, IR::Value high) {
        auto access = Make(IR::Opcode::LoadGlobal32, {Pack(low, high)});
        return TrackIndirect(*access.InstRecursive());
    }
};

int main() try {
    unsigned checks = 0;
    for (u32 offset : {56, 88, 1792, 1800}) {
        for (bool reverse_low : {false, true}) {
            for (bool reverse_high : {false, true}) {
                Fixture fixture;
                const auto index = fixture.Make(IR::Opcode::LocalInvocationId, {});
                const auto element = fixture.Make(IR::Opcode::CompositeExtractU32x3, {index, fixture.Imm(0)});
                const auto bytes = fixture.Make(IR::Opcode::ShiftLeftLogical32, {element, fixture.Imm(4)});
                const auto base = fixture.Cbuf(offset);
                const auto low = reverse_low ? fixture.Make(IR::Opcode::IAdd32, {base, bytes})
                                             : fixture.Make(IR::Opcode::IAdd32, {bytes, base});
                const auto upper = fixture.Cbuf(offset + 4);
                const auto carry = fixture.Carry(low);
                const auto high = reverse_high ? fixture.Make(IR::Opcode::IAdd32, {carry, upper})
                                               : fixture.Make(IR::Opcode::IAdd32, {upper, carry});
                for (auto opcode : {IR::Opcode::LoadGlobal32, IR::Opcode::WriteGlobal32}) {
                    const auto packed = fixture.Pack(low, high);
                    const auto address = fixture.Make(IR::Opcode::IAdd64, {packed, IR::Value{u64{12}}});
                    const auto access = fixture.Make(opcode, {address, fixture.Imm(17)});
                    const auto tracked = TrackLowAddress(access.node);
                    const auto result = tracked ? TrackDirect(*tracked) : std::nullopt;
                    Require(result && result->index == 0 && result->offset == offset && !result->indirect,
                            "direct compute pointer with carry");
                    Require(tracked->imm_offset == 12, "compute constant displacement");
                    ++checks;
                }
            }
        }
    }
    {
        Fixture fixture;
        const auto bytes = fixture.Make(IR::Opcode::GetAttribute, {fixture.Imm(0)});
        const auto low = fixture.Make(IR::Opcode::IAdd32, {bytes, fixture.Cbuf(1792)});
        const auto other_low = fixture.Make(IR::Opcode::IAdd32, {bytes, fixture.Cbuf(1792)});
        const auto real_carry = fixture.Make(IR::Opcode::GetCarryFromOp, {low});
        const auto inverted = fixture.Make(IR::Opcode::SelectU32, {real_carry, fixture.Imm(0), fixture.Imm(1)});
        const auto scaled = fixture.Make(IR::Opcode::SelectU32, {real_carry, fixture.Imm(2), fixture.Imm(0)});
        const auto unrelated = fixture.Make(IR::Opcode::SelectU32, {bytes, fixture.Imm(1), fixture.Imm(0)});
        for (const auto carry : {fixture.Imm(0), fixture.Carry(other_low), inverted, scaled, unrelated, real_carry}) {
            const auto high = fixture.Make(IR::Opcode::IAdd32, {fixture.Cbuf(1796), carry});
            Require(!TrackDirect(LowAddrInfo{low, 0, high}), "unproven compute carry");
            ++checks;
        }
        const auto wrong_high = fixture.Make(IR::Opcode::IAdd32, {fixture.Cbuf(1804), fixture.Carry(low)});
        Require(!TrackDirect(LowAddrInfo{low, 0, wrong_high}), "compute pointer words differ");
        const auto masked = fixture.Make(IR::Opcode::BitwiseAnd32, {fixture.Cbuf(1792), fixture.Imm(0xffff)});
        const auto transformed_low = fixture.Make(IR::Opcode::IAdd32, {bytes, masked});
        const auto transformed_high = fixture.Make(IR::Opcode::IAdd32,
                                                   {fixture.Cbuf(1796), fixture.Carry(transformed_low)});
        Require(!TrackDirect(LowAddrInfo{transformed_low, 0, transformed_high}), "transformed compute base");
        checks += 2;
    }
    for (u32 cbuf : {56U, 88U, 0U, 0xfffffff8U}) {
        Fixture fixture;
        const auto pointer = fixture.Pointer(cbuf);
        const auto address = TrackLowAddress(pointer.node);
        const auto result = address ? TrackDirect(*address) : std::nullopt;
        Require(result && result->index == 0 && result->offset == cbuf && !result->indirect,
                "direct adjacent cbuf pointer");
        Require(address->imm_offset == 16, "direct descriptor displacement");
        Require(!TrackIndirect(*pointer.node), "direct descriptor is not indirect");
        ++checks;
    }
    for (u32 index : {0, 3}) {
        Fixture fixture;
        const auto low = fixture.Make(IR::Opcode::GetCbufU32, {fixture.Imm(index), fixture.Imm(88)});
        const auto high = fixture.Make(IR::Opcode::GetCbufU32, {fixture.Imm(index), fixture.Imm(92)});
        const auto result = TrackDirect(LowAddrInfo{low, 0, high});
        Require(result && result->index == index, "direct cbuf index");
        const auto identity = fixture.Make(IR::Opcode::Identity, {low});
        Require(TrackDirect(LowAddrInfo{identity, 0, high}).has_value(), "direct identity");
        checks += 2;
    }
    {
        Fixture fixture;
        const auto low = fixture.Cbuf(88);
        const auto high = fixture.Cbuf(92);
        const auto dynamic = fixture.Make(IR::Opcode::GetAttribute, {fixture.Imm(0)});
        const auto other = fixture.Make(IR::Opcode::GetCbufU32, {fixture.Imm(1), fixture.Imm(92)});
        const auto dynamic_index = fixture.Make(IR::Opcode::GetCbufU32, {dynamic, fixture.Imm(92)});
        const auto dynamic_offset = fixture.Make(IR::Opcode::GetCbufU32, {fixture.Imm(0), dynamic});
        for (auto invalid : {fixture.Imm(0), fixture.Cbuf(88), fixture.Cbuf(96), other,
                             dynamic_index, dynamic_offset}) {
            Require(!TrackDirect(LowAddrInfo{low, 0, invalid}), "unproven direct high word");
            ++checks;
        }
        Require(!TrackDirect(LowAddrInfo{fixture.Imm(88), 0, high}), "immediate low word");
        Require(!TrackDirect(LowAddrInfo{fixture.Cbuf(92), 0, fixture.Cbuf(96)}), "unaligned direct pointer");
        Require(!TrackDirect(LowAddrInfo{fixture.Cbuf(0xfffffffcU), 0, fixture.Cbuf(0)}), "wrapped pointer");
        checks += 3;
        for (auto opcode : {IR::Opcode::BitwiseAnd32, IR::Opcode::ShiftLeftLogical32,
                            IR::Opcode::IAdd32, IR::Opcode::Phi, IR::Opcode::SelectU32}) {
            const auto transformed = fixture.Make(opcode, {low, fixture.Imm(0)});
            Require(!TrackDirect(LowAddrInfo{transformed, 0, high}), "transformed direct low word");
            ++checks;
        }
        const auto pointer = fixture.Pointer();
        Require(!TrackDirect(LowAddrInfo{fixture.Word(pointer, 0), 0, fixture.Word(pointer, 1)}),
                "loaded descriptor is indirect");
        ++checks;
    }
    for (u32 cbuf : {56, 88}) {
        for (unsigned depth : {0, 1, 2, 5, 20}) {
            for (bool reverse : {false, true}) {
                Fixture fixture;
                const auto pointer = fixture.Pointer(cbuf);
                const auto low = fixture.Word(pointer, 0);
                const auto high = fixture.Word(pointer, 1);
                const auto result = fixture.Track(fixture.Sum(low, depth, reverse),
                                                  fixture.Sum(high, depth + 2, !reverse));
                Require(result && result->storage_buffer.index == 0, "indirect tracking");
                Require(result->storage_buffer.offset == cbuf, "root cbuf offset");
                Require(result->storage_buffer.indirect_offset == 16, "descriptor offset");
                Require(result->pointer_low.node == low.node, "loaded low word offset base");
                ++checks;
            }
        }
    }
    {
        Fixture fixture;
        auto pointer = fixture.Pointer();
        auto load = pointer.InstRecursive();
        load->args[0] = fixture.Make(IR::Opcode::IAdd64, {load->Arg(0), IR::Value{u64{32}}});
        auto result = fixture.Track(fixture.Word(pointer, 2), fixture.Word(pointer, 3));
        Require(result && result->storage_buffer.indirect_offset == 56, "nested IAdd64 descriptor");
        ++checks;
    }
    {
        Fixture fixture;
        const auto first = fixture.Pointer();
        const auto second = fixture.Pointer(56);
        Require(!fixture.Track(fixture.Word(first, 0), fixture.Word(second, 1)), "mixed pointer words");
        Require(!fixture.Track(fixture.Word(first, 1), fixture.Word(first, 2)), "odd pointer element");
        Require(!fixture.Track(fixture.Sum(fixture.Word(first, 0), 100), fixture.Word(first, 1)), "node bound");
        const auto low = fixture.Make(IR::Opcode::IAdd32, {fixture.Word(first, 0), fixture.Word(second, 0)});
        const auto high = fixture.Make(IR::Opcode::IAdd32, {fixture.Word(first, 1), fixture.Word(second, 1)});
        Require(!fixture.Track(low, high), "ambiguous descriptor pairs");
        checks += 4;
    }
    {
        Fixture fixture;
        const auto first = fixture.Pointer();
        const auto second = fixture.Pointer();
        const auto low = fixture.Make(IR::Opcode::IAdd32, {fixture.Word(first, 0), fixture.Word(second, 0)});
        const auto high = fixture.Make(IR::Opcode::IAdd32, {fixture.Word(first, 1), fixture.Word(second, 1)});
        Require(!fixture.Track(low, high), "distinct loads of the same descriptor");
        ++checks;
    }
    for (auto opcode : {IR::Opcode::BitwiseAnd32, IR::Opcode::ShiftLeftLogical32,
                        IR::Opcode::ShiftRightLogical32, IR::Opcode::Phi, IR::Opcode::SelectU32}) {
        Fixture fixture;
        const auto pointer = fixture.Pointer();
        const auto masked = fixture.Make(opcode, {fixture.Word(pointer, 0), fixture.Imm(1)});
        Require(!fixture.Track(masked, fixture.Word(pointer, 1)), "unproven pointer transform");
        ++checks;
    }
    {
        Fixture fixture;
        const auto pointer = fixture.Pointer(88, 0x80000000);
        Require(!fixture.Track(fixture.Word(pointer, 0), fixture.Word(pointer, 1)), "descriptor offset overflow");
        ++checks;
    }
#ifdef LAB_TEST_INDIRECT_DIAGNOSTIC
    {
        Fixture fixture;
        const auto pointer = fixture.Pointer();
        const auto low = fixture.Sum(fixture.Word(pointer, 0), 100);
        const auto access = fixture.Make(IR::Opcode::LoadGlobal32, {fixture.Pack(low, fixture.Word(pointer, 1))});
        for (unsigned i = 0; i < 64; ++i) {
            LogWithoutBiasIR(*access.node, StorageBufferAddr{1, 88});
            LogWithoutBiasIR(*access.node, StorageBufferAddr{0, 104});
        }
        Require(diagnostics.empty(), "diagnostic cbuf gate");
        const u32 offsets[]{56, 88, 1792, 1800};
        for (unsigned i = 0; i < 128; ++i) {
            LogWithoutBiasIR(*access.node, StorageBufferAddr{0, offsets[i % 4]});
        }
        Require(diagnostics.size() == 64, "diagnostic rate bound");
        for (u32 offset : offsets) {
            Require(std::ranges::count(diagnostic_offsets, offset) == 16, "diagnostic observed offsets");
            ++checks;
        }
        Require(diagnostic_truncated, "diagnostic node bound");
        Require(diagnostics[0].find("LoadGlobal32") != std::string::npos, "diagnostic root opcode");
        Require(diagnostics[0].find("IAdd32") != std::string::npos, "diagnostic opcode chain");
        checks += 5;
    }
#endif
    std::printf("INDIRECT STORAGE RESULT checks=%u failures=0\n", checks);
} catch (const std::exception& error) {
    std::printf("INDIRECT STORAGE FAIL %s\n", error.what());
    return 1;
}
