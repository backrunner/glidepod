//! Original guest GLES and direct Vulkan workloads, with separate correctness evidence.
use crate::{
    adb::quote,
    guest::{Guest, resource},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsApi {
    #[default]
    Gles,
    Vulkan,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameIntervalScope {
    /// Reports produced before the measurement scope was recorded.
    #[default]
    Unspecified,
    GuestRenderLoop,
    VulkanImageAcquisition,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRequest {
    #[serde(default)]
    pub api: GraphicsApi,
    #[serde(default)]
    pub validation: bool,
    /// Deliberately corrupt one readback assertion to verify failure propagation.
    #[serde(default)]
    pub negative_control: bool,
    #[serde(default)]
    pub no_recreate: bool,
    #[serde(default)]
    pub trace_stages: bool,
    #[serde(default = "default_gles_version")]
    pub gles_version: u32,
    pub width: u32,
    pub height: u32,
    pub objects: u32,
    pub samples: u32,
    pub capture_frames: bool,
    pub finish_each_frame: bool,
}
fn default_gles_version() -> u32 {
    2
}
impl Default for BenchmarkRequest {
    fn default() -> Self {
        Self {
            api: GraphicsApi::Gles,
            validation: false,
            negative_control: false,
            no_recreate: false,
            trace_stages: false,
            gles_version: 2,
            width: 720,
            height: 1080,
            objects: 16,
            samples: 180,
            capture_frames: true,
            finish_each_frame: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timing {
    pub samples: usize,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}
fn timing(values: &[f64]) -> Result<Timing> {
    ensure!(
        !values.is_empty() && values.iter().all(|v| v.is_finite() && *v >= 0.),
        "Invalid timing samples"
    );
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |p: f64| {
        sorted[((p * sorted.len() as f64).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1)]
    };
    Ok(Timing {
        samples: values.len(),
        mean_ms: values.iter().sum::<f64>() / values.len() as f64,
        p50_ms: at(0.5),
        p95_ms: at(0.95),
        p99_ms: at(0.99),
        max_ms: *sorted.last().unwrap(),
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub epoch: u64,
    #[serde(default)]
    pub display_id: u32,
    pub renderer: glidepod_core::Renderer,
    pub request: BenchmarkRequest,
    pub recorded_unix_ms: u64,
    pub draw: Timing,
    /// Producer timing, never a compositor or physical display timestamp.
    pub frame_interval: Timing,
    #[serde(default)]
    pub frame_interval_scope: FrameIntervalScope,
    pub mean_fps: f64,
    pub frames_over_16_67_ms: usize,
    pub frames_over_33_33_ms: usize,
    pub host_frames: u64,
    pub host_rgba_bytes: u64,
    pub host_read: Option<Timing>,
    pub host_bgra_conversion: Option<Timing>,
    pub build_profile: String,
    pub duration_seconds: f64,
    pub guest: serde_json::Value,
    pub native_presentation_measured: bool,
    /// Guest-reported Vulkan device timestamps, not calibrated host Metal GPU time.
    #[serde(default)]
    pub device_timestamp: Option<Timing>,
}
struct Stop(Arc<AtomicBool>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
struct StopWorkload(Guest, &'static str);
impl Drop for StopWorkload {
    fn drop(&mut self) {
        let _ = self.0.stop_app(self.1);
    }
}
impl Guest {
    pub fn benchmark(&self, r: BenchmarkRequest) -> Result<BenchmarkReport> {
        let vulkan = r.api == GraphicsApi::Vulkan;
        ensure!(
            (2..=3).contains(&r.gles_version)
                && (320..=2048).contains(&r.width)
                && (320..=2048).contains(&r.height)
                && (1..=64).contains(&r.objects)
                && (60..=if vulkan { 3600 } else { 600 }).contains(&r.samples),
            "Benchmark parameters outside supported bounds"
        );
        ensure!(
            if vulkan {
                !r.finish_each_frame && r.gles_version == 2
            } else {
                !r.validation && !r.negative_control && !r.no_recreate && !r.trace_stages
            },
            "Vulkan does not accept --gles or --finish-each-frame; validation, negative control, no-recreate and trace-stages require Vulkan"
        );
        let renderer = self
            .store
            .runtime_status()?
            .effective_renderer
            .ok_or_else(|| anyhow::anyhow!("Android renderer unavailable"))?;
        let (package, apk, authority) = if vulkan {
            (
                "dev.glidepod.vulkan",
                "glidepod-vulkan.apk",
                "dev.glidepod.vulkan.results",
            )
        } else {
            (
                "dev.glidepod.benchmark",
                "glidepod-benchmark.apk",
                "dev.glidepod.benchmark.results",
            )
        };
        self.install(&resource(apk)?, None)?;
        self.stop_app(package)?;
        let window = self.open_app(package, r.width, r.height)?;
        let _workload = StopWorkload(self.clone(), package);
        let run = format!(
            "{:016x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        self.shell(&format!("am start -W --display {} -f 0x10008000 -n {package}/.MainActivity --es run {} --ei objects {} --ei samples {} --ei gles {} --ez finish {} --ez validation {} --ez negative {} --ez recreate {} --ez stages {}",window.info.display_id,quote(&run),r.objects,r.samples,r.gles_version,r.finish_each_frame,r.validation,r.negative_control,!r.no_recreate,r.trace_stages))?;
        let stopped = Arc::new(AtomicBool::new(false));
        let _stop = Stop(stopped.clone());
        let guest = self.clone();
        let token = run.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let result = guest
                    .provider(authority, "result", None)
                    .and_then(|s| Ok(serde_json::from_str::<serde_json::Value>(s.trim())?));
                match result {
                    Ok(v) if v["ready"] == true && v["run"] == token => {
                        let _ = tx.send(Ok(v));
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        let start = Instant::now();
        let mut seq = 0;
        let mut count = 0;
        let mut bytes = 0;
        let mut reads = Vec::new();
        let mut conversions = Vec::new();
        let mut last = None;
        let raw = loop {
            if let Ok(result) = rx.try_recv() {
                break result?;
            }
            ensure!(
                start.elapsed() < Duration::from_secs(180),
                "3D check timed out; Android may have stopped rendering"
            );
            if r.capture_frames {
                let before = Instant::now();
                if let Some(mut frame) = window.frame(seq)? {
                    reads.push(before.elapsed().as_secs_f64() * 1000.);
                    let before = Instant::now();
                    crate::display::swap_red_blue(&mut frame.rgba);
                    conversions.push(before.elapsed().as_secs_f64() * 1000.);
                    seq = frame.sequence;
                    count += 1;
                    bytes += frame.rgba.len() as u64;
                    last = Some(frame);
                }
            } else {
                self.verify_epoch()?;
            }
            thread::sleep(Duration::from_millis(16));
        };
        let out = self.store.root().join("developer/benchmark");
        fs::create_dir_all(&out)?;
        // Preserve failed diagnostics too; errors must not be mistaken for the last successful run.
        fs::write(
            out.join("latest-guest.json"),
            serde_json::to_vec_pretty(&raw)?,
        )?;
        ensure!(
            raw.get("error").is_none(),
            "3D workload failed: {}",
            raw["error"]
        );
        ensure!(
            (if vulkan {
                raw["vk_success"] == true
                    && raw["validation_enabled"] == r.validation
                    && raw["api"] == "vulkan"
                    && raw["recreate_requested"] == !r.no_recreate
                    && raw["stage_timings_enabled"] == r.trace_stages
                    && raw["frame_interval_includes_recreation"] == true
            } else {
                raw["gl_error"] == 0
            }) && raw["colored_pixels"].as_u64().unwrap_or(0) > 100,
            "3D rendering produced an invalid/empty framebuffer"
        );
        ensure!(
            raw["width"] == r.width
                && raw["height"] == r.height
                && raw["objects"] == r.objects
                && (vulkan
                    || (raw["requested_gles"] == r.gles_version
                        && raw["finish_each_frame"] == r.finish_each_frame)),
            "Benchmark surface does not match the requested workload"
        );
        if vulkan && r.capture_frames {
            ensure!(
                last.as_ref().is_some_and(|f| f
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[0].max(p[1]).max(p[2]) > 40
                        && p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]) > 20)
                    .count()
                    > 100),
                "Vulkan presented no colored scene through the independent display transport"
            );
        }
        let draw: Vec<f64> = serde_json::from_value(raw["draw_ms"].clone())?;
        let interval: Vec<f64> = serde_json::from_value(raw["frame_interval_ms"].clone())?;
        ensure!(
            draw.len() == r.samples as usize && interval.len() == draw.len(),
            "Incomplete benchmark samples"
        );
        let frame_interval = timing(&interval)?;
        let device_timestamp = if vulkan {
            let values: Vec<f64> = serde_json::from_value(raw["device_timestamp_ms"].clone())?;
            ensure!(
                values.is_empty() || values.len() == draw.len(),
                "Incomplete device timestamps"
            );
            if values.is_empty() {
                None
            } else {
                Some(timing(&values)?)
            }
        } else {
            None
        };
        let report = BenchmarkReport {
            epoch: self.endpoints.epoch,
            display_id: window.info.display_id,
            renderer,
            request: r,
            recorded_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64,
            draw: timing(&draw)?,
            mean_fps: 1000. / frame_interval.mean_ms,
            frame_interval,
            frame_interval_scope: if vulkan {
                FrameIntervalScope::VulkanImageAcquisition
            } else {
                FrameIntervalScope::GuestRenderLoop
            },
            frames_over_16_67_ms: interval.iter().filter(|v| **v > 1000. / 60.).count(),
            frames_over_33_33_ms: interval.iter().filter(|v| **v > 1000. / 30.).count(),
            host_frames: count,
            host_rgba_bytes: bytes,
            host_read: if reads.is_empty() {
                None
            } else {
                Some(timing(&reads)?)
            },
            host_bgra_conversion: if conversions.is_empty() {
                None
            } else {
                Some(timing(&conversions)?)
            },
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .into(),
            duration_seconds: start.elapsed().as_secs_f64(),
            guest: raw,
            native_presentation_measured: false,
            device_timestamp,
        };
        if let Some(f) = last {
            use std::io::Write;
            let mut file = std::io::BufWriter::new(fs::File::create(out.join("latest.ppm"))?);
            write!(file, "P6\n{} {}\n255\n", f.width, f.height)?;
            for p in f.rgba.as_chunks::<4>().0 {
                file.write_all(&[p[2], p[1], p[0]])?;
            }
            file.flush()?;
        }
        fs::write(out.join("latest.json"), serde_json::to_vec_pretty(&report)?)?;
        ensure!(
            vulkan
                || report.request.gles_version != 3
                || report.guest["gles3_probe"]["correctness"]["passed"] == true,
            "GLES 3 correctness checks failed; see {}",
            out.join("latest.json").display()
        );
        Ok(report)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentile_and_invalid_samples() {
        let t = timing(&[1., 2., 3., 4., 100.]).unwrap();
        assert_eq!(t.p50_ms, 3.);
        assert_eq!(t.p95_ms, 100.);
        assert_eq!(t.mean_ms, 22.);
        assert!(timing(&[]).is_err());
        assert!(timing(&[f64::NAN]).is_err());
    }
}
