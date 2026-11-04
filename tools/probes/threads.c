#define COBJMACROS
#include "probe.h"

#include <emmintrin.h>
#include <stdlib.h>

#include <d3d9.h>

#define PROBE_MAX_THREADS 512
#define PROBE_CHURN 300
#define PROBE_STACK (64 * 1024)

static HANDLE hold_event;
static volatile LONG hold_started;
static HANDLE held[PROBE_MAX_THREADS];
static unsigned int held_count;
static HANDLE kept[PROBE_CHURN];
static DWORD hold_release_ms = INFINITE;
static HANDLE go_event;

static DWORD WINAPI hold_thread(void *arg)
{
    HANDLE events[2];

    InterlockedIncrement(&hold_started);
    if (((UINT_PTR)arg & 1) && hold_release_ms != INFINITE)
    {
        events[0] = hold_event;
        events[1] = go_event;
        if (WaitForMultipleObjects(2, events, FALSE, INFINITE) == WAIT_OBJECT_0 + 1)
            Sleep(hold_release_ms);
        return 0;
    }
    WaitForSingleObject(hold_event, INFINITE);
    return 0;
}

static DWORD WINAPI quick_thread(void *arg)
{
    (void)arg;
    return 0;
}

static HANDLE start_thread(LPTHREAD_START_ROUTINE proc, UINT_PTR index)
{
    return CreateThread(NULL, PROBE_STACK, proc, (void *)index, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
}

static void free_address_space(unsigned long *free_mb, unsigned long *largest_mb)
{
    MEMORY_BASIC_INFORMATION info;
    ULONG_PTR address = 0x10000;
    ULONG_PTR total = 0, largest = 0;

    while (address < 0xFFFF0000u && VirtualQuery((void *)address, &info, sizeof(info)) == sizeof(info))
    {
        if (info.State == MEM_FREE)
        {
            total += info.RegionSize;
            if (info.RegionSize > largest)
                largest = info.RegionSize;
        }
        if (info.RegionSize == 0)
            break;
        address = (ULONG_PTR)info.BaseAddress + info.RegionSize;
    }
    *free_mb = (unsigned long)(total >> 20);
    *largest_mb = (unsigned long)(largest >> 20);
}

static unsigned int hold_all(const char *phase)
{
    unsigned long free_mb, largest_mb;
    DWORD error = 0;
    unsigned int waited;

    ResetEvent(hold_event);
    hold_started = 0;
    held_count = 0;
    while (held_count < PROBE_MAX_THREADS)
    {
        HANDLE thread = start_thread(hold_thread, held_count);

        if (!thread)
        {
            error = GetLastError();
            break;
        }
        held[held_count++] = thread;
    }
    for (waited = 0; (unsigned int)hold_started < held_count && waited < 1000; waited++)
        Sleep(10);
    free_address_space(&free_mb, &largest_mb);
    probe_log("ceiling %s: %u threads, started %ld, refused with error %lu; address space free %lu MB, largest %lu MB",
              phase, held_count, hold_started, error, free_mb, largest_mb);
    return held_count;
}

static void release_all(void)
{
    unsigned int i;
    unsigned int stuck = 0;
    unsigned int count = held_count;
    DWORD start = GetTickCount();

    SetEvent(hold_event);
    for (i = 0; i < held_count; i++)
    {
        if (WaitForSingleObject(held[i], 10000) != WAIT_OBJECT_0)
            stuck++;
        CloseHandle(held[i]);
    }
    if (stuck)
        probe_log("release: %u threads did not end in 10 s", stuck);
    if (count)
        probe_log("released %u threads in %lu ms", count, GetTickCount() - start);
    held_count = 0;
}

static void churn_joined(void)
{
    unsigned int i;
    unsigned int refused = 0;

    for (i = 0; i < PROBE_CHURN; i++)
    {
        HANDLE thread = start_thread(quick_thread, 0);

        if (!thread)
        {
            refused++;
            continue;
        }
        WaitForSingleObject(thread, 10000);
        CloseHandle(thread);
    }
    probe_log("churn joined: %u threads, %u refused", PROBE_CHURN, refused);
}

static void churn_detached(void)
{
    unsigned int i;
    unsigned int refused = 0;
    DWORD error = 0;

    for (i = 0; i < PROBE_CHURN; i++)
    {
        HANDLE thread = start_thread(quick_thread, 0);

        if (!thread)
        {
            refused++;
            error = GetLastError();
            Sleep(10);
            continue;
        }
        CloseHandle(thread);
        if (i % 10 == 9)
            Sleep(1);
    }
    probe_log("churn detached: %u threads, %u refused, last error %lu", PROBE_CHURN, refused, error);
}

static unsigned int churn_kept(void)
{
    unsigned int i;
    unsigned int count = 0;
    unsigned int refused = 0;

    for (i = 0; i < PROBE_CHURN; i++)
    {
        HANDLE thread = start_thread(quick_thread, 0);

        if (!thread)
        {
            refused++;
            continue;
        }
        WaitForSingleObject(thread, 10000);
        kept[count++] = thread;
    }
    probe_log("churn with open handles: %u threads, %u refused", PROBE_CHURN, refused);
    return count;
}

#define PROBE_LOCK_ROUNDS 2000

static CRITICAL_SECTION lock_section;
static HANDLE lock_start;
static volatile LONG lock_slow;
static volatile LONG lock_longest_ms;
static volatile LONG lock_entries;

static DWORD WINAPI lock_thread(void *arg)
{
    LARGE_INTEGER before, after;
    unsigned int i;

    (void)arg;
    WaitForSingleObject(lock_start, INFINITE);
    for (i = 0; i < PROBE_LOCK_ROUNDS; i++)
    {
        LONG ms, longest;

        QueryPerformanceCounter(&before);
        EnterCriticalSection(&lock_section);
        QueryPerformanceCounter(&after);
        lock_entries++;
        LeaveCriticalSection(&lock_section);
        ms = (LONG)((after.QuadPart - before.QuadPart) * 1000 / probe_frequency.QuadPart);
        if (ms >= 100)
            InterlockedIncrement(&lock_slow);
        longest = lock_longest_ms;
        while (ms > longest && InterlockedCompareExchange(&lock_longest_ms, ms, longest) != longest)
            longest = lock_longest_ms;
    }
    return 0;
}

static void lock_storm(unsigned int wanted)
{
    HANDLE threads[PROBE_MAX_THREADS];
    unsigned int count = 0, i;
    DWORD start;

    InitializeCriticalSectionAndSpinCount(&lock_section, 0);
    lock_start = CreateEventA(NULL, TRUE, FALSE, NULL);
    lock_slow = 0;
    lock_longest_ms = 0;
    lock_entries = 0;
    while (count < wanted && count < PROBE_MAX_THREADS)
    {
        HANDLE thread = start_thread(lock_thread, 0);

        if (!thread)
            break;
        threads[count++] = thread;
    }
    start = GetTickCount();
    SetEvent(lock_start);
    for (i = 0; i < count; i++)
    {
        WaitForSingleObject(threads[i], INFINITE);
        CloseHandle(threads[i]);
    }
    probe_log("lock storm: %u threads x %u entries in %lu ms, %ld waits over 100 ms, longest %ld ms, %ld entries",
              count, PROBE_LOCK_ROUNDS, GetTickCount() - start, lock_slow, lock_longest_ms, lock_entries);
    CloseHandle(lock_start);
    DeleteCriticalSection(&lock_section);
}

static volatile LONG spin_count;
static volatile LONG apc_value;

static DWORD WINAPI spin_thread(void *arg)
{
    (void)arg;
    for (;;)
    {
        InterlockedIncrement(&spin_count);
        Sleep(1);
    }
    return 0;
}

static DWORD WINAPI returning_thread(void *arg)
{
    return (DWORD)(UINT_PTR)arg;
}

static DWORD WINAPI alertable_thread(void *arg)
{
    (void)arg;
    return SleepEx(10000, TRUE) == WAIT_IO_COMPLETION ? 1 : 2;
}

static void CALLBACK apc_routine(ULONG_PTR value)
{
    InterlockedExchange(&apc_value, (LONG)value);
}

static void thread_lifecycle(void)
{
    LONG before, after, resumed;
    DWORD code = 0, start, took, waited, suspended;
    CONTEXT context = {0};
    ULONG_PTR pc, sp;
    BOOL got_context;
    HANDLE thread;

    thread = start_thread(spin_thread, 0);
    Sleep(200);
    start = GetTickCount();
    suspended = SuspendThread(thread);
    took = GetTickCount() - start;
    context.ContextFlags = CONTEXT_CONTROL | CONTEXT_INTEGER;
    got_context = GetThreadContext(thread, &context);
    Sleep(50);
    before = spin_count;
    Sleep(300);
    after = spin_count;
    ResumeThread(thread);
    Sleep(300);
    resumed = spin_count;
    probe_log("lifecycle: suspend %s (%ld then %ld while suspended, returned %ld in %lu ms), resume %s (%ld after)",
              before == after && suspended == 0 ? "ok" : "FAILED", before, after, (long)suspended, took,
              resumed > after ? "ok" : "FAILED", resumed);
#ifdef _WIN64
    pc = context.Rip;
    sp = context.Rsp;
#else
    pc = context.Eip;
    sp = context.Esp;
#endif
    probe_log("lifecycle: context %s (pc %llx sp %llx)", got_context && pc && sp ? "ok" : "FAILED",
              (unsigned long long)pc, (unsigned long long)sp);
    start = GetTickCount();
    TerminateThread(thread, 77);
    waited = WaitForSingleObject(thread, 5000);
    took = GetTickCount() - start;
    GetExitCodeThread(thread, &code);
    probe_log("lifecycle: terminate %s in %lu ms, exit code %lu",
              waited == WAIT_OBJECT_0 && code == 77 ? "ok" : "FAILED", took, code);
    CloseHandle(thread);

    thread = start_thread(returning_thread, 42);
    waited = WaitForSingleObject(thread, 5000);
    GetExitCodeThread(thread, &code);
    probe_log("lifecycle: exit code %s (%lu)", waited == WAIT_OBJECT_0 && code == 42 ? "ok" : "FAILED", code);
    CloseHandle(thread);

    thread = start_thread(alertable_thread, 0);
    Sleep(300);
    start = GetTickCount();
    QueueUserAPC(apc_routine, thread, 5);
    WaitForSingleObject(thread, 15000);
    took = GetTickCount() - start;
    GetExitCodeThread(thread, &code);
    probe_log("lifecycle: apc %s (value %ld, the alertable sleep %s after %lu ms)", apc_value == 5 ? "ok" : "FAILED",
              apc_value, code == 1 ? "woke for it" : "ran out", took);
    CloseHandle(thread);
}

enum wait_kind
{
    WAIT_EVENT,
    WAIT_ADDRESS,
    WAIT_SRW,
    WAIT_CONDITION,
    WAIT_SECTION,
    WAIT_KEYED,
    WAIT_SLEEP,
    WAIT_ALERTABLE,
    WAIT_SPIN,
    WAIT_SHORT_SLEEPS,
    WAIT_KIND_COUNT
};

static const char *const wait_names[WAIT_KIND_COUNT] =
{
    "event", "address", "srw lock", "condition variable", "critical section", "keyed event", "sleep",
    "alertable sleep", "spinning", "short sleeps",
};

#define PROBE_SLEEP_MS 1500
#define PROBE_STORM_ROUNDS 300

typedef BOOL (WINAPI *wait_on_address_fn)(volatile void *, void *, SIZE_T, DWORD);
typedef void (WINAPI *wake_by_address_fn)(void *);
typedef LONG (NTAPI *keyed_create_fn)(HANDLE *, ACCESS_MASK, void *, ULONG);
typedef LONG (NTAPI *keyed_call_fn)(HANDLE, const void *, BOOLEAN, const LARGE_INTEGER *);
typedef LONG (NTAPI *query_thread_fn)(HANDLE, int, void *, ULONG, ULONG *);

struct thread_basic
{
    LONG exit_status;
    void *teb;
    void *process;
    void *thread;
    ULONG_PTR affinity;
    LONG priority;
    LONG base_priority;
};

struct waiter
{
    enum wait_kind kind;
    HANDLE thread;
    HANDLE event;
    SRWLOCK srw;
    SRWLOCK condition_lock;
    CONDITION_VARIABLE condition;
    CRITICAL_SECTION section;
    volatile LONG address;
    volatile LONG entered;
    volatile LONG done;
    volatile LONG finished;
    volatile LONG spurious;
    volatile LONG spins;
    DWORD slept_ms;
};

struct grab
{
    HANDLE thread;
    DWORD suspended;
    DWORD error;
    BOOL got;
    CONTEXT context;
    double ms;
};

static wait_on_address_fn wait_on_address;
static wake_by_address_fn wake_by_address_all;
static keyed_create_fn keyed_create;
static keyed_call_fn keyed_wait;
static keyed_call_fn keyed_release;
static query_thread_fn query_thread;
static HANDLE keyed_event;

static void CALLBACK nothing_apc(ULONG_PTR value)
{
    (void)value;
}

static DWORD WINAPI waiter_thread(void *arg)
{
    struct waiter *w = arg;
    LONG zero = 0;
    DWORD start;

    switch (w->kind)
    {
    case WAIT_EVENT:
        InterlockedExchange(&w->entered, 1);
        while (WaitForSingleObject(w->event, INFINITE) == WAIT_OBJECT_0 && !w->done)
            InterlockedIncrement(&w->spurious);
        break;
    case WAIT_ADDRESS:
        InterlockedExchange(&w->entered, 1);
        while (!w->done)
        {
            wait_on_address(&w->address, &zero, sizeof(zero), INFINITE);
            if (!w->done)
                InterlockedIncrement(&w->spurious);
        }
        break;
    case WAIT_SRW:
        InterlockedExchange(&w->entered, 1);
        AcquireSRWLockExclusive(&w->srw);
        if (!w->done)
            InterlockedIncrement(&w->spurious);
        ReleaseSRWLockExclusive(&w->srw);
        break;
    case WAIT_CONDITION:
        AcquireSRWLockExclusive(&w->condition_lock);
        InterlockedExchange(&w->entered, 1);
        while (!w->done)
        {
            SleepConditionVariableSRW(&w->condition, &w->condition_lock, INFINITE, 0);
            if (!w->done)
                InterlockedIncrement(&w->spurious);
        }
        ReleaseSRWLockExclusive(&w->condition_lock);
        break;
    case WAIT_SECTION:
        InterlockedExchange(&w->entered, 1);
        EnterCriticalSection(&w->section);
        if (!w->done)
            InterlockedIncrement(&w->spurious);
        LeaveCriticalSection(&w->section);
        break;
    case WAIT_KEYED:
        InterlockedExchange(&w->entered, 1);
        keyed_wait(keyed_event, (const void *)&w->address, FALSE, NULL);
        if (!w->done)
            InterlockedIncrement(&w->spurious);
        break;
    case WAIT_SLEEP:
        InterlockedExchange(&w->entered, 1);
        start = GetTickCount();
        Sleep(PROBE_SLEEP_MS);
        w->slept_ms = GetTickCount() - start;
        break;
    case WAIT_ALERTABLE:
        InterlockedExchange(&w->entered, 1);
        while (!w->done)
            if (SleepEx(INFINITE, TRUE) != WAIT_IO_COMPLETION || !w->done)
                InterlockedIncrement(&w->spurious);
        break;
    case WAIT_SPIN:
        InterlockedExchange(&w->entered, 1);
        while (!w->done)
            InterlockedIncrement(&w->spins);
        break;
    case WAIT_SHORT_SLEEPS:
        InterlockedExchange(&w->entered, 1);
        while (!w->done)
        {
            Sleep(1);
            InterlockedIncrement(&w->spins);
        }
        break;
    case WAIT_KIND_COUNT:
        break;
    }
    InterlockedExchange(&w->finished, 1);
    return 0;
}

static DWORD WINAPI keyed_release_thread(void *arg)
{
    LARGE_INTEGER timeout;

    timeout.QuadPart = -30000000;
    keyed_release(keyed_event, arg, FALSE, &timeout);
    return 0;
}

static struct waiter *waiter_start(enum wait_kind kind)
{
    struct waiter *w = calloc(1, sizeof(*w));
    DWORD waited;

    w->kind = kind;
    w->event = CreateEventA(NULL, TRUE, FALSE, NULL);
    InitializeSRWLock(&w->srw);
    InitializeSRWLock(&w->condition_lock);
    InitializeConditionVariable(&w->condition);
    InitializeCriticalSectionAndSpinCount(&w->section, 0);
    if (kind == WAIT_SRW)
        AcquireSRWLockExclusive(&w->srw);
    if (kind == WAIT_SECTION)
        EnterCriticalSection(&w->section);
    w->thread = CreateThread(NULL, PROBE_STACK, waiter_thread, w, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
    for (waited = 0; w->thread && !w->entered && waited < 200; waited++)
        Sleep(10);
    Sleep(200);
    return w;
}

static void waiter_wake(struct waiter *w)
{
    HANDLE helper;

    InterlockedExchange(&w->done, 1);
    switch (w->kind)
    {
    case WAIT_EVENT:
        SetEvent(w->event);
        break;
    case WAIT_ADDRESS:
        InterlockedExchange(&w->address, 1);
        wake_by_address_all((void *)&w->address);
        break;
    case WAIT_SRW:
        ReleaseSRWLockExclusive(&w->srw);
        break;
    case WAIT_CONDITION:
        AcquireSRWLockExclusive(&w->condition_lock);
        ReleaseSRWLockExclusive(&w->condition_lock);
        WakeConditionVariable(&w->condition);
        break;
    case WAIT_SECTION:
        LeaveCriticalSection(&w->section);
        break;
    case WAIT_KEYED:
        helper = CreateThread(NULL, 0, keyed_release_thread, (void *)&w->address, 0, NULL);
        if (helper)
            CloseHandle(helper);
        break;
    case WAIT_ALERTABLE:
        QueueUserAPC(nothing_apc, w->thread, 0);
        break;
    case WAIT_SLEEP:
    case WAIT_SPIN:
    case WAIT_SHORT_SLEEPS:
    case WAIT_KIND_COUNT:
        break;
    }
}

static DWORD WINAPI grab_thread(void *arg)
{
    struct grab *g = arg;
    LARGE_INTEGER before, after;

    QueryPerformanceCounter(&before);
    g->suspended = SuspendThread(g->thread);
    g->error = g->suspended == (DWORD)-1 ? GetLastError() : 0;
    g->context.ContextFlags = CONTEXT_CONTROL | CONTEXT_INTEGER;
    g->got = GetThreadContext(g->thread, &g->context);
    QueryPerformanceCounter(&after);
    g->ms = (double)(after.QuadPart - before.QuadPart) * 1000.0 / (double)probe_frequency.QuadPart;
    return 0;
}

static ULONG_PTR context_pc(const CONTEXT *context)
{
#ifdef _WIN64
    return context->Rip;
#else
    return context->Eip;
#endif
}

static ULONG_PTR context_sp(const CONTEXT *context)
{
#ifdef _WIN64
    return context->Rsp;
#else
    return context->Esp;
#endif
}

static BOOL sp_in_stack(HANDLE thread, ULONG_PTR sp)
{
    struct thread_basic basic = {0};
    const NT_TIB *tib;

    if (!query_thread || query_thread(thread, 0, &basic, sizeof(basic), NULL) || !basic.teb)
        return FALSE;
    tib = basic.teb;
    return sp >= (ULONG_PTR)tib->StackLimit && sp < (ULONG_PTR)tib->StackBase;
}

static void pc_place(ULONG_PTR pc, char *place, size_t size)
{
    HMODULE module = NULL;
    char path[MAX_PATH];
    const char *name;

    if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            (LPCSTR)pc, &module) || !GetModuleFileNameA(module, path, sizeof(path)))
    {
        snprintf(place, size, "%llx outside modules", (unsigned long long)pc);
        return;
    }
    name = strrchr(path, '\\');
    snprintf(place, size, "%s+0x%llx", name ? name + 1 : path, (unsigned long long)(pc - (ULONG_PTR)module));
}

static BOOL suspend_check(enum wait_kind kind)
{
    struct waiter *w = waiter_start(kind);
    struct grab g = {0};
    HANDLE grabber;
    LONG spins;
    BOOL hung, in_stack = FALSE, moved, ended, ok;
    char place[MAX_PATH + 32] = "none";

    if (!w->thread || !w->entered)
    {
        probe_log("suspend %s: FAILED, the waiting thread did not start", wait_names[kind]);
        return FALSE;
    }
    g.thread = w->thread;
    grabber = CreateThread(NULL, 0, grab_thread, &g, 0, NULL);
    hung = WaitForSingleObject(grabber, 5000) != WAIT_OBJECT_0;
    if (!hung)
    {
        in_stack = sp_in_stack(w->thread, context_sp(&g.context));
        pc_place(context_pc(&g.context), place, sizeof(place));
    }
    spins = w->spins;
    waiter_wake(w);
    Sleep(kind == WAIT_SLEEP ? PROBE_SLEEP_MS + 300 : 300);
    moved = w->finished || w->spins != spins;
    ResumeThread(w->thread);
    ended = WaitForSingleObject(w->thread, 5000) == WAIT_OBJECT_0;
    ok = !hung && g.got && g.suspended == 0 && in_stack && !moved && ended && !w->spurious && g.ms < 1000.0;
    if (hung)
        probe_log("suspend %s: FAILED, SuspendThread and GetThreadContext did not return in 5 s", wait_names[kind]);
    else if (g.suspended)
        probe_log("suspend %s: FAILED, SuspendThread returned %ld, error %lu", wait_names[kind], (long)g.suspended,
                  g.error);
    else
        probe_log("suspend %s: %s, context %s in %.1f ms, sp %llx %s the stack, pc %s, %s while suspended, %s after "
                  "resume, %ld spurious wake-ups",
                  wait_names[kind], ok ? "ok" : "FAILED", g.got ? "read" : "NOT read", g.ms,
                  (unsigned long long)context_sp(&g.context), in_stack ? "inside" : "OUTSIDE", place,
                  moved ? "RAN" : "held", ended ? "ended" : "DID NOT END", w->spurious);
    if (kind == WAIT_SLEEP && ended)
        probe_log("suspend sleep: a %u ms sleep took %lu ms with the thread suspended past its end",
                  PROBE_SLEEP_MS, w->slept_ms);
    if (grabber && !hung)
        CloseHandle(grabber);
    if (ended)
    {
        CloseHandle(w->thread);
        CloseHandle(w->event);
        DeleteCriticalSection(&w->section);
        free(w);
    }
    return ok;
}

struct storm
{
    struct waiter *waiters[WAIT_KIND_COUNT];
    unsigned int count;
    unsigned int failed;
    double longest_ms;
};

static DWORD WINAPI storm_thread(void *arg)
{
    struct storm *s = arg;
    LARGE_INTEGER before, after;
    CONTEXT context;
    unsigned int round, i;
    double ms;

    for (round = 0; round < PROBE_STORM_ROUNDS; round++)
    {
        QueryPerformanceCounter(&before);
        for (i = 0; i < s->count; i++)
            SuspendThread(s->waiters[i]->thread);
        for (i = 0; i < s->count; i++)
        {
            context.ContextFlags = CONTEXT_CONTROL | CONTEXT_INTEGER;
            if (!GetThreadContext(s->waiters[i]->thread, &context) || !context_sp(&context))
                s->failed++;
        }
        for (i = 0; i < s->count; i++)
            ResumeThread(s->waiters[i]->thread);
        QueryPerformanceCounter(&after);
        ms = (double)(after.QuadPart - before.QuadPart) * 1000.0 / (double)probe_frequency.QuadPart;
        if (ms > s->longest_ms)
            s->longest_ms = ms;
    }
    return 0;
}

static BOOL suspend_storm(void)
{
    static const enum wait_kind kinds[] =
    {
        WAIT_EVENT, WAIT_ADDRESS, WAIT_SRW, WAIT_CONDITION, WAIT_SECTION, WAIT_KEYED, WAIT_ALERTABLE, WAIT_SPIN,
        WAIT_SHORT_SLEEPS,
    };
    struct storm s = {0};
    HANDLE stormer;
    DWORD start, took;
    LONG spurious = 0;
    unsigned int i, ended = 0;
    BOOL hung, ok;

    for (i = 0; i < sizeof(kinds) / sizeof(kinds[0]); i++)
    {
        struct waiter *w = waiter_start(kinds[i]);

        if (w->thread && w->entered)
            s.waiters[s.count++] = w;
    }
    start = GetTickCount();
    stormer = CreateThread(NULL, 0, storm_thread, &s, 0, NULL);
    hung = WaitForSingleObject(stormer, 60000) != WAIT_OBJECT_0;
    took = GetTickCount() - start;
    for (i = 0; i < s.count; i++)
        waiter_wake(s.waiters[i]);
    for (i = 0; i < s.count; i++)
    {
        if (WaitForSingleObject(s.waiters[i]->thread, 5000) == WAIT_OBJECT_0)
            ended++;
        spurious += s.waiters[i]->spurious;
    }
    ok = !hung && !s.failed && ended == s.count && !spurious;
    probe_log("suspend storm: %s, %u rounds over %u threads in %lu ms%s, %u contexts not read, longest round %.1f ms, "
              "%u of %u threads ended, %ld spurious wake-ups",
              ok ? "ok" : "FAILED", PROBE_STORM_ROUNDS, s.count, took, hung ? " and STILL GOING after 60 s" : "",
              s.failed, s.longest_ms, ended, s.count, spurious);
    return ok;
}

static volatile LONG hijacked;

static void WINAPI hijack_target(void)
{
    InterlockedExchange(&hijacked, 1);
    ExitThread(7);
}

static BOOL set_context_check(enum wait_kind kind)
{
    struct waiter *w = waiter_start(kind);
    struct grab g = {0};
    HANDLE grabber;
    DWORD code = 0;
    BOOL set = FALSE, ended, ok;

    hijacked = 0;
    if (!w->thread || !w->entered)
    {
        probe_log("set context %s: FAILED, the thread did not start", wait_names[kind]);
        return FALSE;
    }
    g.thread = w->thread;
    grabber = CreateThread(NULL, 0, grab_thread, &g, 0, NULL);
    if (WaitForSingleObject(grabber, 5000) != WAIT_OBJECT_0 || !g.got)
    {
        probe_log("set context %s: FAILED, the registers were not read", wait_names[kind]);
        return FALSE;
    }
    CloseHandle(grabber);
#ifdef _WIN64
    g.context.Rip = (ULONG_PTR)hijack_target;
    g.context.Rsp = (g.context.Rsp & ~(ULONG_PTR)15) - 8;
#else
    g.context.Eip = (ULONG_PTR)hijack_target;
    g.context.Esp = (g.context.Esp & ~(ULONG_PTR)15) - 4;
#endif
    g.context.ContextFlags = CONTEXT_CONTROL;
    set = SetThreadContext(w->thread, &g.context);
    ResumeThread(w->thread);
    ended = WaitForSingleObject(w->thread, 5000) == WAIT_OBJECT_0;
    GetExitCodeThread(w->thread, &code);
    ok = set && ended && hijacked && code == 7;
    probe_log("set context %s: %s, SetThreadContext %s, the new code %s, the thread %s with code %lu",
              wait_names[kind], ok ? "ok" : "FAILED", set ? "took it" : "REFUSED", hijacked ? "ran" : "DID NOT RUN",
              ended ? "ended" : "DID NOT END", code);
    if (!ended) InterlockedExchange(&w->done, 1);
    return ok;
}

#define PROBE_REGISTER_ROUNDS 200
#define PROBE_REGISTER_GAP 1000

static volatile LONG vector_stop;
static volatile LONG vector_bad;
static volatile LONG vector_spins;

__attribute__((target("sse2"))) static DWORD WINAPI vector_thread(void *arg)
{
    const __m128i step = _mm_set1_epi32(1);
    const __m128i gap = _mm_set1_epi32(PROBE_REGISTER_GAP);
    __m128i low = _mm_set_epi32(4, 3, 2, 1);
    __m128i high = _mm_add_epi32(low, gap);
    double float_low = 1.0;
    double float_high = float_low + PROBE_REGISTER_GAP;

    (void)arg;
    while (!vector_stop)
    {
        low = _mm_add_epi32(low, step);
        high = _mm_add_epi32(high, step);
        float_low += 1.0;
        float_high += 1.0;
        if (_mm_movemask_epi8(_mm_cmpeq_epi32(_mm_sub_epi32(high, low), gap)) != 0xffff
            || float_high - float_low != PROBE_REGISTER_GAP)
        {
            InterlockedIncrement(&vector_bad);
            high = _mm_add_epi32(low, gap);
            float_high = float_low + PROBE_REGISTER_GAP;
        }
        InterlockedIncrement(&vector_spins);
    }
    return 0;
}

static BOOL registers_check(void)
{
    HANDLE thread;
    CONTEXT context;
    unsigned int round;
    LONG spins;
    BOOL ok;

    vector_stop = 0;
    vector_bad = 0;
    vector_spins = 0;
    thread = CreateThread(NULL, PROBE_STACK, vector_thread, NULL, 0, NULL);
    if (!thread)
    {
        probe_log("registers: FAILED, the thread did not start");
        return FALSE;
    }
    Sleep(200);
    for (round = 0; round < PROBE_REGISTER_ROUNDS; round++)
    {
        if (SuspendThread(thread) == (DWORD)-1) break;
        context.ContextFlags = CONTEXT_FULL;
        GetThreadContext(thread, &context);
        ResumeThread(thread);
        Sleep(2);
    }
    spins = vector_spins;
    Sleep(100);
    InterlockedExchange(&vector_stop, 1);
    WaitForSingleObject(thread, 5000);
    CloseHandle(thread);
    ok = round == PROBE_REGISTER_ROUNDS && !vector_bad && vector_spins > spins;
    probe_log("registers: %s, %u suspensions of a thread computing in SSE registers, %ld values changed, %ld rounds of "
              "its loop", ok ? "ok" : "FAILED", round, vector_bad, vector_spins);
    return ok;
}

static BOOL terminate_check(enum wait_kind kind)
{
    struct waiter *w = waiter_start(kind);
    DWORD start, took, code = 0;
    BOOL ended, ok;

    if (!w->thread || !w->entered)
    {
        probe_log("terminate %s: FAILED, the thread did not start", wait_names[kind]);
        return FALSE;
    }
    start = GetTickCount();
    TerminateThread(w->thread, 9);
    ended = WaitForSingleObject(w->thread, 5000) == WAIT_OBJECT_0;
    took = GetTickCount() - start;
    GetExitCodeThread(w->thread, &code);
    ok = ended && code == 9 && took < 1000;
    probe_log("terminate %s: %s, the thread %s in %lu ms with code %lu", wait_names[kind], ok ? "ok" : "FAILED",
              ended ? "ended" : "DID NOT END", took, code);
    if (!ended) waiter_wake(w);
    return ok;
}

static void suspend_checks(void)
{
    HMODULE kernelbase = GetModuleHandleA("kernelbase.dll");
    HMODULE ntdll = GetModuleHandleA("ntdll.dll");
    unsigned int kind, passed = 0, total = 0;

    wait_on_address = (wait_on_address_fn)(void *)GetProcAddress(kernelbase, "WaitOnAddress");
    wake_by_address_all = (wake_by_address_fn)(void *)GetProcAddress(kernelbase, "WakeByAddressAll");
    keyed_create = (keyed_create_fn)(void *)GetProcAddress(ntdll, "NtCreateKeyedEvent");
    keyed_wait = (keyed_call_fn)(void *)GetProcAddress(ntdll, "NtWaitForKeyedEvent");
    keyed_release = (keyed_call_fn)(void *)GetProcAddress(ntdll, "NtReleaseKeyedEvent");
    query_thread = (query_thread_fn)(void *)GetProcAddress(ntdll, "NtQueryInformationThread");
    if (!wait_on_address || !wake_by_address_all || !keyed_create || !keyed_wait || !keyed_release ||
        keyed_create(&keyed_event, EVENT_ALL_ACCESS, NULL, 0))
    {
        probe_log("suspend: FAILED, WaitOnAddress or keyed events are missing");
        return;
    }
    for (kind = 0; kind < WAIT_KIND_COUNT; kind++, total++)
        passed += suspend_check((enum wait_kind)kind);
    passed += suspend_storm();
    total++;
    passed += set_context_check(WAIT_SPIN);
    passed += set_context_check(WAIT_SLEEP);
    total += 2;
    passed += registers_check();
    total++;
    passed += terminate_check(WAIT_ADDRESS);
    passed += terminate_check(WAIT_CONDITION);
    passed += terminate_check(WAIT_EVENT);
    passed += terminate_check(WAIT_SLEEP);
    total += 4;
    probe_log("suspend: %u of %u ok", passed, total);
}

static IDirect3DDevice9 *create_device(IDirect3D9 *d3d, HWND window, HRESULT *hr)
{
    D3DPRESENT_PARAMETERS params = {0};
    IDirect3DDevice9 *device = NULL;

    params.Windowed = TRUE;
    params.SwapEffect = D3DSWAPEFFECT_DISCARD;
    params.BackBufferFormat = D3DFMT_X8R8G8B8;
    params.BackBufferWidth = PROBE_WIDTH;
    params.BackBufferHeight = PROBE_HEIGHT;
    params.hDeviceWindow = window;
    params.PresentationInterval = D3DPRESENT_INTERVAL_ONE;
    *hr = IDirect3D9_CreateDevice(d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
                                  D3DCREATE_HARDWARE_VERTEXPROCESSING, &params, &device);
    return SUCCEEDED(*hr) ? device : NULL;
}

static BOOL present(IDirect3DDevice9 *device, DWORD milliseconds)
{
    DWORD start = GetTickCount();
    unsigned int frame;

    for (frame = 1; GetTickCount() - start < milliseconds && probe_pump(); frame++)
    {
        int i;

        IDirect3DDevice9_BeginScene(device);
        for (i = 0; i < 4; i++)
        {
            const struct probe_quadrant *q = &probe_quadrants[i];
            D3DRECT rect = {q->left, q->top, q->right, q->bottom};

            IDirect3DDevice9_Clear(device, 1, &rect, D3DCLEAR_TARGET, D3DCOLOR_COLORVALUE(q->r, q->g, q->b, 1.0f),
                                   1.0f, 0);
        }
        IDirect3DDevice9_EndScene(device);
        if (FAILED(IDirect3DDevice9_Present(device, NULL, NULL, NULL, NULL)))
        {
            probe_log("Present failed");
            return FALSE;
        }
        probe_frame_done(frame);
    }
    return TRUE;
}

#define PROBE_SYSCALL_CALLS 200000
#define PROBE_SYSCALL_ROUNDS 5

typedef ULONG (NTAPI *processor_number_fn)(void);
typedef LONG (NTAPI *system_time_fn)(LARGE_INTEGER *);

static double best_ns_per_call(unsigned int which, processor_number_fn processor_number, system_time_fn system_time,
                               query_thread_fn query)
{
    double best = 0;
    unsigned int round, i;

    for (round = 0; round < PROBE_SYSCALL_ROUNDS; round++)
    {
        LARGE_INTEGER start, end, time;
        struct thread_basic basic;
        double ns;

        QueryPerformanceCounter(&start);
        for (i = 0; i < PROBE_SYSCALL_CALLS; i++)
        {
            if (which == 0) processor_number();
            else if (which == 1) system_time(&time);
            else query(GetCurrentThread(), 0, &basic, sizeof(basic), NULL);
        }
        QueryPerformanceCounter(&end);
        ns = (double)(end.QuadPart - start.QuadPart) * 1e9 / (double)probe_frequency.QuadPart / PROBE_SYSCALL_CALLS;
        if (!round || ns < best) best = ns;
    }
    return best;
}

static void syscall_costs(void)
{
    static const char *const names[] =
    {
        "NtGetCurrentProcessorNumber", "NtQuerySystemTime", "NtQueryInformationThread",
    };
    HMODULE ntdll = GetModuleHandleA("ntdll.dll");
    processor_number_fn processor_number =
        (processor_number_fn)(void *)GetProcAddress(ntdll, "NtGetCurrentProcessorNumber");
    system_time_fn system_time = (system_time_fn)(void *)GetProcAddress(ntdll, "NtQuerySystemTime");
    query_thread_fn query = (query_thread_fn)(void *)GetProcAddress(ntdll, "NtQueryInformationThread");
    unsigned int which;

    if (!processor_number || !system_time || !query)
    {
        probe_log("syscalls: FAILED, a system call is missing");
        return;
    }
    for (which = 0; which < sizeof(names) / sizeof(names[0]); which++)
        probe_log("syscalls: %s %.0f ns a call, best of %u rounds of %u", names[which],
                  best_ns_per_call(which, processor_number, system_time, query), PROBE_SYSCALL_ROUNDS,
                  PROBE_SYSCALL_CALLS);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    IDirect3D9 *d3d;
    IDirect3DDevice9 *device;
    IDirect3DDevice9 *second;
    HRESULT hr;
    unsigned int count;
    unsigned int i;

    (void)instance;
    (void)previous;
    (void)show;
    probe_open_log("threads");
    hold_event = CreateEventA(NULL, TRUE, FALSE, NULL);
    go_event = CreateEventA(NULL, TRUE, FALSE, NULL);
    window = probe_window("Carafe threads probe", probe_default_proc);
    probe_pump();
    if (command_line && strstr(command_line, "census"))
    {
        probe_log("census: no ceiling or churn phases");
        goto device;
    }

    hold_all("at start");
    release_all();
    hold_all("right after release");
    release_all();
    Sleep(3000);
    hold_all("3 s after release");
    release_all();

    churn_joined();
    hold_all("after joined churn");
    release_all();

    churn_detached();
    Sleep(3000);
    hold_all("after detached churn");
    release_all();

    count = churn_kept();
    hold_all("with open handles of ended threads");
    release_all();
    for (i = 0; i < count; i++)
        CloseHandle(kept[i]);
    Sleep(3000);
    hold_all("after closing those handles");
    release_all();

device:
    d3d = Direct3DCreate9(D3D_SDK_VERSION);
    if (!d3d)
    {
        probe_log("Direct3DCreate9 failed");
        return 3;
    }
    device = create_device(d3d, window, &hr);
    if (!device)
    {
        probe_log("CreateDevice failed: 0x%08lx", (unsigned long)hr);
        return 4;
    }
    probe_log("device ready");
    probe_log("idle for 15 s");
    present(device, 15000);
    second = NULL;
    if (command_line && strstr(command_line, "dxvk-hard"))
    {
        hold_all("with a Direct3D 9 device");
        probe_log("second device with every thread taken");
        second = create_device(d3d, window, &hr);
        probe_log("second device survived: 0x%08lx", (unsigned long)hr);
    }
    else if (command_line && strstr(command_line, "dxvk"))
    {
        hold_release_ms = strstr(command_line, "dxvk-late") ? 40000 : 2000;
        hold_all(hold_release_ms > 2000 ? "with a Direct3D 9 device, half of them ending 40 s after the device is asked for"
                                        : "with a Direct3D 9 device, half of them ending 2 s after the device is asked for");
        SetEvent(go_event);
        probe_log("second device with every thread taken");
        second = create_device(d3d, window, &hr);
        probe_log("second device survived: 0x%08lx", (unsigned long)hr);
    }
    else if (command_line && strstr(command_line, "lifecycle"))
    {
        thread_lifecycle();
    }
    else if (command_line && strstr(command_line, "suspend"))
    {
        suspend_checks();
    }
    else if (command_line && strstr(command_line, "syscalls"))
    {
        syscall_costs();
    }
    else if (command_line && strstr(command_line, "crash"))
    {
        volatile int *page = VirtualAlloc(NULL, 4096, MEM_COMMIT | MEM_RESERVE, PAGE_NOACCESS);

        probe_log("crash: writing to a page with no access at %p", (void *)page);
        *page = 1;
        probe_log("crash: the write went through");
    }
    else if (command_line && strstr(command_line, "abort"))
    {
        probe_log("abort: calling abort()");
        abort();
    }
    else if (command_line && strstr(command_line, "locks"))
    {
        lock_storm(4);
        lock_storm(16);
        lock_storm(48);
    }
    else
    {
        hold_all("with a Direct3D 9 device");
        probe_log("holding every thread for 30 s");
        present(device, 30000);
    }
    release_all();
    if (second)
        IDirect3DDevice9_Release(second);
    present(device, 5000);
    IDirect3DDevice9_Release(device);
    IDirect3D9_Release(d3d);
    probe_log("exit");
    return 0;
}
