//! Read-only host process measurements. CPU is per core: 100% = one busy core.
use anyhow::{Context, Result, ensure};
use std::{path::Path, time::Instant};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct Sample {
    started: u64,
    cpu_ns: u64,
    footprint: u64,
    resident: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Usage {
    pub cpu_percent: Option<f64>,
    pub footprint_bytes: u64,
    pub resident_bytes: u64,
}

#[derive(Default)]
pub struct Sampler {
    previous: Option<(u32, Sample, Instant)>,
}
impl Sampler {
    pub fn reset(&mut self) {
        self.previous = None;
    }

    pub fn sample(&mut self, pid: u32, executable: &Path) -> Result<Usage> {
        let sample = match read(pid, executable) {
            Ok(sample) => sample,
            Err(error) => {
                self.reset();
                return Err(error);
            }
        };
        Ok(self.update(pid, sample, Instant::now()))
    }

    fn update(&mut self, pid: u32, sample: Sample, now: Instant) -> Usage {
        let cpu_percent = self.previous.and_then(|(old_pid, old, time)| {
            if old_pid != pid || old.started != sample.started {
                return None;
            }
            let elapsed = now.checked_duration_since(time)?.as_secs_f64();
            if elapsed < 0.05 {
                return None;
            }
            let cpu_ns = sample.cpu_ns.checked_sub(old.cpu_ns)?;
            Some(cpu_ns as f64 / 1e9 / elapsed * 100.)
        });
        self.previous = Some((pid, sample, now));
        Usage {
            cpu_percent,
            footprint_bytes: sample.footprint,
            resident_bytes: sample.resident,
        }
    }
}

#[cfg(target_os = "macos")]
fn read(pid: u32, executable: &Path) -> Result<Sample> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn glidepod_process_sample(
            pid: i32,
            expected: *const std::ffi::c_char,
            out: *mut Sample,
        ) -> i32;
    }
    let pid = i32::try_from(pid).context("Invalid VM process ID")?;
    let path = CString::new(executable.canonicalize()?.as_os_str().as_bytes())?;
    let mut sample = Sample::default();
    ensure!(
        unsafe { glidepod_process_sample(pid, path.as_ptr(), &mut sample) } == 1,
        "VM process metrics are unavailable"
    );
    Ok(sample)
}
#[cfg(not(target_os = "macos"))]
fn read(_: u32, _: &Path) -> Result<Sample> {
    anyhow::bail!("Process metrics require macOS")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn cpu_uses_elapsed_time_and_resets_for_pid_reuse_and_counter_reset() {
        let mut sampler = Sampler::default();
        let t = Instant::now();
        let mut sample = Sample {
            started: 7,
            cpu_ns: 100,
            footprint: 4096,
            resident: 8192,
        };
        assert!(sampler.update(4, sample, t).cpu_percent.is_none());
        sample.cpu_ns += 3_000_000_000;
        let usage = sampler.update(4, sample, t + Duration::from_secs(2));
        assert_eq!(usage.cpu_percent, Some(150.));
        assert_eq!(usage.footprint_bytes, 4096);
        sample.started = 8;
        assert!(
            sampler
                .update(4, sample, t + Duration::from_secs(3))
                .cpu_percent
                .is_none()
        );
        sample.cpu_ns = 0;
        assert!(
            sampler
                .update(4, sample, t + Duration::from_secs(4))
                .cpu_percent
                .is_none()
        );
        sampler.reset();
        assert!(
            sampler
                .update(4, sample, t + Duration::from_secs(5))
                .cpu_percent
                .is_none()
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_sample_checks_executable_identity_and_memory() {
        let mut sampler = Sampler::default();
        let usage = sampler
            .sample(std::process::id(), &std::env::current_exe().unwrap())
            .unwrap();
        assert!(usage.footprint_bytes > 0 && usage.resident_bytes > 0);
        assert!(
            sampler
                .sample(std::process::id(), Path::new("/usr/bin/true"))
                .is_err()
        );
        assert!(sampler.previous.is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_cpu_time_agrees_with_posix_rusage_on_apple_silicon() {
        fn reference() -> f64 {
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
            (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64 * 1e9
                + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 * 1e3
        }
        let exe = std::env::current_exe().unwrap();
        let before = reference();
        let first = read(std::process::id(), &exe).unwrap();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(150) {
            std::hint::black_box(start.elapsed());
        }
        let second = read(std::process::id(), &exe).unwrap();
        let reference = reference() - before;
        let ratio = (second.cpu_ns - first.cpu_ns) as f64 / reference;
        assert!(
            (0.85..1.15).contains(&ratio),
            "Mach/Posix CPU time ratio: {ratio}"
        );
    }
}
