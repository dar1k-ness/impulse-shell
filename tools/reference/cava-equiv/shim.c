// LD_PRELOAD shim that makes a real `cava` (fifo input) step one frame at a time for
// crates/qml/tests/cava_equiv.rs. Fds from the environment:
//   CAVA_SHIM_FRAME (w): one byte each time the main thread finishes a frame (its sleep);
//   CAVA_SHIM_GO    (r): the main thread's sleep returns after one byte;
//   CAVA_SHIM_IDLE  (w): one byte each time the input thread starts its 10 ms "no data" sleep;
//   CAVA_SHIM_DATA  (r): that sleep returns after one byte;
//   CAVA_SHIM_TOOK  (w): one byte each time the input thread stored a block (mutex unlock).
// The test writes a block only while the input thread sleeps (after its IDLE byte), so each
// DATA byte ends exactly one sleep: a block the thread found without sleeping would leave its
// DATA byte behind, and enough of those count as the 10 empty reads that reset cava's input.
// The main thread's CLOCK_MONOTONIC only advances by the sleeps it asked for, so cava's
// frame pacing (`wakeupTimens`) never sees the time spent blocked here.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdlib.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static int fd(const char *name) {
    const char *v = getenv(name);
    return v ? atoi(v) : -1;
}

static int is_main(void) { return syscall(SYS_gettid) == getpid(); }

static long long vclock;
static int started;

int nanosleep(const struct timespec *req, struct timespec *rem) {
    char c = 0;
    if (is_main()) {
        // The first sleeps wait for the input thread to report its format.
        if (!started) {
            started = 1;
            usleep(1000);
            return 0;
        }
        vclock += req->tv_sec * 1000000000LL + req->tv_nsec;
        if (write(fd("CAVA_SHIM_FRAME"), &c, 1) != 1 || read(fd("CAVA_SHIM_GO"), &c, 1) != 1)
            _exit(0);
        return 0;
    }
    if (write(fd("CAVA_SHIM_IDLE"), &c, 1) != 1 || read(fd("CAVA_SHIM_DATA"), &c, 1) != 1)
        _exit(0);
    return 0;
}

int clock_gettime(clockid_t id, struct timespec *ts) {
    static int (*real)(clockid_t, struct timespec *);
    if (!real)
        real = dlsym(RTLD_NEXT, "clock_gettime");
    if (id != CLOCK_MONOTONIC || !is_main())
        return real(id, ts);
    ts->tv_sec = 1000 + vclock / 1000000000LL;
    ts->tv_nsec = vclock % 1000000000LL;
    return 0;
}

int pthread_mutex_unlock(pthread_mutex_t *m) {
    static int (*real)(pthread_mutex_t *);
    if (!real)
        real = dlsym(RTLD_NEXT, "pthread_mutex_unlock");
    int r = real(m);
    if (!is_main()) {
        char c = 0;
        if (write(fd("CAVA_SHIM_TOOK"), &c, 1) != 1)
            _exit(0);
    }
    return r;
}
