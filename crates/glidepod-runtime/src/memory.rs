//! Bounded memory policy. Host and guest pressure are separate signals.
use crate::{Store, guest::Guest};
use anyhow::{Result, ensure};
use glidepod_core::{MemoryMode, ProfileSettings};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub version: u32,
    pub active: bool,
    pub reporting: bool,
    pub target_mib: u32,
    pub actual_mib: u32,
    pub reclaimed_bytes: u64,
}
/// Guest RAM backing only, sampled without touching/faulting guest contents.
/// Paged-out bytes are logical size, not physical compressor storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResidency {
    pub version: u32,
    pub capacity_bytes: u64,
    pub resident_bytes: u64,
    pub paged_out_bytes: u64,
    pub unbacked_bytes: u64,
    pub scan_ms: u64,
}
impl MemoryResidency {
    fn parse(reply: &str) -> Result<Self> {
        let state: Self = serde_json::from_str(reply).map_err(|_| {
            anyhow::anyhow!("RAM diagnostics require an updated runtime and active balloon driver.")
        })?;
        ensure!(
            state.version == 1
                && state.capacity_bytes > 0
                && [
                    state.capacity_bytes,
                    state.resident_bytes,
                    state.paged_out_bytes,
                    state.unbacked_bytes
                ]
                .iter()
                .all(|n| n % 16384 == 0)
                && state
                    .resident_bytes
                    .checked_add(state.paged_out_bytes)
                    .and_then(|n| n.checked_add(state.unbacked_bytes))
                    == Some(state.capacity_bytes),
            "Invalid guest RAM backing sample"
        );
        Ok(state)
    }
}
pub fn boot_memory(settings: &ProfileSettings, host_mib: u64) -> u32 {
    match settings.memory_mode {
        MemoryMode::Manual => settings.memory_mib,
        MemoryMode::Automatic => ((host_mib / 4 / 1024) * 1024).clamp(2048, 8192) as u32,
    }
}
pub fn control(guest: &Guest, command: &str) -> Result<String> {
    guest.verify_epoch()?;
    let mut socket = UnixStream::connect(&guest.endpoints.control)?;
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    socket.set_write_timeout(Some(Duration::from_secs(3)))?;
    writeln!(socket, "{command}")?;
    let mut reply = String::new();
    BufReader::new(socket).take(4097).read_line(&mut reply)?;
    ensure!(
        reply.len() <= 4096 && reply.ends_with('\n'),
        "Invalid memory control reply"
    );
    Ok(reply)
}
impl Guest {
    /// Explicit diagnostic, never polled by the memory governor or tray.
    pub fn memory_residency(&self) -> Result<MemoryResidency> {
        MemoryResidency::parse(&control(self, "memory-residency")?)
    }
    pub(crate) fn compact_idle_memory(&self) -> Result<()> {
        // Migrate live pages into fewer extents so reporting can surrender whole
        // host objects. This neither drops caches nor stops Android services.
        self.shell_timeout(
            "su 0 nice -n 19 timeout 3 sh -c 'echo 1 > /proc/sys/vm/compact_memory'",
            Duration::from_secs(5),
        )?;
        Ok(())
    }
    pub fn memory_status(&self) -> Result<MemoryStatus> {
        let reply = control(self, "memory-status")?;
        let state: MemoryStatus = serde_json::from_str(&reply)
            .map_err(|_| anyhow::anyhow!("Memory reclamation requires an updated runtime."))?;
        ensure!(state.version == 1, "Unsupported memory control version");
        Ok(state)
    }
    pub fn prepare_memory(&self) -> Result<MemoryStatus> {
        // The pinned image includes the module, but its default modules.load omits it.
        self.shell("su 0 sh -c 'test -d /sys/module/virtio_balloon || insmod /system_dlkm/lib/modules/virtio_balloon.ko'")?;
        // 64 KiB extents reclaim fragmented idle RAM while bounding host mapping fragmentation.
        self.shell(
            "su 0 sh -c 'echo 4 > /sys/module/page_reporting/parameters/page_reporting_order'",
        )?;
        self.memory_status()
    }
    pub fn balloon_target(&self, mib: u32) -> Result<()> {
        ensure!(
            control(self, &format!("memory {mib}"))?.trim() == "ok",
            "Engine refused memory target"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub available_mib: u32,
    pub pressured: bool,
}
/// Once per settled idle period, with a five-minute cooldown across window churn.
#[derive(Default)]
pub(crate) struct IdleCompaction {
    epoch: u64,
    idle_since: Option<Instant>,
    attempted: bool,
    last_attempt: Option<Instant>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl IdleCompaction {
    fn due(&mut self, epoch: u64, windows: bool, sample: Sample, now: Instant) -> bool {
        if self.epoch != epoch || windows {
            self.epoch = epoch;
            self.idle_since = None;
            self.attempted = false;
        }
        if windows {
            return false;
        }
        let idle = self.idle_since.get_or_insert(now);
        if self.attempted
            || now.duration_since(*idle) < Duration::from_secs(60)
            || self
                .last_attempt
                .is_some_and(|last| now.duration_since(last) < Duration::from_secs(300))
            || sample.pressured
            || sample.available_mib < 1024
        {
            return false;
        }
        self.attempted = true;
        self.last_attempt = Some(now);
        true
    }
    pub(crate) fn tick(&mut self, store: &Store, epoch: u64, windows: bool, sample: Sample) {
        if self.worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if !self.due(epoch, windows, sample, Instant::now()) {
            return;
        }
        let store = store.clone();
        // Keep control requests, tray actions and notification polling responsive.
        self.worker = Some(std::thread::spawn(move || {
            let started = Instant::now();
            let result = (|| -> Result<()> {
                let _maintenance = store.maintenance_lock(false)?;
                ensure!(
                    !crate::background::window_active(&store)?,
                    "An app window opened"
                );
                let guest = store.guest()?;
                ensure!(guest.endpoints.epoch == epoch, "Android restarted");
                guest.compact_idle_memory()
            })();
            match result {
                Ok(()) => eprintln!(
                    "[memory] idle compaction completed in {} ms",
                    started.elapsed().as_millis()
                ),
                Err(error) => eprintln!("[memory] idle compaction skipped/failed: {error:#}"),
            }
        }));
    }
}
impl Sample {
    pub fn parse(text: &str) -> Result<Self> {
        let available = text.lines().find_map(|line| {
            line.strip_prefix("MemAvailable:")
                .and_then(|v| v.split_whitespace().next()?.parse::<u32>().ok())
        });
        let pressure: Vec<f64> = text
            .lines()
            .filter(|l| l.starts_with("some ") || l.starts_with("full "))
            .filter_map(|l| {
                l.split_whitespace()
                    .find_map(|v| v.strip_prefix("avg10=")?.parse::<f64>().ok())
            })
            .collect();
        ensure!(
            pressure.len() == 2 && pressure.iter().all(|v| v.is_finite() && *v >= 0.),
            "Guest pressure sample missing or invalid"
        );
        let pressured = pressure.iter().any(|v| *v > 0.5);
        Ok(Self {
            available_mib: available
                .ok_or_else(|| anyhow::anyhow!("Guest memory sample missing"))?
                / 1024,
            pressured,
        })
    }
}
/// Grow promptly; shrink only unused headroom, at most 128 MiB per slow tick.
pub fn next_target(current: u32, capacity: u32, sample: Sample, shrink: bool) -> u32 {
    if sample.pressured || sample.available_mib < 768 {
        return current.saturating_sub(512);
    }
    if !shrink || sample.available_mib <= 1280 {
        return current;
    }
    current
        .saturating_add(128.min(sample.available_mib - 1280))
        .min(capacity.saturating_sub(2048))
}
pub fn sample(store: &Store) -> Result<Sample> {
    Sample::parse(
        &store
            .guest()?
            .shell("cat /proc/meminfo /proc/pressure/memory")?,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn residency_rejects_inconsistent_or_wrapping_accounting() {
        let mut value = serde_json::json!({"version":1,"capacity_bytes":65536,"resident_bytes":32768,
            "paged_out_bytes":16384,"unbacked_bytes":16384,"scan_ms":1});
        assert!(MemoryResidency::parse(&value.to_string()).is_ok());
        value["unbacked_bytes"] = 32768.into();
        assert!(MemoryResidency::parse(&value.to_string()).is_err());
        value["resident_bytes"] = (u64::MAX - 16383).into();
        assert!(MemoryResidency::parse(&value.to_string()).is_err());
        assert!(MemoryResidency::parse("error unknown command").is_err());
    }
    #[test]
    fn compaction_waits_for_idle_and_headroom_and_never_loops_on_failure() {
        let mut policy = IdleCompaction::default();
        let t = Instant::now();
        let healthy = Sample {
            available_mib: 2048,
            pressured: false,
        };
        assert!(!policy.due(1, false, healthy, t));
        assert!(!policy.due(
            1,
            false,
            Sample {
                pressured: true,
                ..healthy
            },
            t + Duration::from_secs(60)
        ));
        assert!(!policy.due(
            1,
            false,
            Sample {
                available_mib: 900,
                ..healthy
            },
            t + Duration::from_secs(65)
        ));
        assert!(policy.due(1, false, healthy, t + Duration::from_secs(70)));
        assert!(!policy.due(1, false, healthy, t + Duration::from_secs(600)));
        assert!(!policy.due(1, true, healthy, t + Duration::from_secs(610)));
        assert!(!policy.due(1, false, healthy, t + Duration::from_secs(620)));
        assert!(policy.due(1, false, healthy, t + Duration::from_secs(680)));
        // Another quick window cycle cannot trigger another expensive operation.
        assert!(!policy.due(1, true, healthy, t + Duration::from_secs(690)));
        assert!(!policy.due(1, false, healthy, t + Duration::from_secs(700)));
        assert!(!policy.due(1, false, healthy, t + Duration::from_secs(760)));
        assert!(!policy.due(2, false, healthy, t + Duration::from_secs(1000)));
        assert!(policy.due(2, false, healthy, t + Duration::from_secs(1060)));
    }
    #[test]
    fn incomplete_pressure_is_not_permission_to_shrink() {
        assert!(Sample::parse("MemAvailable: 2097152 kB").is_err());
        assert!(
            Sample::parse("MemAvailable: 2097152 kB\nsome avg10=NaN\nfull avg10=0.00").is_err()
        );
        let s =
            Sample::parse("MemAvailable: 2097152 kB\nsome avg10=0.00\nfull avg10=0.00").unwrap();
        assert_eq!(s.available_mib, 2048);
        assert!(!s.pressured);
    }
    #[test]
    fn pressure_returns_headroom_before_android_kills_services() {
        assert_eq!(
            next_target(
                1024,
                4096,
                Sample {
                    available_mib: 700,
                    pressured: false
                },
                true
            ),
            512
        );
        assert_eq!(
            next_target(
                256,
                4096,
                Sample {
                    available_mib: 2000,
                    pressured: true
                },
                true
            ),
            0
        );
        assert_eq!(
            next_target(
                0,
                4096,
                Sample {
                    available_mib: 1300,
                    pressured: false
                },
                true
            ),
            20
        );
        assert_eq!(
            next_target(
                2048,
                4096,
                Sample {
                    available_mib: 2000,
                    pressured: false
                },
                true
            ),
            2048
        );
        assert_eq!(
            next_target(
                0,
                4096,
                Sample {
                    available_mib: 2000,
                    pressured: false
                },
                false
            ),
            0
        );
    }
    #[test]
    fn auto_ceiling_preserves_host_headroom() {
        let mut s = ProfileSettings::default();
        assert_eq!(boot_memory(&s, 8192), 2048);
        assert_eq!(boot_memory(&s, 16384), 4096);
        assert_eq!(boot_memory(&s, 65536), 8192);
        s.memory_mode = MemoryMode::Manual;
        assert_eq!(boot_memory(&s, 65536), 4096);
    }
}
