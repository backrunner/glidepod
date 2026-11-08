// Isolate Hypervisor.framework memory accounting from Android and graphics.
#include <Hypervisor/Hypervisor.h>
#include <mach/mach.h>
#include <sys/mman.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern char probe_code[], probe_end[];
__asm__(".text\n.p2align 2\n_probe_code:\nstr x2, [x0], #8\ncmp x0, x1\nb.lo _probe_code\nhvc #0\n_probe_end:\n");

static void check(hv_return_t value) {
    if (value != HV_SUCCESS) { fprintf(stderr, "HV error: 0x%x\n", value); exit(1); }
}
static mach_port_t observed_task;
static unsigned char *observed_ram;
static size_t observed_length;
static void physical_sample(void) {
    vm_statistics64_data_t v = {0};
    mach_msg_type_number_t n = HOST_VM_INFO64_COUNT;
    if (host_statistics64(mach_host_self(),HOST_VM_INFO64,(host_info64_t)&v,&n) != KERN_SUCCESS) exit(8);
    fprintf(stderr,"host_pages free=%u active=%u inactive=%u wired=%u speculative=%u compressor=%u internal=%u external=%u\n",v.free_count,v.active_count,v.inactive_count,v.wire_count,v.speculative_count,v.compressor_page_count,v.internal_page_count,v.external_page_count);
}
static void sample(const char *stage) {
    task_vm_info_data_t t = {0};
    mach_msg_type_number_t n = TASK_VM_INFO_COUNT;
    kern_return_t result = task_info(observed_task, TASK_VM_INFO, (task_info_t)&t, &n);
    if (result != KERN_SUCCESS) { fprintf(stderr,"task_info: %s\n",mach_error_string(result)); exit(2); }
    printf("{\"stage\":\"%s\",\"footprint\":%llu,\"resident\":%llu,\"internal\":%llu,\"compressed\":%llu,\"device\":%llu,\"reusable\":%llu,\"regions\":%d}\n", stage,
        t.phys_footprint,t.resident_size,t.internal,t.compressed,t.device,t.reusable,t.region_count);
    fflush(stdout);
    physical_sample();
    if (observed_ram) {
        size_t pages = observed_length / 16384;
        char *vec = calloc(pages,1);
        if (mincore(observed_ram,observed_length,vec)) exit(11);
        size_t incore = 0, modified = 0, referenced = 0;
        for (size_t i=0;i<pages;i++) { incore += !!(vec[i]&MINCORE_INCORE); modified += !!(vec[i]&MINCORE_MODIFIED); referenced += !!(vec[i]&MINCORE_REFERENCED); }
        fprintf(stderr,"backing pages resident=%zu modified=%zu referenced=%zu\n",incore,modified,referenced);
        free(vec);
    }
}
int main(int argc, char **argv) {
    observed_task = mach_task_self();
    if (argc > 2 && strcmp(argv[1],"--pid") == 0) {
        kern_return_t result = task_for_pid(mach_task_self(),atoi(argv[2]),&observed_task);
        if (result != KERN_SUCCESS) { fprintf(stderr,"task_for_pid: %s\n",mach_error_string(result)); return 2; }
        sample("external"); return 0;
    }
    size_t total = 1024UL << 20, granule = argc > 1 ? strtoul(argv[1], NULL, 10) : 65536;
    if (granule < 16384 || total % granule) return 3;
    check(hv_vm_create(NULL));
    sample("created");
    unsigned char *ram = mmap(NULL,total,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON,-1,0);
    if (ram == MAP_FAILED) return 4;
    for (size_t off = 0; off < total; off += granule) {
        if (mmap(ram+off,granule,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0) == MAP_FAILED) return 5;
        check(hv_vm_map(ram+off,0x80000000+off,granule,HV_MEMORY_READ|HV_MEMORY_WRITE|HV_MEMORY_EXEC));
    }
    observed_ram = ram; observed_length = total;
    sample("untouched_mapped");
    hv_vcpu_t cpu = 0;
    hv_vcpu_exit_t *exit_info = NULL;
    if (argc > 2) {
        memcpy(ram,probe_code,(size_t)(probe_end-probe_code));
        check(hv_vcpu_create(&cpu,&exit_info,NULL));
        check(hv_vcpu_set_reg(cpu,HV_REG_PC,0x80000000));
        check(hv_vcpu_set_reg(cpu,HV_REG_CPSR,0x3c5));
        check(hv_vcpu_set_reg(cpu,HV_REG_X0,0x80010000));
        check(hv_vcpu_set_reg(cpu,HV_REG_X1,0x80000000+total));
        check(hv_vcpu_set_reg(cpu,HV_REG_X2,0x5a5a5a5a5a5a5a5a));
        check(hv_vcpu_run(cpu));
        fprintf(stderr,"Guest exit %d syndrome %llx\n",exit_info->reason,exit_info->exception.syndrome);
    } else {
        memset(ram,0x5a,total);
    }
    sample("written");
    fprintf(stderr,"Before unmap: %llx\n",*(volatile unsigned long long *)(ram+total-8));
    sample("host_read_tail");
    unsigned long long sum = 0;
    for (size_t off = 0; off < total; off += 16384) sum += *(volatile unsigned char *)(ram+off);
    fprintf(stderr,"Host read checksum: %llu\n",sum);
    sample("host_read_all");
    if (mprotect(ram,total,PROT_NONE)) return 9;
    sample("host_inaccessible");
    if (mprotect(ram,total,PROT_READ|PROT_WRITE)) return 10;
    sample("host_access_restored");
    fprintf(stderr,"Restored tail: %llx\n",*(volatile unsigned long long *)(ram+total-8));
    for (size_t off = 0; off < total / 2; off += granule) {
        check(hv_vm_unmap(0x80000000+off,granule));
        if (mmap(ram+off,granule,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0) == MAP_FAILED) return 6;
        check(hv_vm_map(ram+off,0x80000000+off,granule,HV_MEMORY_READ|HV_MEMORY_WRITE|HV_MEMORY_EXEC));
    }
    sample("replaced_half");
    check(hv_vm_unmap(0x80000000,total));
    sample("hv_unmapped");
    fprintf(stderr,"After unmap: %llx\n",*(volatile unsigned long long *)(ram+total-8));
    sample("host_read_unmapped_tail");
    if (mmap(ram,total,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON|MAP_FIXED,-1,0) == MAP_FAILED) return 7;
    sample("host_replaced_all");
    check(hv_vm_map(ram,0x80000000,total,HV_MEMORY_READ|HV_MEMORY_WRITE|HV_MEMORY_EXEC));
    sample("remapped_untouched");
    if (exit_info) check(hv_vcpu_destroy(cpu));
    check(hv_vm_unmap(0x80000000,total));
    observed_ram = NULL;
    munmap(ram,total);
    check(hv_vm_destroy());
    sample("destroyed");
}
