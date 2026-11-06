//! Inventory is a prerequisite check, not Google API, account or certification validation.
use crate::guest::Guest;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

const PACKAGES: [&str; 3] = [
    "com.google.android.gms",
    "com.google.android.gsf",
    "com.android.vending",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GooglePackage {
    pub package_id: String,
    pub installed: bool,
    pub enabled: bool,
    pub system: bool,
    pub version: String,
    pub version_code: u64,
    /// Observed signing certificates, not an authenticity verdict.
    pub signer_sha256: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleInventory {
    pub sdk: u32,
    pub release: String,
    pub build_type: String,
    pub build_tags: String,
    pub fingerprint: String,
    pub abis: Vec<String>,
    pub packages: Vec<GooglePackage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoogleServicesReport {
    pub schema_version: u32,
    pub epoch: u64,
    pub inventory: GoogleInventory,
    pub package_state: &'static str,
    pub api_availability: &'static str,
    pub account_sign_in: &'static str,
    pub play_store: &'static str,
    pub fcm_delivery: &'static str,
    pub play_protect_certification: &'static str,
    pub play_integrity: &'static str,
}

impl GoogleServicesReport {
    fn from_inventory(epoch: u64, inventory: GoogleInventory) -> Result<Self> {
        ensure!(
            inventory.packages.len() == PACKAGES.len(),
            "Incomplete Google package inventory"
        );
        for expected in PACKAGES {
            ensure!(
                inventory
                    .packages
                    .iter()
                    .filter(|p| p.package_id == expected)
                    .count()
                    == 1,
                "Invalid Google package inventory"
            );
        }
        for p in &inventory.packages {
            ensure!(
                p.version.len() <= 1024
                    && p.signer_sha256.len() <= 16
                    && p.signer_sha256
                        .iter()
                        .all(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
                "Invalid Google package metadata"
            );
        }
        let package_state = if inventory.packages.iter().any(|p| !p.installed) {
            "missing_components"
        } else if inventory.packages.iter().any(|p| !p.enabled) {
            "disabled_components"
        } else {
            "present_unverified"
        };
        Ok(Self {
            schema_version: 1,
            epoch,
            inventory,
            package_state,
            api_availability: "not_tested",
            account_sign_in: "not_tested",
            play_store: "not_tested",
            fcm_delivery: "not_tested",
            play_protect_certification: "not_tested",
            play_integrity: "not_tested",
        })
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!("Google services: {}", self.package_state)];
        for p in &self.inventory.packages {
            lines.push(format!(
                "{}: {}{}",
                p.package_id,
                if !p.installed {
                    "missing"
                } else if !p.enabled {
                    "disabled"
                } else {
                    "present"
                },
                if p.installed {
                    format!(" ({})", p.version)
                } else {
                    String::new()
                }
            ));
        }
        lines.push("Package presence does not verify Google APIs, sign-in, Play Store, FCM or device certification.".into());
        lines.join("\n")
    }
}

impl Guest {
    pub fn google_services(&self) -> Result<GoogleServicesReport> {
        let output = self.bridge("dev.glidepod.bridge.GoogleServices", &[])?;
        ensure!(
            output.len() <= 64 * 1024,
            "Google services inventory exceeds limit"
        );
        let inventory = serde_json::from_str(output.trim())?;
        GoogleServicesReport::from_inventory(self.endpoints.epoch, inventory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> GoogleInventory {
        GoogleInventory {
            sdk: 37,
            release: "17".into(),
            build_type: "userdebug".into(),
            build_tags: "test-keys".into(),
            fingerprint: "fixture".into(),
            abis: vec!["arm64-v8a".into()],
            packages: PACKAGES
                .into_iter()
                .map(|name| GooglePackage {
                    package_id: name.into(),
                    installed: true,
                    enabled: true,
                    system: true,
                    version: "1".into(),
                    version_code: 1,
                    signer_sha256: vec!["a".repeat(64)],
                })
                .collect(),
        }
    }

    #[test]
    fn installed_packages_never_claim_working_services_or_certification() {
        let report = GoogleServicesReport::from_inventory(12, inventory()).unwrap();
        assert_eq!(report.package_state, "present_unverified");
        assert_eq!(report.api_availability, "not_tested");
        assert_eq!(report.fcm_delivery, "not_tested");
        assert_eq!(report.play_protect_certification, "not_tested");
        let mut missing = inventory();
        missing.packages[0].installed = false;
        assert_eq!(
            GoogleServicesReport::from_inventory(12, missing)
                .unwrap()
                .package_state,
            "missing_components"
        );
        let mut disabled = inventory();
        disabled.packages[0].enabled = false;
        assert_eq!(
            GoogleServicesReport::from_inventory(12, disabled)
                .unwrap()
                .package_state,
            "disabled_components"
        );
    }

    #[test]
    fn duplicate_or_unexpected_packages_cannot_satisfy_prerequisites() {
        let mut duplicate = inventory();
        duplicate.packages[0] = duplicate.packages[1].clone();
        assert!(GoogleServicesReport::from_inventory(12, duplicate).is_err());
        let mut invalid = inventory();
        invalid.packages[0].signer_sha256 = vec!["unknown".into()];
        assert!(GoogleServicesReport::from_inventory(12, invalid).is_err());
    }

    #[test]
    #[ignore = "downloads/stages proprietary GApps only via tools/evaluate-google-apps.py in a NEW disposable VM"]
    fn isolated_google_apps_candidate() -> Result<()> {
        use crate::{
            Store,
            adb::{Adb, quote},
            experimental::RuntimeOwner,
        };
        use anyhow::Context;
        use glidepod_core::VmState;
        use std::{
            fs,
            path::{Path, PathBuf},
            thread,
            time::{Duration, Instant},
        };

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let path = PathBuf::from(
            std::env::var_os("GLIDEPOD_GAPPS_STORE")
                .context("Use the candidate evaluation tool")?,
        )
        .canonicalize()?;
        ensure!(
            path.starts_with(root.join(".cache")) && path != root.join(".cache"),
            "Disposable project-local store required"
        );
        let marker: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("google-apps-evaluation.json"))?)?;
        ensure!(
            marker["purpose"] == "disposable_google_apps_candidate",
            "Evaluation marker missing"
        );
        ensure!(
            !path.join("developer/instance").exists(),
            "Never install over an existing VM in this evaluator"
        );
        let payload =
            PathBuf::from(std::env::var_os("GLIDEPOD_GAPPS_PAYLOAD").context("Missing payload")?)
                .canonicalize()?;
        let plan: serde_json::Value = serde_json::from_slice(&fs::read(PathBuf::from(
            std::env::var_os("GLIDEPOD_GAPPS_PLAN").context("Missing plan")?,
        ))?)?;
        ensure!(
            payload.starts_with(root.join(".cache/google-apps"))
                && payload.metadata()?.len() <= 2 * 1024 * 1024 * 1024,
            "Invalid staged payload"
        );
        ensure!(
            marker["payload_sha256"] == plan["payload_sha256"]
                && plan["payload_sha256"] == crate::experimental::digest(&payload)?,
            "Staged payload changed"
        );
        let files = plan["files"].as_array().context("Missing payload paths")?;
        ensure!(
            !files.is_empty() && files.len() <= 4096,
            "Invalid payload count"
        );
        let mut paths = Vec::new();
        for file in files {
            let file = file.as_str().context("Invalid payload path")?;
            ensure!(
                (file.starts_with("product/") || file.starts_with("system_ext/"))
                    && file.split('/').all(|p| !p.is_empty()
                        && p != "."
                        && p != ".."
                        && p.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
                "Unsafe payload path"
            );
            paths.push(file.to_owned());
        }
        paths.sort_by_key(|p| (!p.contains("/etc/"), p.clone()));

        struct Candidate {
            store: Store,
            owner: Option<RuntimeOwner>,
        }
        impl Drop for Candidate {
            fn drop(&mut self) {
                let _ = self.store.stop_vm();
                self.owner.take();
            }
        }
        fn start(candidate: &mut Candidate, helper: &Path) -> Result<Guest> {
            candidate.owner = Some(candidate.store.start_experimental(helper)?);
            let deadline = Instant::now() + Duration::from_secs(140);
            loop {
                let state = candidate.store.runtime_status()?;
                match state.vm {
                    VmState::Running { .. } => return candidate.store.guest(),
                    VmState::Failed { message } => {
                        anyhow::bail!("Candidate boot failed: {message}")
                    }
                    _ => {}
                }
                ensure!(Instant::now() < deadline, "Candidate boot timed out");
                thread::sleep(Duration::from_millis(300));
            }
        }
        fn stop(candidate: &mut Candidate) -> Result<()> {
            candidate.store.stop_vm()?;
            candidate.owner.take();
            let deadline = Instant::now() + Duration::from_secs(30);
            while candidate.store.runtime_status()?.active() {
                ensure!(Instant::now() < deadline, "Candidate stop timed out");
                thread::sleep(Duration::from_millis(200));
            }
            Ok(())
        }
        fn root_shell(guest: &Guest, script: &str) -> Result<String> {
            guest.shell_timeout(
                &format!("su 0 sh -c {}", quote(script)),
                Duration::from_secs(180),
            )
        }
        let mut candidate = Candidate {
            store: Store::new(path)?,
            owner: None,
        };
        let helper = root.join("target/debug/glidepodctl");
        eprintln!("GApps candidate: starting a clean isolated Android instance");
        let mut guest = start(&mut candidate, &helper)?;
        let before = guest.google_services()?;
        ensure!(
            before.inventory.sdk == plan["sdk"]
                && before.inventory.abis.iter().any(|a| a == "arm64-v8a"),
            "Guest does not match GApps package"
        );
        ensure!(
            before.inventory.packages.iter().all(|p| !p.installed),
            "Google packages already exist"
        );
        ensure!(
            candidate.store.runtime_status()?.effective_root,
            "Candidate requires the Developer guest adapter"
        );
        let verity = root_shell(&guest, "avbctl get-verity")?;
        if verity.contains("verity is enabled") {
            eprintln!("GApps candidate: disabling verity only on this disposable instance");
            root_shell(&guest, "avbctl disable-verity")?;
            stop(&mut candidate)?;
            guest = start(&mut candidate, &helper)?;
        } else {
            ensure!(
                verity.contains("verity is disabled"),
                "Unknown verity state"
            );
        }
        let remount = root_shell(&guest, "remount")?;
        if remount.to_ascii_lowercase().contains("reboot your device") {
            eprintln!("GApps candidate: restarting for the private overlay");
            stop(&mut candidate)?;
            guest = start(&mut candidate, &helper)?;
            root_shell(&guest, "remount")?;
        }
        root_shell(
            &guest,
            "set -e; touch /product/.glidepod-probe /system_ext/.glidepod-probe; rm /product/.glidepod-probe /system_ext/.glidepod-probe; if grep -q ' /mnt/scratch ' /proc/mounts; then mount -o remount,async /mnt/scratch; fi",
        )?;
        let mut collision_probe = String::from("set -e\n");
        for file in &paths {
            collision_probe.push_str(&format!("test ! -e {}\n", quote(&format!("/{file}"))));
        }
        root_shell(&guest, &collision_probe)?;
        let space = guest.storage_overview()?;
        let required_space = payload.metadata()?.len() + 256 * 1024 * 1024;
        let scratch_stat = root_shell(&guest, "stat -f -c '%a %S' /mnt/scratch")?;
        let scratch_fields = scratch_stat
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            scratch_fields.len() == 2,
            "Invalid scratch filesystem statistics"
        );
        let scratch_free = scratch_fields[0]
            .checked_mul(scratch_fields[1])
            .context("Scratch size overflow")?;
        let scratch_required = plan["payload_bytes"]
            .as_u64()
            .context("Missing payload size")?
            * 2
            + 256 * 1024 * 1024;
        eprintln!(
            "GApps candidate: {} bytes free, {} required including staging and headroom",
            space.available_bytes, required_space
        );
        ensure!(
            space.available_bytes >= required_space,
            "Not enough guest data space for the archive and headroom"
        );
        ensure!(
            scratch_free >= scratch_required,
            "Not enough scratch space for staging, overlay and headroom"
        );
        eprintln!("GApps candidate: streaming bounded payload to the private guest");
        let transfer_started = Instant::now();
        Adb::push(
            &guest.endpoints.vsock,
            &payload,
            "/data/local/tmp/glidepod/google-apps.tar",
        )?;
        let transfer_ms = transfer_started.elapsed().as_millis();
        let guest_hash = guest.shell_timeout(
            "sha256sum /data/local/tmp/glidepod/google-apps.tar",
            Duration::from_secs(120),
        )?;
        ensure!(
            guest_hash.split_whitespace().next() == plan["payload_sha256"].as_str(),
            "Guest payload digest mismatch"
        );
        eprintln!("GApps candidate: payload transferred and verified in {transfer_ms} ms");
        root_shell(
            &guest,
            "set -e; mkdir -m 700 /mnt/scratch/glidepod-google-apps; tar -xf /data/local/tmp/glidepod/google-apps.tar -C /mnt/scratch/glidepod-google-apps; rm /data/local/tmp/glidepod/google-apps.tar",
        )?;
        let mut commit = String::from("set -e\n");
        for file in &paths {
            let destination = format!("/{file}");
            let parent = Path::new(&destination)
                .parent()
                .context("Invalid destination")?
                .to_str()
                .context("Non-UTF8 path")?;
            commit.push_str(&format!(
                "mkdir -p {}\ncp {} {}\nchown 0:0 {}\nchmod 644 {}\n",
                quote(parent),
                quote(&format!("/mnt/scratch/glidepod-google-apps/{file}")),
                quote(&destination),
                quote(&destination),
                quote(&destination)
            ));
        }
        commit.push_str(
            "restorecon -RF /product /system_ext\nsync\nrm -rf /mnt/scratch/glidepod-google-apps\n",
        );
        eprintln!("GApps candidate: installing allowlists before system APKs");
        root_shell(&guest, &commit)?;
        stop(&mut candidate)?;
        eprintln!("GApps candidate: verifying Android after installation");
        guest = start(&mut candidate, &helper)?;
        let after = guest.google_services()?;
        ensure!(
            after.package_state == "present_unverified"
                && after
                    .inventory
                    .packages
                    .iter()
                    .all(|p| p.system && !p.signer_sha256.is_empty()),
            "Google system packages failed prerequisites"
        );
        let variant = plan["variant"]
            .as_str()
            .context("Missing candidate variant")?;
        ensure!(
            matches!(variant, "core" | "full"),
            "Unknown candidate variant"
        );
        let evidence = root
            .join("evidence/gms-notifications-2026-11-05")
            .join(format!("candidate-{variant}"));
        fs::create_dir_all(&evidence)?;
        fs::write(
            evidence.join("before.json"),
            serde_json::to_vec_pretty(&before)?,
        )?;
        fs::write(
            evidence.join("after.json"),
            serde_json::to_vec_pretty(&after)?,
        )?;
        let window = guest.open_app("com.android.vending", 720, 1080)?;
        let until = Instant::now() + Duration::from_secs(20);
        let mut captured = false;
        thread::sleep(Duration::from_secs(3));
        while Instant::now() < until {
            if let Some(frame) = window.frame(0)? {
                use std::io::Write;
                let mut image = fs::File::create(evidence.join("play-store.ppm"))?;
                write!(image, "P6\n{} {}\n255\n", frame.width, frame.height)?;
                for pixel in frame.pixels.as_chunks::<4>().0 {
                    image.write_all(&pixel[..3])?;
                }
                captured = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        ensure!(
            captured && window.task_count()? > 0,
            "Play Store did not produce a live task/frame"
        );
        drop(window);
        stop(&mut candidate)?;
        fs::write(
            evidence.join("result.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "isolated_installation_passed": true, "play_store_task_and_frame": true,
                "account_sign_in_verified": false, "fcm_delivery_verified": false,
                "certification_verified": false, "production_installer_implemented": false,
                "candidate_stopped": true, "payload_bytes": plan["payload_bytes"]
            , "payload_transfer_ms": transfer_ms, "guest_payload_sha256_verified": true
            , "variant": plan["variant"], "data_space_before": space
            , "scratch_available_before": scratch_free
            }))?,
        )?;
        Ok(())
    }
}
