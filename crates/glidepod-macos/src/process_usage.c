#include <libproc.h>
#include <sys/resource.h>
#include <stdint.h>
#include <string.h>
#include <mach/mach_time.h>
#include <pthread.h>

static mach_timebase_info_data_t timebase;
static pthread_once_t timebase_once = PTHREAD_ONCE_INIT;
static void initialize_timebase(void) { mach_timebase_info(&timebase); }

struct glidepod_process_usage {
    uint64_t started;
    uint64_t cpu_ns;
    uint64_t footprint;
    uint64_t resident;
};

// Only sample the executable registered as the VM engine, never a reused PID
// belonging to another program. Both paths come from canonical filesystem paths.
int glidepod_process_sample(int pid, const char *expected, struct glidepod_process_usage *out) {
    char path[PROC_PIDPATHINFO_MAXSIZE] = {0};
    struct rusage_info_v2 info = {0};
    if (pid <= 0 || proc_pidpath(pid, path, sizeof(path)) <= 0 || strcmp(path, expected) != 0) return 0;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V2, (rusage_info_t *)&info) != 0 || info.ri_proc_exit_abstime != 0) return 0;
    // Verify the identity again in case the PID changed during the read.
    char after[PROC_PIDPATHINFO_MAXSIZE] = {0};
    if (proc_pidpath(pid, after, sizeof(after)) <= 0 || strcmp(path, after) != 0) return 0;
    pthread_once(&timebase_once, initialize_timebase);
    if (!timebase.denom) return 0;
    // XNU rusage CPU totals are Mach absolute ticks (125/3 ns on this arm64 host).
    // Convert before combining with Rust's nanosecond wall-clock duration.
    __uint128_t cpu_ns = ((__uint128_t)info.ri_user_time + info.ri_system_time) * timebase.numer / timebase.denom;
    *out = (struct glidepod_process_usage) {
        .started = info.ri_proc_start_abstime,
        .cpu_ns = cpu_ns > UINT64_MAX ? UINT64_MAX : (uint64_t)cpu_ns,
        .footprint = info.ri_phys_footprint,
        .resident = info.ri_resident_size,
    };
    return 1;
}
