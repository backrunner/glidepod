//! Small native probes. Host Metal capability is not Android acceleration evidence.
pub mod gamepad;
pub mod launchers;
pub mod notifications;
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostReport {
    pub architecture: String,
    pub macos_version: String,
    pub memory_mib: u64,
    pub logical_cpus: u16,
    pub host_page_size: u64,
    pub hypervisor_supported: bool,
    pub metal_device: Option<String>,
}
fn output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program).args(args).output().ok()?;
    if !result.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&result.stdout).trim().to_string())
}
pub fn host_report() -> HostReport {
    let number = |name| {
        output("/usr/sbin/sysctl", &["-n", name])
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };
    HostReport {
        architecture: std::env::consts::ARCH.into(),
        macos_version: output("/usr/bin/sw_vers", &["-productVersion"])
            .unwrap_or_else(|| "Unavailable".into()),
        memory_mib: number("hw.memsize") / 1024 / 1024,
        logical_cpus: number("hw.logicalcpu") as u16,
        host_page_size: number("hw.pagesize"),
        hypervisor_supported: number("kern.hv_support") == 1,
        metal_device: metal_name(),
    }
}
#[cfg(target_os = "macos")]
fn metal_name() -> Option<String> {
    objc::rc::autoreleasepool(|| metal::Device::system_default().map(|d| d.name().to_string()))
}
#[cfg(not(target_os = "macos"))]
fn metal_name() -> Option<String> {
    None
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeReport {
    pub host: HostReport,
    pub probes: Vec<ProbeResult>,
    pub android_boot_verified: bool,
    pub android_gpu_verified: bool,
}
pub fn run_probes() -> ProbeReport {
    ProbeReport {
        host: host_report(),
        probes: vec![hvf_probe(), metal_probe()],
        android_boot_verified: false,
        android_gpu_verified: false,
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn hvf_probe() -> ProbeResult {
    unsafe extern "C" {
        fn glidepod_hvf_probe(value: *mut u64, syndrome: *mut u64) -> i32;
    }
    let (mut value, mut syndrome) = (0, 0);
    // SAFETY: two valid out pointers; the SDK shim owns and tears down its VM, vCPU and mapping.
    let result = unsafe { glidepod_hvf_probe(&mut value, &mut syndrome) };
    let passed = result == 0 && value == 42 && syndrome >> 26 == 0x16;
    ProbeResult {
        name: "hvf_arm64_execution".into(),
        passed,
        detail: if result != 0 {
            format!(
                "Hypervisor.framework returned 0x{:08x}. The probe executable requires the hypervisor entitlement.",
                result as u32
            )
        } else {
            format!("EL1 MOV/HVC: X0={value}, ESR=0x{syndrome:x}; expected X0=42 and HVC64 trap.")
        },
    }
}
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn hvf_probe() -> ProbeResult {
    ProbeResult {
        name: "hvf_arm64_execution".into(),
        passed: false,
        detail: "Requires Apple Silicon macOS.".into(),
    }
}
#[cfg(target_os = "macos")]
fn metal_probe() -> ProbeResult {
    let result = objc::rc::autoreleasepool(|| -> Result<String, String> {
        use metal::*;
        let device = Device::system_default().ok_or("No Metal device.")?;
        let source = "#include <metal_stdlib>\nusing namespace metal;\nkernel void verify(device uint* values [[buffer(0)]], uint i [[thread_position_in_grid]]) { values[i] = i * 1664525u + 1013904223u; }";
        let library = device.new_library_with_source(source, &CompileOptions::new())?;
        let function = library.get_function("verify", None)?;
        let pipeline = device.new_compute_pipeline_state_with_function(&function)?;
        let buffer = device.new_buffer(256 * 4, MTLResourceOptions::StorageModeShared);
        let queue = device.new_command_queue();
        let command = queue.new_command_buffer();
        let encoder = command.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&pipeline);
        encoder.set_buffer(0, Some(&buffer), 0);
        encoder.dispatch_threads(MTLSize::new(256, 1, 1), MTLSize::new(64, 1, 1));
        encoder.end_encoding();
        command.commit();
        command.wait_until_completed();
        if command.status() != MTLCommandBufferStatus::Completed {
            return Err(format!("Command buffer status: {:?}", command.status()));
        }
        // SAFETY: shared buffer holds 256 u32 values and the GPU finished before the CPU reads it.
        let values = unsafe { std::slice::from_raw_parts(buffer.contents().cast::<u32>(), 256) };
        if values
            .iter()
            .enumerate()
            .any(|(i, v)| *v != (i as u32).wrapping_mul(1664525).wrapping_add(1013904223))
        {
            return Err("GPU result did not match the CPU reference.".into());
        }
        Ok(format!(
            "{}: compiled and executed Metal compute; all 256 output words match the CPU reference. This is a host GPU probe, not an Android render test.",
            device.name()
        ))
    });
    ProbeResult {
        name: "metal_compute_readback".into(),
        passed: result.is_ok(),
        detail: result.unwrap_or_else(|e| e),
    }
}
#[cfg(not(target_os = "macos"))]
fn metal_probe() -> ProbeResult {
    ProbeResult {
        name: "metal_compute_readback".into(),
        passed: false,
        detail: "Requires macOS.".into(),
    }
}
