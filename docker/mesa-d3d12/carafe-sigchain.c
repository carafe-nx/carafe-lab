#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define TLS __thread __attribute__((tls_model("initial-exec")))
#define MAX_DEPTH 8
typedef int (*sigaction_fn)(int, const struct sigaction *, struct sigaction *);
static sigaction_fn real_sigaction;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static const char *owner_lib = "qcgpuarm64compilercore";
static struct sigaction app[2][2];
static atomic_int app_cur[2];
static atomic_ulong owner_calls, owner_faults;
static TLS struct sigaction own[2][MAX_DEPTH];
static TLS struct sigaction own_prev[2][MAX_DEPTH];
static TLS int own_depth[2];

static int slot(int sig) { return sig == SIGSEGV ? 0 : sig == SIGBUS ? 1 : -1; }
static void init(void) { if (!real_sigaction) real_sigaction = (sigaction_fn)dlsym(RTLD_NEXT, "sigaction"); }
static bool same_handler(const struct sigaction *a, const struct sigaction *b) {
    return a->sa_handler == b->sa_handler && (a->sa_flags & SA_SIGINFO) == (b->sa_flags & SA_SIGINFO);
}
static void call(const struct sigaction *sa, int sig, siginfo_t *info, void *uc) {
    if (sa->sa_flags & SA_SIGINFO) {
        sa->sa_sigaction(sig, info, uc);
    } else if (sa->sa_handler == SIG_DFL) {
        struct sigaction dfl;
        memset(&dfl, 0, sizeof(dfl));
        dfl.sa_handler = SIG_DFL;
        real_sigaction(sig, &dfl, NULL);
    } else if (sa->sa_handler != SIG_IGN) {
        sa->sa_handler(sig);
    }
}
static void dispatch(int sig, siginfo_t *info, void *uc) {
    const int k = slot(sig);
    if (own_depth[k] > 0) {
        atomic_fetch_add_explicit(&owner_faults, 1, memory_order_relaxed);
        call(&own[k][own_depth[k] - 1], sig, info, uc);
        return;
    }
    call(&app[k][atomic_load_explicit(&app_cur[k], memory_order_acquire)], sig, info, uc);
}
static int install(int sig, const struct sigaction *app_sa) {
    struct sigaction sa = *app_sa;
    sa.sa_sigaction = dispatch;
    sa.sa_flags = (app_sa->sa_flags | SA_SIGINFO | SA_ONSTACK) & ~SA_RESETHAND;
    return real_sigaction(sig, &sa, NULL);
}
static bool from_owner(const void *caller) {
    Dl_info info;
    return dladdr(caller, &info) && info.dli_fname && strstr(info.dli_fname, owner_lib);
}
__attribute__((noinline)) static int handle(int sig, const struct sigaction *act,
                                            struct sigaction *oldact, const void *caller) {
    init();
    const int k = slot(sig);
    if (k < 0) return real_sigaction(sig, act, oldact);
    pthread_mutex_lock(&lock);
    int cur = atomic_load_explicit(&app_cur[k], memory_order_relaxed);
    struct sigaction kernel;
    real_sigaction(sig, NULL, &kernel);
    const bool installed = (kernel.sa_flags & SA_SIGINFO) && kernel.sa_sigaction == dispatch;
    if (!installed) {
        cur ^= 1;
        app[k][cur] = kernel;
        atomic_store_explicit(&app_cur[k], cur, memory_order_release);
    }
    int ret = 0;
    if (from_owner(caller)) {
        const int d = own_depth[k];
        const struct sigaction *visible = d > 0 ? &own[k][d - 1] : &app[k][cur];
        if (oldact) *oldact = *visible;
        if (act) {
            if (d > 0 && same_handler(act, &own_prev[k][d - 1])) {
                own_depth[k] = d - 1;
            } else if (d < MAX_DEPTH) {
                own_prev[k][d] = *visible;
                own[k][d] = *act;
                own_depth[k] = d + 1;
                if (!installed) ret = install(sig, &app[k][cur]);
            }
            if (atomic_fetch_add_explicit(&owner_calls, 1, memory_order_relaxed) == 0) {
                static const char msg[] = "carafe-sigchain: compiler signal handler kept to its thread\n";
                (void)!write(2, msg, sizeof(msg) - 1);
            }
        }
    } else {
        if (oldact) *oldact = app[k][cur];
        if (act) {
            const int next = cur ^ 1;
            app[k][next] = *act;
            atomic_store_explicit(&app_cur[k], next, memory_order_release);
            ret = install(sig, act);
        }
    }
    pthread_mutex_unlock(&lock);
    return ret;
}
int sigaction(int sig, const struct sigaction *act, struct sigaction *oldact) {
    return handle(sig, act, oldact, __builtin_return_address(0));
}
__attribute__((constructor)) static void setup(void) {
    init();
    const char *lib = getenv("CARAFE_SIGCHAIN_LIB");
    if (lib && *lib) owner_lib = lib;
}
unsigned long carafe_sigchain_owner_calls(void) { return atomic_load(&owner_calls); }
unsigned long carafe_sigchain_owner_faults(void) { return atomic_load(&owner_faults); }
