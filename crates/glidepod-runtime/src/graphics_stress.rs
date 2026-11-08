//! Offscreen, completion-bounded GLES pressure tests; never native-display FPS.
use crate::{benchmark::StopWorkload, guest::Guest};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Workload {
    DrawCalls,
    Fragment,
    Texture,
}
impl Workload {
    pub fn name(self) -> &'static str {
        match self {
            Self::DrawCalls => "draw_calls",
            Self::Fragment => "fragment",
            Self::Texture => "texture",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressRequest {
    pub workload: Workload,
    pub width: u32,
    pub height: u32,
    pub seconds: u32,
}
impl StressRequest {
    fn validate(&self) -> Result<()> {
        ensure!(
            (320..=2048).contains(&self.width)
                && (320..=2048).contains(&self.height)
                && (1..=120).contains(&self.seconds),
            "Stress bounds: 320–2048 pixels and 1–120 seconds"
        );
        Ok(())
    }
}
impl Guest {
    pub fn vulkan_stress(
        &self,
        width: u32,
        height: u32,
        objects: u32,
        seconds: u32,
        validation: bool,
    ) -> Result<serde_json::Value> {
        StressRequest {
            workload: Workload::DrawCalls,
            width,
            height,
            seconds,
        }
        .validate()?;
        ensure!(
            (1..=4096).contains(&objects),
            "Vulkan stress requires 1–4096 objects"
        );
        self.verify_epoch()?;
        let runtime = self.store.runtime_status()?;
        let package = "dev.glidepod.vulkan";
        self.ensure_workload_apk("glidepod-vulkan.apk", package)?;
        self.stop_app(package)?;
        let window = self.open_app(package, width, height)?;
        let _workload = StopWorkload(self.clone(), package);
        let run = format!(
            "{:016x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        self.shell(&format!("am start -W --display {} -f 0x10008000 -n {package}/.MainActivity --es run {run} --ei objects {objects} --ei stress_seconds {seconds} --ez validation {validation}", window.info.display_id))?;
        let started = Instant::now();
        let guest = loop {
            ensure!(
                started.elapsed() < Duration::from_secs(u64::from(seconds) + 60),
                "Vulkan stress timed out"
            );
            self.verify_epoch()?;
            let result = self.provider("dev.glidepod.vulkan.results", "result", None)?;
            let value: serde_json::Value = serde_json::from_str(result.trim())?;
            if value["ready"] == true && value["run"] == run {
                break value;
            }
            thread::sleep(Duration::from_millis(500));
        };
        let report = serde_json::json!({"request":{"api":"vulkan", "width":width,"height":height,"objects":objects,"seconds":seconds,"validation":validation},"runtime":runtime,"guest":guest,"native_presentation_measured":false});
        let out = self.store.root().join("developer/stress");
        fs::create_dir_all(&out)?;
        fs::write(
            out.join("latest-vulkan.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        ensure!(
            guest.get("error").is_none(),
            "Vulkan stress failed: {}",
            guest["error"]
        );
        ensure!(
            guest["width"] == width
                && guest["height"] == height
                && guest["objects"] == objects
                && guest["seconds_requested"] == seconds
                && guest["vk_success"] == true
                && guest["validation_enabled"] == validation
                && guest["validation_errors"] == 0
                && guest["frames"].as_u64().unwrap_or(0) > 0
                && guest["measured_ms"].as_f64().unwrap_or(0.) >= f64::from(seconds) * 1000.,
            "Vulkan stress correctness, workload or duration check failed"
        );
        Ok(report)
    }

    pub fn graphics_stress(&self, request: StressRequest) -> Result<serde_json::Value> {
        request.validate()?;
        self.verify_epoch()?;
        let runtime = self.store.runtime_status()?;
        let package = "dev.glidepod.benchmark";
        self.ensure_workload_apk("glidepod-benchmark.apk", package)?;
        self.stop_app(package)?;
        let window = self.open_app(package, request.width, request.height)?;
        let _workload = StopWorkload(self.clone(), package);
        let run = format!(
            "{:016x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        self.shell(&format!("am start -W --display {} -f 0x10008000 -n {package}/.MainActivity --es run {run} --es stress {} --ei seconds {}", window.info.display_id, request.workload.name(), request.seconds))?;
        let started = Instant::now();
        let guest = loop {
            ensure!(
                started.elapsed() < Duration::from_secs(u64::from(request.seconds) + 60),
                "GLES stress timed out"
            );
            self.verify_epoch()?;
            let result = self.provider("dev.glidepod.benchmark.results", "result", None)?;
            let value: serde_json::Value = serde_json::from_str(result.trim())?;
            if value["ready"] == true && value["run"] == run {
                break value;
            }
            thread::sleep(Duration::from_millis(500));
        };
        let report = serde_json::json!({"request":request,"runtime":runtime,"guest":guest,"native_presentation_measured":false});
        let out = self.store.root().join("developer/stress");
        fs::create_dir_all(&out)?;
        fs::write(out.join("latest.json"), serde_json::to_vec_pretty(&report)?)?;
        ensure!(
            guest.get("error").is_none(),
            "GLES stress failed: {}",
            guest["error"]
        );
        ensure!(
            guest["workload"] == request.workload.name()
                && guest["width"] == request.width
                && guest["height"] == request.height
                && guest["seconds_requested"] == request.seconds,
            "Stress workload identity mismatch"
        );
        ensure!(
            guest["gl_error"] == 0
                && guest["reference_pixel_passed"] == true
                && guest["gles3_correctness"]["passed"] == true
                && guest["frames"].as_u64().unwrap_or(0) > 0
                && guest["measured_ms"].as_f64().unwrap_or(0.)
                    >= f64::from(request.seconds) * 1000.,
            "Stress correctness or duration check failed"
        );
        Ok(report)
    }
}
