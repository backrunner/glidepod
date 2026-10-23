use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use glidepod_core::{ProfileId, Renderer};
use glidepod_runtime::Store;
use std::path::PathBuf;
#[derive(Parser)]
#[command(version, about = "GlidePod host diagnostics and runtime configuration")]
struct Args {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Read actual host capabilities without creating a VM.
    Doctor,
    /// Execute an isolated ARM64 guest instruction and a Metal compute workload.
    Probe,
    /// Read runtime truth. No backend is reported ready before qualification.
    Status,
    /// Parse APK manifest and ABI metadata locally; does not install or verify signatures.
    InspectApk { path: PathBuf },
    /// Check image files against a local provenance manifest; does not boot them.
    VerifyImage { manifest: PathBuf },
    /// Select the profile used by the management center.
    Profile {
        #[arg(value_enum)]
        profile: ProfileChoice,
    },
    /// Save resource settings; future runtime starts apply them.
    Configure {
        #[arg(long)]
        memory_mib: Option<u32>,
        #[arg(long)]
        cpus: Option<u16>,
        #[arg(long, value_enum)]
        renderer: Option<RendererChoice>,
    },
    /// Register local evaluation artifacts. Does not distribute or qualify them.
    RegisterExperimental {
        #[arg(long)]
        engine: PathBuf,
        #[arg(long)]
        images: PathBuf,
        #[arg(long)]
        fingerprint: String,
    },
    /// Pin a locally built experimental gfxstream / MoltenVK renderer.
    RegisterGpu { directory: PathBuf },
    /// Start an experimental Developer VM in the foreground; Ctrl-C stops it.
    Start,
    /// Stop the live session owned by the app or a foreground CLI.
    Stop,
    /// Refresh the launchable Android app catalog.
    Apps,
    /// Query Android app sizes and requested runtime permissions.
    AppInfo { package: String },
    /// Query free and total bytes on Android's data filesystem.
    Storage,
    /// Run original GLES or direct Vulkan checks on an independent display.
    Benchmark {
        #[arg(long, value_enum, default_value_t = ApiChoice::Gles)]
        api: ApiChoice,
        #[arg(long)]
        validation: bool,
        #[arg(long, hide = true)]
        negative_control: bool,
        /// Vulkan control run without a forced swapchain recreation.
        #[arg(long)]
        no_recreate: bool,
        /// Retain Vulkan stage timings and enable ATrace annotations.
        #[arg(long)]
        trace_stages: bool,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(2..=3))]
        gles: u32,
        #[arg(long, default_value_t = 720)]
        width: u32,
        #[arg(long, default_value_t = 1080)]
        height: u32,
        #[arg(long, default_value_t = 16)]
        objects: u32,
        #[arg(long, default_value_t = 180)]
        samples: u32,
        #[arg(long)]
        no_capture: bool,
        #[arg(long)]
        finish_each_frame: bool,
    },
    /// Clear one user app's cache, or all data with --data --confirm PACKAGE.
    ClearStorage {
        package: String,
        #[arg(long)]
        data: bool,
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Change a runtime permission requested by a user app.
    Permission {
        package: String,
        permission: String,
        #[arg(value_enum)]
        action: PermissionChoice,
    },
    /// Create a Finder/Launchpad entry for an installed Android app.
    AddLauncher { package: String },
    /// Install or update an APK using Android Package Manager signature checks.
    Install { path: PathBuf },
    /// Remove an installed user app and its Android data.
    Uninstall { package: String },
    /// Run a command inside the Developer guest (never the macOS host).
    Shell {
        #[arg(long)]
        root: bool,
        command: String,
    },
    /// Hold an independent app display; optionally capture a PPM frame for diagnostics.
    Window {
        package: String,
        #[arg(long)]
        capture: Option<PathBuf>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        key: Option<u32>,
    },
    #[command(hide = true)]
    RuntimeWorker,
}
#[derive(Clone, Copy, ValueEnum)]
enum ProfileChoice {
    Personal,
    Developer,
}
#[derive(Clone, Copy, ValueEnum)]
enum ApiChoice {
    Gles,
    Vulkan,
}
#[derive(Clone, Copy, ValueEnum)]
enum RendererChoice {
    Automatic,
    Metal,
    Software,
}
#[derive(Clone, Copy, ValueEnum)]
enum PermissionChoice {
    Grant,
    Revoke,
}
fn main() {
    let args = Args::parse();
    let json = args.json;
    let result = run(args);
    if let Err(error) = result {
        if json {
            eprintln!("{}", serde_json::json!({"error":format!("{error:#}")}));
        } else {
            eprintln!("error: {error:#}");
        }
        std::process::exit(1);
    }
}
fn run(args: Args) -> Result<()> {
    let store = || match &args.data_dir {
        Some(path) => Store::new(path.clone()),
        None => Store::default_location(),
    };
    let (value, summary, passed) = match args.command {
        Operation::Doctor => {
            let report = glidepod_macos::host_report();
            let summary = format!(
                "{} · macOS {} · {} MiB · {} CPUs\nHypervisor: {}\nMetal: {}",
                report.architecture,
                report.macos_version,
                report.memory_mib,
                report.logical_cpus,
                report.hypervisor_supported,
                report.metal_device.as_deref().unwrap_or("Unavailable")
            );
            (serde_json::to_value(report)?, summary, true)
        }
        Operation::Probe => {
            let report = glidepod_macos::run_probes();
            let passed = report.probes.iter().all(|p| p.passed);
            let summary = report
                .probes
                .iter()
                .map(|p| {
                    format!(
                        "{} {}: {}",
                        if p.passed { "PASS" } else { "FAIL" },
                        p.name,
                        p.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            (serde_json::to_value(report)?, summary, passed)
        }
        Operation::Status => {
            let s = store()?.snapshot()?;
            let summary = format!(
                "{} · {}\n{}",
                s.configuration.active_profile.label(),
                s.runtime.label(),
                s.runtime.message
            );
            (serde_json::to_value(s)?, summary, true)
        }
        Operation::InspectApk { path } => {
            let r = glidepod_installer::inspect(&path)?;
            let summary = format!(
                "{} · {} bytes\nARM64 candidate: {}\nSignature verification: not performed\nSHA-256: {}",
                r.manifest.package_id, r.bytes, r.arm64_candidate, r.sha256
            );
            (serde_json::to_value(r)?, summary, true)
        }
        Operation::VerifyImage { manifest } => {
            let r = glidepod_runtime::verify_image(&manifest)?;
            let summary = format!(
                "Verified {} files ({} bytes) for {}. Signature and boot qualification remain pending.",
                r.files_verified, r.bytes_verified, r.image_id
            );
            (serde_json::to_value(r)?, summary, true)
        }
        Operation::Profile { profile } => {
            let store = store()?;
            let c = store.load()?;
            let c = store.update(c.revision, |c| {
                c.active_profile = match profile {
                    ProfileChoice::Personal => ProfileId::Personal,
                    ProfileChoice::Developer => ProfileId::Developer,
                }
            })?;
            (
                serde_json::to_value(&c)?,
                format!("Active profile: {}", c.active_profile.label()),
                true,
            )
        }
        Operation::Configure {
            memory_mib,
            cpus,
            renderer,
        } => {
            let store = store()?;
            let c = store.load()?;
            let c = store.update(c.revision, |c| {
                let s = c.settings_mut();
                if let Some(v) = memory_mib {
                    s.memory_mib = v;
                }
                if let Some(v) = cpus {
                    s.vcpus = v;
                }
                if let Some(v) = renderer {
                    s.renderer = match v {
                        RendererChoice::Automatic => Renderer::Automatic,
                        RendererChoice::Metal => Renderer::Metal,
                        RendererChoice::Software => Renderer::Software,
                    };
                }
            })?;
            (
                serde_json::to_value(&c)?,
                "Settings saved. Resource changes apply at the next start.".into(),
                true,
            )
        }
        Operation::RegisterExperimental {
            engine,
            images,
            fingerprint,
        } => {
            let runtime = glidepod_runtime::experimental::ExperimentalRuntime::from_local(
                &engine,
                &images,
                fingerprint,
            )?;
            store()?.register_experimental(&runtime)?;
            (
                serde_json::to_value(runtime)?,
                "Experimental runtime registered. Developer profile only; software rendering."
                    .into(),
                true,
            )
        }
        Operation::RegisterGpu { directory } => {
            let gpu = store()?.register_gpu(&directory)?;
            (serde_json::to_value(gpu)?, "Experimental GPU renderer registered. Select Metal explicitly; game compatibility remains unqualified.".into(), true)
        }
        Operation::RuntimeWorker => return glidepod_runtime::experimental::run_worker(&store()?),
        Operation::AppInfo { package } => {
            let details = store()?.guest()?.app_details(&package)?;
            (
                serde_json::to_value(details)?,
                format!("App information: {package}"),
                true,
            )
        }
        Operation::Storage => {
            let s = store()?.guest()?.storage_overview()?;
            let summary = format!(
                "{} available / {} total bytes in Android",
                s.available_bytes, s.total_bytes
            );
            (serde_json::to_value(s)?, summary, true)
        }
        Operation::Benchmark {
            api,
            validation,
            negative_control,
            no_recreate,
            trace_stages,
            gles,
            width,
            height,
            objects,
            samples,
            no_capture,
            finish_each_frame,
        } => {
            let r = store()?
                .guest()?
                .benchmark(glidepod_runtime::benchmark::BenchmarkRequest {
                    api: match api {
                        ApiChoice::Gles => glidepod_runtime::benchmark::GraphicsApi::Gles,
                        ApiChoice::Vulkan => glidepod_runtime::benchmark::GraphicsApi::Vulkan,
                    },
                    validation,
                    negative_control,
                    no_recreate,
                    trace_stages,
                    gles_version: gles,
                    width,
                    height,
                    objects,
                    samples,
                    capture_frames: !no_capture,
                    finish_each_frame,
                })?;
            let summary = format!(
                "Guest 3D: {:.1} FPS, frame p95 {:.2} ms, draw p95 {:.2} ms. Native presentation not measured.",
                r.mean_fps, r.frame_interval.p95_ms, r.draw.p95_ms
            );
            (serde_json::to_value(r)?, summary, true)
        }
        Operation::ClearStorage {
            package,
            data,
            confirm,
        } => {
            anyhow::ensure!(
                !data || confirm.as_deref() == Some(&package),
                "Clearing app data deletes its files and accounts. Pass --confirm {package} to confirm."
            );
            let kind = if data {
                glidepod_runtime::management::ClearStorage::Data
            } else {
                glidepod_runtime::management::ClearStorage::Cache
            };
            let d = store()?.guest()?.clear_app_storage(&package, kind)?;
            (
                serde_json::to_value(d)?,
                format!(
                    "Cleared {} for {package}",
                    if data { "data" } else { "cache" }
                ),
                true,
            )
        }
        Operation::Permission {
            package,
            permission,
            action,
        } => {
            let d = store()?.guest()?.set_permission(
                &package,
                &permission,
                matches!(action, PermissionChoice::Grant),
            )?;
            (
                serde_json::to_value(d)?,
                format!("Updated {permission} for {package}"),
                true,
            )
        }
        Operation::Start => return run_foreground(&store()?, args.json),
        Operation::Stop => {
            let status = store()?.stop_vm()?;
            (
                serde_json::to_value(status)?,
                "Android shutdown requested.".into(),
                true,
            )
        }
        Operation::Apps => {
            let apps = store()?.guest()?.catalog()?;
            let text = apps
                .iter()
                .map(|a| format!("{}  {}", a.package_id, a.label))
                .collect::<Vec<_>>()
                .join("\n");
            (serde_json::to_value(apps)?, text, true)
        }
        Operation::AddLauncher { package } => {
            let apps = store()?.cached_apps()?;
            let app = apps
                .iter()
                .find(|a| a.package_id == package)
                .ok_or_else(|| anyhow::anyhow!("Refresh the app catalog first"))?;
            let path = glidepod_macos::launchers::create(
                &app.package_id,
                &app.label,
                app.icon_path.as_deref(),
            )?;
            (
                serde_json::json!({"path":path}),
                format!("Created {}", path.display()),
                true,
            )
        }
        Operation::Install { path } => {
            let guest = store()?.guest()?;
            let receipt = guest.install(&path, None)?;
            let catalog = guest.catalog();
            let message = format!(
                "Installed {}{}",
                receipt.package_id,
                if catalog.is_err() {
                    "; refresh the app list to load metadata"
                } else {
                    ""
                }
            );
            (serde_json::to_value(receipt)?, message, true)
        }
        Operation::Uninstall { package } => {
            store()?.guest()?.uninstall(&package)?;
            (
                serde_json::json!({"removed":package}),
                format!("Removed {package}"),
                true,
            )
        }
        Operation::Shell { root, command } => {
            let output = store()?.guest()?.developer_shell(&command, root)?;
            (serde_json::json!({"stdout":output}), output, true)
        }
        Operation::Window {
            package,
            capture,
            text,
            key,
        } => {
            let window = store()?.guest()?.open_app(&package, 720, 1080)?;
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Some(text) = text {
                window.text(&text)?;
            }
            if let Some(key) = key {
                window.key(key)?;
            }
            if let Some(path) = capture {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let frame = window
                    .frame(0)?
                    .ok_or_else(|| anyhow::anyhow!("No frame published"))?;
                use std::io::Write;
                let mut file = std::fs::File::create(path)?;
                write!(file, "P6\n{} {}\n255\n", frame.width, frame.height)?;
                for rgba in frame.rgba.as_chunks::<4>().0 {
                    file.write_all(&rgba[..3])?;
                }
            } else {
                unsafe {
                    libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
                    libc::signal(
                        libc::SIGTERM,
                        interrupted as *const () as libc::sighandler_t,
                    );
                }
                while !INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed) {
                    window.guest.verify_epoch()?;
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
            (
                serde_json::to_value(&window.info)?,
                "App display closed.".into(),
                true,
            )
        }
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{summary}");
    }
    if !passed {
        std::process::exit(2);
    }
    Ok(())
}

static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}
fn run_foreground(store: &Store, json: bool) -> Result<()> {
    use std::{sync::atomic::Ordering, thread, time::Duration};
    // Only the CLI foreground owner changes signal handling; the GPUI process does not.
    unsafe {
        libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
        libc::signal(
            libc::SIGTERM,
            interrupted as *const () as libc::sighandler_t,
        );
    }
    let mut owner = Some(store.start_experimental(&std::env::current_exe()?)?);
    let mut previous = String::new();
    loop {
        if INTERRUPTED.load(Ordering::Relaxed) {
            owner.take();
        }
        let status = store.runtime_status()?;
        let encoded = serde_json::to_string(&status)?;
        if previous != encoded {
            if json {
                println!("{encoded}");
            } else {
                println!("{} — {}", status.label(), status.message);
            }
            previous = encoded;
        }
        if !status.active() {
            if let glidepod_core::VmState::Failed { message } = status.vm {
                anyhow::bail!(message);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
}
