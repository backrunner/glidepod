// Original SDK-based probe. No upstream VMM implementation is linked.
#include <Hypervisor/Hypervisor.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

struct watchdog { pthread_mutex_t mutex; pthread_cond_t cond; bool done; hv_vcpu_t cpu; };
static void *cancel_after_deadline(void *opaque) {
    struct watchdog *w = opaque;
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += 2;
    pthread_mutex_lock(&w->mutex);
    int result = 0;
    while (!w->done && result == 0) result = pthread_cond_timedwait(&w->cond, &w->mutex, &deadline);
    if (!w->done) hv_vcpus_exit(&w->cpu, 1);
    pthread_mutex_unlock(&w->mutex);
    return NULL;
}
int32_t glidepod_hvf_probe(uint64_t *value, uint64_t *syndrome) {
    hv_return_t result = hv_vm_create(NULL);
    if (result != HV_SUCCESS) return result;
    void *memory = NULL;
    hv_vcpu_t cpu = 0;
    hv_vcpu_exit_t *exit = NULL;
    bool mapped = false, created = false;
    const size_t bytes = 65536;
    const hv_ipa_t address = 0x80000000;
    if (posix_memalign(&memory, 65536, bytes) != 0) { result = -1; goto cleanup; }
    memset(memory, 0, bytes);
    // mov x0, #42; hvc #0. An isolated EL1 program with no devices or host I/O.
    const uint32_t code[] = { 0xd2800540, 0xd4000002 };
    memcpy(memory, code, sizeof(code));
    result = hv_vm_map(memory, address, bytes, HV_MEMORY_READ | HV_MEMORY_WRITE | HV_MEMORY_EXEC);
    if (result != HV_SUCCESS) goto cleanup;
    mapped = true;
    result = hv_vcpu_create(&cpu, &exit, NULL);
    if (result != HV_SUCCESS) goto cleanup;
    created = true;
    result = hv_vcpu_set_reg(cpu, HV_REG_PC, address);
    if (result != HV_SUCCESS) goto cleanup;
    result = hv_vcpu_set_reg(cpu, HV_REG_CPSR, 0x3c5); // EL1h, interrupts masked.
    if (result != HV_SUCCESS) goto cleanup;
    struct watchdog w = { .mutex = PTHREAD_MUTEX_INITIALIZER, .cond = PTHREAD_COND_INITIALIZER, .done = false, .cpu = cpu };
    pthread_t thread;
    if (pthread_create(&thread, NULL, cancel_after_deadline, &w) != 0) { result = -2; goto cleanup; }
    result = hv_vcpu_run(cpu);
    pthread_mutex_lock(&w.mutex); w.done = true; pthread_cond_signal(&w.cond); pthread_mutex_unlock(&w.mutex);
    pthread_join(thread, NULL);
    pthread_cond_destroy(&w.cond); pthread_mutex_destroy(&w.mutex);
    if (result == HV_SUCCESS && exit->reason != HV_EXIT_REASON_EXCEPTION) result = -3;
    if (result == HV_SUCCESS) {
        *syndrome = exit->exception.syndrome;
        result = hv_vcpu_get_reg(cpu, HV_REG_X0, value);
    }
cleanup:
    if (created) hv_vcpu_destroy(cpu);
    if (mapped) hv_vm_unmap(address, bytes);
    free(memory);
    hv_vm_destroy();
    return result;
}
