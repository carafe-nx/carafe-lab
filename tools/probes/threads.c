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
#define PROBE_WAKE_ROUNDS 50
#define PROBE_WAKE_PAUSE_MS 20
#define PROBE_QUIET_MS 16000
#define PROBE_QUEUE_SHIFT 4
#define PROBE_QUEUE_COUNT 256
#define PROBE_QUEUE_SPAN (PROBE_QUEUE_COUNT << PROBE_QUEUE_SHIFT)
#define PROBE_QUEUE_ROUNDS 200
#define PROBE_QUEUE_WAKE_MS 1000

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

static void terminate_polled(enum wait_kind kind)
{
    struct waiter *w = waiter_start(kind);
    DWORD start, returned, signalled;

    if (!w->thread || !w->entered)
    {
        probe_log("terminate %s, polled: the thread did not start", wait_names[kind]);
        return;
    }
    start = GetTickCount();
    TerminateThread(w->thread, 9);
    returned = GetTickCount() - start;
    while (WaitForSingleObject(w->thread, 0) != WAIT_OBJECT_0 && GetTickCount() - start < 5000)
        Sleep(1);
    signalled = GetTickCount() - start;
    probe_log("terminate %s, polled: TerminateThread returned in %lu ms, the thread was signalled in %lu ms",
              wait_names[kind], returned, signalled);
}

struct wake_pair
{
    HANDLE ask, answer;
};

static DWORD WINAPI wake_answerer(void *param)
{
    struct wake_pair *pair = param;

    while (WaitForSingleObject(pair->ask, INFINITE) == WAIT_OBJECT_0)
        SetEvent(pair->answer);
    return 0;
}

static void wake_latency(void)
{
    struct wake_pair pair = { CreateEventA(NULL, FALSE, FALSE, NULL), CreateEventA(NULL, FALSE, FALSE, NULL) };
    LARGE_INTEGER frequency, start, end;
    double worst = 0, total = 0, took;
    HANDLE thread = CreateThread(NULL, 0, wake_answerer, &pair, 0, NULL);
    unsigned int i;

    QueryPerformanceFrequency(&frequency);
    for (i = 0; i < PROBE_WAKE_ROUNDS; i++)
    {
        Sleep(PROBE_WAKE_PAUSE_MS);
        QueryPerformanceCounter(&start);
        SetEvent(pair.ask);
        WaitForSingleObject(pair.answer, INFINITE);
        QueryPerformanceCounter(&end);
        took = (end.QuadPart - start.QuadPart) * 1000.0 / frequency.QuadPart;
        total += took;
        if (took > worst) worst = took;
    }
    probe_log("wake event: a thread waiting for an event answered in %.2f ms on average, %.2f ms at worst",
              total / PROBE_WAKE_ROUNDS, worst);
    TerminateThread(thread, 0);
    WaitForSingleObject(thread, 5000);
    CloseHandle(thread);
    CloseHandle(pair.ask);
    CloseHandle(pair.answer);
}

static BOOL wait_imports(const char *mode)
{
    HMODULE kernelbase = GetModuleHandleA("kernelbase.dll");
    HMODULE ntdll = GetModuleHandleA("ntdll.dll");

    wait_on_address = (wait_on_address_fn)(void *)GetProcAddress(kernelbase, "WaitOnAddress");
    wake_by_address_all = (wake_by_address_fn)(void *)GetProcAddress(kernelbase, "WakeByAddressAll");
    keyed_create = (keyed_create_fn)(void *)GetProcAddress(ntdll, "NtCreateKeyedEvent");
    keyed_wait = (keyed_call_fn)(void *)GetProcAddress(ntdll, "NtWaitForKeyedEvent");
    keyed_release = (keyed_call_fn)(void *)GetProcAddress(ntdll, "NtReleaseKeyedEvent");
    query_thread = (query_thread_fn)(void *)GetProcAddress(ntdll, "NtQueryInformationThread");
    if (!wait_on_address || !wake_by_address_all || !keyed_create || !keyed_wait || !keyed_release ||
        keyed_create(&keyed_event, EVENT_ALL_ACCESS, NULL, 0))
    {
        probe_log("%s: FAILED, WaitOnAddress or keyed events are missing", mode);
        return FALSE;
    }
    return TRUE;
}

struct queue_waiter
{
    volatile LONG *address;
    volatile LONG waiting, woken;
};

static __attribute__((aligned(PROBE_QUEUE_SPAN))) volatile LONG queue_area[PROBE_QUEUE_SPAN / sizeof(LONG)];

static DWORD WINAPI queue_wait(void *param)
{
    struct queue_waiter *q = param;
    LONG seen = 0;

    while (seen < PROBE_QUEUE_ROUNDS)
    {
        InterlockedExchange(&q->waiting, seen + 1);
        while (*q->address == seen)
            wait_on_address(q->address, &seen, sizeof(seen), INFINITE);
        seen = *q->address;
        InterlockedExchange(&q->woken, seen);
    }
    return 0;
}

static BOOL queue_rounds(struct queue_waiter *q, LONG *done)
{
    DWORD start;
    LONG round;

    for (round = 1; round <= PROBE_QUEUE_ROUNDS; round++)
    {
        for (start = GetTickCount(); q->waiting != round && GetTickCount() - start < PROBE_QUEUE_WAKE_MS;)
            Sleep(0);
        InterlockedExchange(q->address, round);
        wake_by_address_all((void *)q->address);
        for (start = GetTickCount(); q->woken != round && GetTickCount() - start < PROBE_QUEUE_WAKE_MS;)
            Sleep(0);
        *done = round;
        if (q->woken != round)
            return FALSE;
    }
    return TRUE;
}

static DWORD WINAPI queue_nothing(void *param)
{
    (void)param;
    return 0;
}

static void queue_end_a_thread(void)
{
    HANDLE thread = CreateThread(NULL, PROBE_STACK, queue_nothing, NULL, 0, NULL);

    if (!thread) return;
    WaitForSingleObject(thread, 5000);
    CloseHandle(thread);
}

static BOOL terminate_queue_check(enum wait_kind kind)
{
    struct waiter *w = waiter_start(kind);
    const void *target = kind == WAIT_SRW ? (const void *)&w->srw : (const void *)&w->condition;
    ULONG_PTR slot = ((ULONG_PTR)target >> PROBE_QUEUE_SHIFT) % PROBE_QUEUE_COUNT;
    struct queue_waiter q = { (volatile LONG *)((char *)queue_area + (slot << PROBE_QUEUE_SHIFT)), 0, 0 };
    HANDLE thread;
    LONG done = 0;
    BOOL ok;

    if (!w->thread || !w->entered)
    {
        probe_log("terminate %s, then its wait queue: FAILED, the thread did not start", wait_names[kind]);
        return FALSE;
    }
    TerminateThread(w->thread, 9);
    WaitForSingleObject(w->thread, 5000);
    queue_end_a_thread();
    *q.address = 0;
    probe_log("terminate %s, then its wait queue: the thread ended and then another one, waking %u times a thread "
              "that waits on an address in the same queue", wait_names[kind], PROBE_QUEUE_ROUNDS);
    thread = CreateThread(NULL, PROBE_STACK, queue_wait, &q, 0, NULL);
    ok = thread && queue_rounds(&q, &done);
    probe_log("terminate %s, then its wait queue: %s, %ld of %u wakes reached the other thread", wait_names[kind],
              ok ? "ok" : "FAILED", (long)(ok ? done : done - 1), PROBE_QUEUE_ROUNDS);
    if (thread)
    {
        if (!ok) TerminateThread(thread, 0);
        WaitForSingleObject(thread, 5000);
        CloseHandle(thread);
    }
    return ok;
}

static void terminate_checks(void)
{
    unsigned int passed = 0;

    if (!wait_imports("terminate"))
        return;
    passed += terminate_check(WAIT_ADDRESS);
    passed += terminate_check(WAIT_CONDITION);
    passed += terminate_check(WAIT_EVENT);
    passed += terminate_check(WAIT_SLEEP);
    terminate_polled(WAIT_EVENT);
    terminate_polled(WAIT_ADDRESS);
    wake_latency();
    passed += terminate_queue_check(WAIT_CONDITION);
    passed += terminate_queue_check(WAIT_SRW);
    probe_log("terminate: %u of 6 ok; quiet for %u s", passed, PROBE_QUIET_MS / 1000);
    Sleep(PROBE_QUIET_MS);
}

static void suspend_checks(void)
{
    unsigned int kind, passed = 0, total = 0;

    if (!wait_imports("suspend"))
        return;
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
    terminate_polled(WAIT_EVENT);
    terminate_polled(WAIT_ADDRESS);
    wake_latency();
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

#define PROBE_MEMORY_SIZE 256
#define PROBE_MEMORY_SPAN 24
#define PROBE_MEMORY_PASSES 2
#define PROBE_MEMORY_REPORTS 8
#define PROBE_SEARCH_SIZE (1024 * 1024)
#define PROBE_NEEDLE_SIZE 12
#define PROBE_NEEDLES 3

static __attribute__((aligned(64))) unsigned char memory_bytes[PROBE_MEMORY_SIZE];
static __attribute__((aligned(64))) unsigned char memory_copy[PROBE_MEMORY_SIZE];
static unsigned int memory_failed;
static unsigned int memory_checked;

static unsigned char pattern_byte(unsigned int i)
{
    return (unsigned char)(i * 37 + 11);
}

static void memory_fill(unsigned char *bytes, unsigned int size)
{
    unsigned int i;

    for (i = 0; i < size; i++)
        bytes[i] = pattern_byte(i);
}

static unsigned long long pattern_value(unsigned int offset, unsigned int size)
{
    unsigned long long value = 0;
    unsigned int i;

    for (i = 0; i < size; i++)
        value |= (unsigned long long)pattern_byte(offset + i) << (8 * i);
    return value;
}

static void memory_result(const char *what, unsigned int offset, unsigned int pass, unsigned long long got,
                          unsigned long long expected)
{
    memory_checked++;
    if (got == expected) return;
    if (memory_failed++ < PROBE_MEMORY_REPORTS)
        probe_log("memory: FAILED, %s at offset %u, pass %u: got %#llx, expected %#llx", what, offset, pass, got,
                  expected);
}

static void memory_loads(void)
{
    unsigned int pass, offset;

    memory_fill(memory_bytes, PROBE_MEMORY_SIZE);
    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        for (offset = 0; offset < PROBE_MEMORY_SPAN; offset++)
        {
            const unsigned char *at = memory_bytes + offset;

            memory_result("8-bit load", offset, pass, *(volatile const unsigned char *)at, pattern_value(offset, 1));
            memory_result("16-bit load", offset, pass, *(volatile const unsigned short *)at, pattern_value(offset, 2));
            memory_result("32-bit load", offset, pass, *(volatile const unsigned int *)at, pattern_value(offset, 4));
            memory_result("64-bit load", offset, pass, *(volatile const unsigned long long *)at,
                          pattern_value(offset, 8));
            memory_result("signed 8-bit load", offset, pass, (unsigned long long)(long long)*(volatile const signed char *)at,
                          (unsigned long long)(long long)(signed char)pattern_value(offset, 1));
            memory_result("signed 16-bit load", offset, pass, (unsigned long long)(long long)*(volatile const short *)at,
                          (unsigned long long)(long long)(short)pattern_value(offset, 2));
        }
}

static void memory_stores(void)
{
    static const unsigned long long values[] = { 0x8899aabbccddeeffull, 0x0123456789abcdefull };
    unsigned int pass, offset, size, i;

    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        for (offset = 0; offset < PROBE_MEMORY_SPAN; offset++)
            for (size = 1; size <= 8; size *= 2)
            {
                unsigned long long value = values[pass], got = 0;
                unsigned char *at = memory_bytes + offset;

                memory_fill(memory_bytes, PROBE_MEMORY_SIZE);
                if (size == 1) *(volatile unsigned char *)at = (unsigned char)value;
                else if (size == 2) *(volatile unsigned short *)at = (unsigned short)value;
                else if (size == 4) *(volatile unsigned int *)at = (unsigned int)value;
                else *(volatile unsigned long long *)at = value;
                for (i = 0; i < size; i++)
                    got |= (unsigned long long)((volatile unsigned char *)at)[i] << (8 * i);
                memory_result(size == 1 ? "8-bit store" : size == 2 ? "16-bit store" : size == 4 ? "32-bit store" :
                              "64-bit store", offset, pass, got,
                              size == 8 ? value : value & ((1ull << (8 * size)) - 1));
                memory_result("bytes around a store", offset, pass,
                              (unsigned long long)memory_bytes[offset + size] << 8 |
                                  (offset ? memory_bytes[offset - 1] : pattern_byte(0)),
                              (unsigned long long)pattern_byte(offset + size) << 8 |
                                  (offset ? pattern_byte(offset - 1) : pattern_byte(0)));
            }
}

static void memory_zero_stores(void)
{
    unsigned int pass, offset, size, i;

    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        for (offset = 0; offset < PROBE_MEMORY_SPAN; offset++)
            for (size = 2; size <= 8; size *= 2)
            {
                unsigned long long got = 0;
                unsigned char *at = memory_bytes + offset;

                memory_fill(memory_bytes, PROBE_MEMORY_SIZE);
                if (size == 2) *(volatile unsigned short *)at = 0;
                else if (size == 4) *(volatile unsigned int *)at = 0;
                else *(volatile unsigned long long *)at = 0;
                for (i = 0; i < size; i++)
                    got |= (unsigned long long)((volatile unsigned char *)at)[i] << (8 * i);
                memory_result(size == 2 ? "16-bit store of zero" : size == 4 ? "32-bit store of zero" :
                              "64-bit store of zero", offset, pass, got, 0);
            }
}

static unsigned int repe_cmpsb(const void *a, const void *b, size_t count, size_t *left)
{
    unsigned char equal;

    __asm__ volatile("cld\n\trepe cmpsb\n\tsete %0" : "=q"(equal), "+S"(a), "+D"(b), "+c"(count) : : "cc", "memory");
    *left = count;
    return equal;
}

static unsigned int repe_cmpsd(const void *a, const void *b, size_t count, size_t *left)
{
    unsigned char equal;

    __asm__ volatile("cld\n\trepe cmpsl\n\tsete %0" : "=q"(equal), "+S"(a), "+D"(b), "+c"(count) : : "cc", "memory");
    *left = count;
    return equal;
}

static const unsigned char *repne_scasb(const unsigned char *at, size_t count, unsigned char value)
{
    __asm__ volatile("cld\n\trepne scasb" : "+D"(at), "+c"(count) : "a"(value) : "cc", "memory");
    return at - 1;
}

static unsigned int lodsd_at(const void *at)
{
    unsigned int value;

    __asm__ volatile("cld\n\tlodsl" : "=a"(value), "+S"(at) : : "memory");
    return value;
}

static void rep_movsb(void *to, const void *from, size_t count)
{
    __asm__ volatile("cld\n\trep movsb" : "+D"(to), "+S"(from), "+c"(count) : : "memory");
}

static void rep_movsd(void *to, const void *from, size_t count)
{
    __asm__ volatile("cld\n\trep movsl" : "+D"(to), "+S"(from), "+c"(count) : : "memory");
}

static void memory_strings(void)
{
    unsigned int pass, offset;
    size_t left;

    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        for (offset = 0; offset < PROBE_MEMORY_SPAN; offset++)
        {
            unsigned int length = PROBE_MEMORY_SIZE / 2 - PROBE_MEMORY_SPAN;

            memory_fill(memory_bytes, PROBE_MEMORY_SIZE);
            memory_fill(memory_copy, PROBE_MEMORY_SIZE);
            memory_result("repe cmpsb of equal bytes", offset, pass,
                          repe_cmpsb(memory_bytes + offset, memory_copy + offset, length, &left) | (left << 8), 1);
            memory_copy[offset + 5] ^= 0xff;
            memory_result("repe cmpsb stops at a difference", offset, pass,
                          repe_cmpsb(memory_bytes + offset, memory_copy + offset, length, &left) | (left << 8),
                          (unsigned long long)(length - 6) << 8);
            memory_fill(memory_copy, PROBE_MEMORY_SIZE);
            memory_result("repe cmpsd of equal words", offset, pass,
                          repe_cmpsd(memory_bytes + offset, memory_copy + offset, length / 4, &left) | (left << 8), 1);
            memory_result("repne scasb finds a byte", offset, pass,
                          (unsigned long long)(repne_scasb(memory_bytes + offset, length, pattern_byte(offset + 9)) -
                                               memory_bytes),
                          offset + 9);
            memory_result("lodsd", offset, pass, lodsd_at(memory_bytes + offset), pattern_value(offset, 4));
            memset(memory_copy, 0, PROBE_MEMORY_SIZE);
            rep_movsb(memory_copy + (offset ^ 3), memory_bytes + offset, length);
            memory_result("rep movsb", offset, pass,
                          memcmp(memory_copy + (offset ^ 3), memory_bytes + offset, length) == 0, 1);
            memset(memory_copy, 0, PROBE_MEMORY_SIZE);
            rep_movsd(memory_copy + (offset ^ 1), memory_bytes + offset, length / 4);
            memory_result("rep movsd", offset, pass,
                          memcmp(memory_copy + (offset ^ 1), memory_bytes + offset, length / 4 * 4) == 0, 1);
        }
}

__attribute__((target("sse2"))) static void memory_vectors(void)
{
    unsigned int pass, offset, i;
    __attribute__((aligned(16))) unsigned char lane[16];

    memory_fill(memory_bytes, PROBE_MEMORY_SIZE);
    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        for (offset = 0; offset < PROBE_MEMORY_SPAN; offset++)
        {
            __m128i value = _mm_loadu_si128((const __m128i *)(memory_bytes + offset));
            unsigned long long low = 0, high = 0;

            _mm_store_si128((__m128i *)lane, value);
            for (i = 0; i < 8; i++)
            {
                low |= (unsigned long long)lane[i] << (8 * i);
                high |= (unsigned long long)lane[8 + i] << (8 * i);
            }
            memory_result("unaligned 16-byte load, low half", offset, pass, low, pattern_value(offset, 8));
            memory_result("unaligned 16-byte load, high half", offset, pass, high, pattern_value(offset + 8, 8));
            memory_result("byte compare mask", offset, pass,
                          (unsigned int)_mm_movemask_epi8(_mm_cmpeq_epi8(value, _mm_set1_epi8((char)pattern_byte(offset + 3)))),
                          1u << 3);
        }
}

static void memory_search(void)
{
    static const unsigned int places[PROBE_NEEDLES] = { 4097, 300003, 1000001 };
    unsigned char *haystack = VirtualAlloc(NULL, PROBE_SEARCH_SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    unsigned char needle[PROBE_NEEDLE_SIZE];
    unsigned int i, found_bytes = 0, found_memcmp = 0, found_memchr = 0;
    const unsigned char *at, *end;

    if (!haystack)
    {
        memory_result("search buffer", 0, 0, 0, 1);
        return;
    }
    for (i = 0; i < PROBE_SEARCH_SIZE; i++)
        haystack[i] = pattern_byte(i * 7);
    for (i = 0; i < PROBE_NEEDLE_SIZE; i++)
        needle[i] = (unsigned char)(0xe8 + i);
    for (i = 0; i < PROBE_NEEDLES; i++)
        memcpy(haystack + places[i], needle, PROBE_NEEDLE_SIZE);
    end = haystack + PROBE_SEARCH_SIZE - PROBE_NEEDLE_SIZE;
    for (at = haystack; at <= end; at++)
    {
        unsigned int k;

        for (k = 0; k < PROBE_NEEDLE_SIZE && at[k] == needle[k]; k++)
            ;
        found_bytes += k == PROBE_NEEDLE_SIZE;
        found_memcmp += !memcmp(at, needle, PROBE_NEEDLE_SIZE);
    }
    for (at = haystack; at <= end && (at = memchr(at, needle[0], end - at + 1)); at++)
        found_memchr += !memcmp(at, needle, PROBE_NEEDLE_SIZE);
    memory_result("pattern search byte by byte", 0, 0, found_bytes, PROBE_NEEDLES);
    memory_result("pattern search with memcmp", 0, 0, found_memcmp, PROBE_NEEDLES);
    memory_result("pattern search with memchr", 0, 0, found_memchr, PROBE_NEEDLES);
    VirtualFree(haystack, 0, MEM_RELEASE);
}

#define PROBE_IMAGE_NEEDLE 16
#define PROBE_IMAGE_WILDCARD 5
#define PROBE_BAD_CHARACTERS 256

static unsigned int image_search(const unsigned char *start, const unsigned char *end, const unsigned char *needle,
                                 const unsigned char *mask, int length)
{
    int skip[PROBE_BAD_CHARACTERS];
    unsigned int found = 0;
    const unsigned char *at;
    int i, first = -1;

    for (i = 0; i < length; i++)
        if (mask[i] != 0xff) first = i;
    for (i = 0; i < PROBE_BAD_CHARACTERS; i++)
        skip[i] = first;
    for (i = 0; i < length; i++)
        if (skip[needle[i]] < i) skip[needle[i]] = i;
    for (at = start; at <= end - length;)
    {
        int k = length - 1, shift;

        while (k >= 0 && (at[k] & mask[k]) == needle[k])
            k--;
        if (k < 0)
        {
            found++;
            at++;
            continue;
        }
        shift = k - skip[at[k]];
        at += shift > 1 ? shift : 1;
    }
    return found;
}

static void memory_image(void)
{
    const unsigned char *base = (const unsigned char *)GetModuleHandleA(NULL);
    const IMAGE_NT_HEADERS *nt = (const IMAGE_NT_HEADERS *)(base + ((const IMAGE_DOS_HEADER *)base)->e_lfanew);
    const IMAGE_SECTION_HEADER *section = IMAGE_FIRST_SECTION(nt);
    const unsigned char *code = (const unsigned char *)memory_search, *start = NULL, *end = NULL;
    unsigned char needle[PROBE_IMAGE_NEEDLE], mask[PROBE_IMAGE_NEEDLE];
    unsigned int i, executable = 0, plain = 0;

    for (i = 0; i < nt->FileHeader.NumberOfSections; i++, section++)
    {
        if (!(section->Characteristics & IMAGE_SCN_MEM_EXECUTE)) continue;
        executable++;
        if (code >= base + section->VirtualAddress && code < base + section->VirtualAddress + section->Misc.VirtualSize)
        {
            start = base + section->VirtualAddress;
            end = start + section->Misc.VirtualSize;
        }
    }
    memory_result("executable sections found in the image's headers", 0, 0, executable > 0 && start, 1);
    if (!start) return;
    for (i = 0; i < PROBE_IMAGE_NEEDLE; i++)
    {
        needle[i] = code[i];
        mask[i] = 0xff;
    }
    needle[PROBE_IMAGE_WILDCARD] = 0;
    mask[PROBE_IMAGE_WILDCARD] = 0;
    for (i = 0; start + i + PROBE_IMAGE_NEEDLE <= end; i++)
        plain += !memcmp(start + i, code, PROBE_IMAGE_NEEDLE);
    memory_result("masked search of the image's own code finds what memcmp finds", 0, 0,
                  image_search(start, end, needle, mask, PROBE_IMAGE_NEEDLE), plain);
    memory_result("the image's own code is found", 0, 0, plain >= 1, 1);
}

static void memory_operands(void)
{
    __attribute__((aligned(16))) unsigned int table[PROBE_BAD_CHARACTERS];
    unsigned int pass, i, tested = 0, compared = 0, anded = 0, subtracted = 0;

    for (i = 0; i < PROBE_BAD_CHARACTERS; i++)
        table[i] = i * 3;
    memory_fill(memory_bytes, PROBE_MEMORY_SIZE);
    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        for (i = 0; i < PROBE_MEMORY_SPAN; i++)
        {
            const unsigned char *at = memory_bytes + i;
            unsigned char byte = pattern_byte(i), and_result = 0xff;
            unsigned short word = (unsigned short)pattern_value(i, 2);
            unsigned int value = 7, set, equal;
            UINT_PTR index = pattern_byte(i);

            __asm__ volatile("testl $0x20000000, (%1)\n\tsetne %b0" : "=q"(set) : "r"(at) : "cc", "memory");
            tested += (set & 1) == !!(pattern_value(i, 4) & 0x20000000);
            __asm__ volatile("cmpw (%1), %w2\n\tsete %b0" : "=q"(equal) : "r"(at), "r"(word) : "cc", "memory");
            compared += equal & 1;
            __asm__ volatile("andb (%1), %0" : "+q"(and_result) : "r"(at) : "cc", "memory");
            anded += and_result == byte;
            __asm__ volatile("cmpb %2, (%1)\n\tsete %b0" : "=q"(equal) : "r"(at), "q"(byte) : "cc", "memory");
            compared += equal & 1;
            __asm__ volatile("subl -0x400(%1,%2,4), %0" : "+r"(value) : "r"(table + 0x100), "r"(index) : "cc", "memory");
            subtracted += value == (unsigned int)(7 - index * 3);
        }
    memory_result("test of memory with an immediate", 0, 0, tested, PROBE_MEMORY_PASSES * PROBE_MEMORY_SPAN);
    memory_result("byte and word compares with memory", 0, 0, compared, 2 * PROBE_MEMORY_PASSES * PROBE_MEMORY_SPAN);
    memory_result("and with a memory byte", 0, 0, anded, PROBE_MEMORY_PASSES * PROBE_MEMORY_SPAN);
    memory_result("subtract with base, scaled index and negative displacement", 0, 0, subtracted,
                  PROBE_MEMORY_PASSES * PROBE_MEMORY_SPAN);
}

typedef unsigned long (__cdecl *strtoul_fn)(const char *, char **, int);
typedef int (__cdecl *isxdigit_fn)(int);
typedef long (__cdecl *strtol_fn)(const char *, char **, int);

#define PROBE_INI_NAME "probe-memory.ini"

static void memory_runtime(void)
{
    HMODULE ucrt = LoadLibraryA("ucrtbase.dll");
    strtoul_fn ucrt_strtoul = ucrt ? (strtoul_fn)(void *)GetProcAddress(ucrt, "strtoul") : NULL;
    isxdigit_fn ucrt_isxdigit = ucrt ? (isxdigit_fn)(void *)GetProcAddress(ucrt, "isxdigit") : NULL;
    strtol_fn ucrt_strtol = ucrt ? (strtol_fn)(void *)GetProcAddress(ucrt, "strtol") : NULL;
    char path[MAX_PATH], text[64], *slash;
    unsigned int pass, digits = 0, value = 0;
    FILE *ini;

    memory_result("ucrtbase loaded with strtoul, isxdigit and strtol", 0, 0,
                  ucrt_strtoul && ucrt_isxdigit && ucrt_strtol, 1);
    if (ucrt_strtoul && ucrt_isxdigit && ucrt_strtol)
        for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
        {
            const char *hex = "0123456789abcdefABCDEF";
            unsigned int i;

            memory_result("strtoul of a hex byte", 0, pass, ucrt_strtoul("8B", NULL, 16), 0x8b);
            memory_result("strtoul of a hex word", 0, pass, ucrt_strtoul("0xDEADBEEF", NULL, 16), 0xdeadbeef);
            for (i = digits = 0; hex[i]; i++)
                digits += ucrt_isxdigit((unsigned char)hex[i]) != 0;
            digits += ucrt_isxdigit('g') == 0 && ucrt_isxdigit('?') == 0;
            memory_result("isxdigit", 0, pass, digits, 23);
            value = (unsigned int)ucrt_strtol("-0x2A", NULL, 16);
            memory_result("strtol of a negative hex number", 0, pass, value, (unsigned int)-0x2a);
        }
    GetModuleFileNameA(NULL, path, sizeof(path));
    if ((slash = strrchr(path, '\\'))) strcpy(slash + 1, PROBE_INI_NAME);
    if ((ini = fopen(path, "w")))
    {
        fputs("[MAIN]\nResX = 1280\nResY = 720\nFixHUD = 1\nPattern = 8B 0D ? ? ? ? 85 C9\n", ini);
        fclose(ini);
    }
    for (pass = 0; pass < PROBE_MEMORY_PASSES; pass++)
    {
        memory_result("GetPrivateProfileInt", 0, pass, GetPrivateProfileIntA("MAIN", "ResX", 0, path), 1280);
        memory_result("GetPrivateProfileInt of a second key", 0, pass, GetPrivateProfileIntA("MAIN", "FixHUD", 0, path),
                      1);
        GetPrivateProfileStringA("MAIN", "Pattern", "", text, sizeof(text), path);
        memory_result("GetPrivateProfileString", 0, pass, !strcmp(text, "8B 0D ? ? ? ? 85 C9"), 1);
    }
}

static void memory_checks(void)
{
    memory_failed = 0;
    memory_checked = 0;
    memory_loads();
    memory_stores();
    memory_zero_stores();
    memory_strings();
    memory_vectors();
    memory_search();
    memory_image();
    memory_operands();
    memory_runtime();
    probe_log("memory: %u of %u ok", memory_checked - memory_failed, memory_checked);
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
    else if (command_line && strstr(command_line, "terminate"))
    {
        terminate_checks();
    }
    else if (command_line && strstr(command_line, "syscalls"))
    {
        syscall_costs();
    }
    else if (command_line && strstr(command_line, "memory"))
    {
        memory_checks();
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
