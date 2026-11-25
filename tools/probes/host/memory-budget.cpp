#include <cstdio>
#include <memory>
#include "video_core/renderer_opengl/gl_memory_budget.h"
#include "video_core/delayed_destruction_ring.h"
#include "video_core/renderer_opengl/gl_staging_lease.h"

int main() {
    using namespace OpenGL::MemoryAccounting;
    constexpr std::uint64_t mib = 1024 * 1024;
    unsigned failures = 0;
    const auto check = [&](const char* name, bool passed) {
        std::printf("%s %s\n", passed ? "PASS" : "FAIL", name);
        failures += !passed;
    };
    check("default cache target", ReadBudget(nullptr) == 4096 * mib);
    check("configured cache target", ReadBudget("2048") == 2048 * mib);
    check("minimum cache target", ReadBudget("512") == 512 * mib);
    check("maximum cache target", ReadBudget("8192") == 8192 * mib);
    for (const char* invalid : {"", "0", "511", "8193", "2048x", "-1", "999999999999999999999999"})
        check(invalid, ReadBudget(invalid) == DefaultBudget);
    check("pressure follows free memory", UsageSince(4096 * mib, 1024 * mib) == 3072 * mib);
    check("free memory increase cannot underflow", UsageSince(1024 * mib, 4096 * mib) == 0);
    check("an allocation fitting the budget needs no reclamation", !ExceedsBudget(384 * mib, 128 * mib, 512 * mib));
    check("incoming storage participates in pressure", ExceedsBudget(384 * mib, 129 * mib, 512 * mib));
    check("an oversized live set remains under pressure", ExceedsBudget(513 * mib, 0, 512 * mib));
    check("allocation arithmetic cannot overflow", ExceedsBudget(UINT64_MAX, 1, 512 * mib));
    struct SizedResource {
        std::uint64_t bytes;
        std::uint64_t SizeBytes() const { return bytes; }
    };
    VideoCommon::DelayedDestructionRing<SizedResource, 8> sized_retirement;
    sized_retirement.Push({64 * mib});
    sized_retirement.Tick();
    sized_retirement.Push({128 * mib});
    check("retired bytes include every frame bucket", sized_retirement.SizeBytes() == 192 * mib);
    check("retired storage causes pressure without another frame",
          ExceedsBudget(256 * mib + sized_retirement.SizeBytes(), 128 * mib, 512 * mib));
    sized_retirement.Clear();
    check("reclamation removes retired byte pressure", sized_retirement.SizeBytes() == 0);
    std::size_t borrowers = 0;
    {
        StagingLease first{borrowers};
        {
            StagingLease second = first;
            StagingLease third = std::move(second);
            check("copied staging spans retain both borrowers", borrowers == 2);
            check("a copied span cannot insert the final GPU fence", !first.IsLast());
            second = third;
            check("copy assignment retains a staging borrower", borrowers == 3);
            second = std::move(third);
            check("move assignment releases only its old borrower", borrowers == 2);
        }
        check("an outer staging span prevents reclamation", borrowers == 1);
        check("the last CPU span owns the final GPU fence", first.IsLast());
    }
    check("staging becomes idle after its last CPU span", borrowers == 0);
    unsigned destroyed = 0;
    const auto release = [&](int* value) { delete value; ++destroyed; };
    using Owned = std::unique_ptr<int, decltype(release)>;
    VideoCommon::DelayedDestructionRing<Owned, 8> retired;
    retired.Push(Owned(new int(1), release));
    retired.Push(Owned(new int(2), release));
    retired.Tick();
    retired.Push(Owned(new int(3), release));
    check("retirement holds live resources", destroyed == 0);
    check("retired count spans every bucket", retired.Size() == 3);
    retired.Clear();
    check("draining releases every retired resource", destroyed == 3);
    check("empty retirement needs no GPU wait", retired.Size() == 0);
    retired.Push(Owned(new int(4), release));
    for (unsigned tick = 0; tick < 7; ++tick) retired.Tick();
    check("normal retirement survives a drain", destroyed == 3);
    retired.Tick();
    check("normal retirement completes after its full delay", destroyed == 4);
    return failures ? 1 : 0;
}
