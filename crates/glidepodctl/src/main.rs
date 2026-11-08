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
    /// Inspect macOS system proxies and the pinned VMM's coverage; no traffic or settings changes.
    Network,
    /// Select Shared or Bridged networking; changes apply at the next Android start.
    NetworkMode {
        #[arg(value_enum)]
        mode: Option<NetworkChoice>,
        #[arg(long)]
        interface: Option<String>,
    },
    /// Select Off, Host (Core Location), or a fixed Virtual location.
    Location {
        #[arg(value_enum)]
        source: Option<LocationChoice>,
        #[arg(long, allow_hyphen_values = true)]
        latitude: Option<f64>,
        #[arg(long, allow_hyphen_values = true)]
        longitude: Option<f64>,
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        altitude: f64,
        #[arg(long, default_value_t = 10.0)]
        accuracy: f64,
    },
    /// Inventory Google services in the running guest; does not claim API or certification success.
    Gms,
    /// Install, remove or recover optional Google services with transactional disk recovery.
    GoogleServices {
        #[arg(value_enum)]
        action: GoogleAction,
    },
    /// Execute an isolated ARM64 guest instruction and a Metal compute workload.
    Probe,
    /// Read runtime truth. No backend is reported ready before qualification.
    Status,
    /// Inspect guest RAM backing without double-counting host/guest mappings.
    Memory,
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
        #[arg(long, value_enum)]
        builtin_apps: Option<BuiltinAppsChoice>,
        #[arg(long, value_enum)]
        runtime_mode: Option<RuntimeModeChoice>,
        #[arg(long, value_enum)]
        memory_mode: Option<MemoryModeChoice>,
        #[arg(long)]
        memory_mib: Option<u32>,
        #[arg(long)]
        cpus: Option<u16>,
        #[arg(long, value_enum)]
        renderer: Option<RendererChoice>,
        /// Guest display rate, applied at the next start: 60, 90 or 120 Hz.
        #[arg(long)]
        refresh_rate: Option<u32>,
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
    /// Pin an external gfxstream renderer and its Vulkan-to-Metal driver.
    RegisterGpu {
        directory: PathBuf,
        #[arg(long, value_enum, default_value_t = VulkanDriverChoice::Moltenvk)]
        driver: VulkanDriverChoice,
    },
    /// Show registered Vulkan drivers, or select one for the next Metal start.
    GpuDriver {
        #[arg(value_enum)]
        driver: Option<VulkanDriverChoice>,
    },
    /// Run an uncapped offscreen GLES workload; measures completion, not display FPS.
    GraphicsStress {
        #[arg(long, value_enum)]
        workload: StressChoice,
        #[arg(long, default_value_t = 1024)]
        width: u32,
        #[arg(long, default_value_t = 1024)]
        height: u32,
        #[arg(long, default_value_t = 30)]
        seconds: u32,
    },
    /// Run an uncapped indexed Vulkan scene with fence completion per frame.
    VulkanStress {
        #[arg(long, default_value_t = 1024)]
        width: u32,
        #[arg(long, default_value_t = 1024)]
        height: u32,
        #[arg(long, default_value_t = 256)]
        objects: u32,
        #[arg(long, default_value_t = 30)]
        seconds: u32,
        #[arg(long)]
        validation: bool,
    },
    /// Start an experimental Developer VM in the foreground; Ctrl-C stops it.
    Start,
    /// Stop the live session owned by the app or a foreground CLI.
    Stop,
    /// Refresh the launchable Android app catalog.
    Apps,
    /// Reconcile installed apps with Finder and Spotlight entries without launching them.
    SyncLaunchers,
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
    #[command(hide = true)]
    BackgroundWorker,
    /// Inspect the live window-independent VM/notification owner.
    BackgroundStatus,
    /// Start Android under the persistent background owner.
    BackgroundStart,
    /// Stop the background runtime and exit its menu bar/notification agent.
    BackgroundQuit,
    /// Keep a package online for push/background work even with no open window.
    BackgroundApp {
        package: String,
        #[arg(long)]
        disable: bool,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum RuntimeModeChoice {
    Automatic,
    Resident,
}
#[derive(Clone, Copy, ValueEnum)]
enum MemoryModeChoice {
    Automatic,
    Manual,
}
#[derive(Clone, Copy, ValueEnum)]
enum GoogleAction {
    Install,
    Remove,
    Enable,
    Disable,
    Recover,
    Status,
}
#[derive(Clone, Copy, ValueEnum)]
enum NetworkChoice {
    Shared,
    Bridged,
}
#[derive(Clone, Copy, ValueEnum)]
enum LocationChoice {
    Off,
    Host,
    Virtual,
}
#[derive(Clone, Copy, ValueEnum)]
enum BuiltinAppsChoice {
    Minimal,
    Full,
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
enum VulkanDriverChoice {
    Moltenvk,
    Kosmickrisp,
}
#[derive(Clone, Copy, ValueEnum)]
enum StressChoice {
    DrawCalls,
    Fragment,
    Texture,
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
        Operation::Memory => {
            let report = store()?.guest()?.memory_residency()?;
            let summary = format!(
                "Guest RAM backing: {:.0} MiB resident · {:.0} MiB paged out (logical) · {:.0} MiB unbacked\nCapacity: {:.0} MiB · scan: {} ms\nExcludes VMM/graphics allocations. macOS process footprint can count host/guest aliases twice; these values are not the total VM process footprint.",
                report.resident_bytes as f64 / 1048576.,
                report.paged_out_bytes as f64 / 1048576.,
                report.unbacked_bytes as f64 / 1048576.,
                report.capacity_bytes as f64 / 1048576.,
                report.scan_ms,
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
        Operation::NetworkMode { mode, interface } => {
            let store = store()?;
            if let Some(mode) = mode {
                let mode = match mode {
                    NetworkChoice::Shared => glidepod_runtime::network::Mode::Shared,
                    NetworkChoice::Bridged => glidepod_runtime::network::Mode::Bridged,
                };
                let selected = glidepod_runtime::network::Configuration { mode, interface };
                selected.validate()?;
                if mode == glidepod_runtime::network::Mode::Bridged {
                    anyhow::ensure!(
                        glidepod_macos::network::bridge_interfaces()?
                            .contains(selected.interface.as_ref().unwrap()),
                        "Bridge interface is unavailable"
                    );
                }
                let config = store.load()?;
                store.update(config.revision, |c| c.network = selected)?;
            } else {
                anyhow::ensure!(
                    interface.is_none(),
                    "Choose a network mode with --interface"
                );
            }
            let config = store.load()?;
            let runtime = store.runtime_status()?;
            let startup = if let glidepod_core::VmState::Running { epoch } = runtime.vm {
                store.network_startup_status(epoch)
            } else {
                None
            };
            let effective = if matches!(runtime.vm, glidepod_core::VmState::Running { .. }) {
                runtime.effective_network
            } else {
                None
            };
            (
                serde_json::json!({"desired":config.network,"effective":effective,"startup":startup,"bridge_interfaces":glidepod_macos::network::bridge_interfaces()?}),
                format!(
                    "Network: {:?}. Changes apply at the next Android start; bridge failures never fall back to Shared.",
                    config.network.mode
                ),
                true,
            )
        }
        Operation::Location {
            source,
            latitude,
            longitude,
            altitude,
            accuracy,
        } => {
            let store = store()?;
            if let Some(source) = source {
                let source = match source {
                    LocationChoice::Off => glidepod_runtime::location::Source::Off,
                    LocationChoice::Host => glidepod_runtime::location::Source::Host,
                    LocationChoice::Virtual => glidepod_runtime::location::Source::Virtual,
                };
                let point = if source == glidepod_runtime::location::Source::Virtual {
                    Some(glidepod_runtime::location::Point {
                        latitude: latitude.ok_or_else(|| anyhow::anyhow!("Provide --latitude"))?,
                        longitude: longitude
                            .ok_or_else(|| anyhow::anyhow!("Provide --longitude"))?,
                        altitude,
                        accuracy,
                    })
                } else {
                    anyhow::ensure!(
                        latitude.is_none() && longitude.is_none(),
                        "Coordinates require Virtual location"
                    );
                    None
                };
                let selected = glidepod_runtime::location::Configuration { source, point };
                selected.validate()?;
                let config = store.load()?;
                store.update(config.revision, |c| c.location = selected)?;
            } else {
                anyhow::ensure!(
                    latitude.is_none() && longitude.is_none(),
                    "Choose Virtual location to set coordinates"
                );
            }
            let status = store.location_status()?;
            (serde_json::to_value(&status)?, status.message, true)
        }
        Operation::Network => {
            let proxy = glidepod_macos::network::system_proxy_snapshot()?;
            let summary = format!(
                "macOS system proxy snapshot\nHTTP: {}\nHTTPS: {}\nSOCKS: {}\nPAC enabled: {} · Auto discovery: {}\nBypass rules: {} · Exclude simple hostnames: {}\nPinned VMM: host sockets; system proxy settings are not applied.\nThis is configuration discovery. Proxy reachability and VPN routing have not been tested.",
                proxy.http.summary(),
                proxy.https.summary(),
                proxy.socks.summary(),
                proxy.pac_enabled,
                proxy.auto_discovery_enabled,
                proxy.bypass_rule_count,
                proxy.exclude_simple_hostnames,
            );
            (
                serde_json::json!({
                    "schema_version": 1,
                    "scope": "system_proxy_inventory_and_pinned_backend_source_audit",
                    "system_proxy": proxy,
                    "pinned_backend": {
                        "revision": glidepod_runtime::experimental::ENGINE_REVISION,
                        "egress": "host_sockets",
                        "ip_version": "ipv4_only",
                        "system_proxy_applied": false,
                        "dns": "forwarded_to_resolv_conf_nameserver",
                    },
                    "proxy_reachability_tested": false,
                    "vpn_routing_verified": false,
                    "pac_evaluated": false,
                    "settings_changed": false,
                }),
                summary,
                true,
            )
        }
        Operation::GoogleServices { action } => {
            let store = store()?;
            match action {
                GoogleAction::Status => println!(
                    "{}",
                    serde_json::json!({"installation":store.google_apps_installation()?,"recovery_pending":store.google_apps_recovery_pending()})
                ),
                GoogleAction::Recover => store.recover_google_apps()?,
                GoogleAction::Install
                | GoogleAction::Remove
                | GoogleAction::Enable
                | GoogleAction::Disable => {
                    static CANCEL: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    extern "C" fn cancel(_: libc::c_int) {
                        CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    unsafe {
                        libc::signal(libc::SIGINT, cancel as *const () as libc::sighandler_t);
                    }
                    store.manage_google_apps(
                        &std::env::current_exe()?,
                        match action {
                            GoogleAction::Install => glidepod_runtime::google_apps::Action::Install,
                            GoogleAction::Remove => glidepod_runtime::google_apps::Action::Remove,
                            GoogleAction::Enable => glidepod_runtime::google_apps::Action::Enable,
                            GoogleAction::Disable => glidepod_runtime::google_apps::Action::Disable,
                            _ => unreachable!(),
                        },
                        &CANCEL,
                        &mut |p| {
                            if args.json {
                                println!("{}", serde_json::to_string(&p).unwrap());
                            } else {
                                println!("{}", p.message);
                            }
                        },
                    )?;
                }
            }
            return Ok(());
        }
        Operation::Gms => {
            let report = store()?.guest()?.google_services()?;
            let summary = report.summary();
            (serde_json::to_value(report)?, summary, true)
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
            builtin_apps,
            runtime_mode,
            memory_mode,
            memory_mib,
            cpus,
            renderer,
            refresh_rate,
        } => {
            let store = store()?;
            let c = store.load()?;
            let c = store.update(c.revision, |c| {
                if let Some(mode) = builtin_apps {
                    c.minimal_builtin_apps = matches!(mode, BuiltinAppsChoice::Minimal);
                }
                if let Some(mode) = runtime_mode {
                    c.runtime_mode = match mode {
                        RuntimeModeChoice::Automatic => glidepod_core::RuntimeMode::Automatic,
                        RuntimeModeChoice::Resident => glidepod_core::RuntimeMode::Resident,
                    };
                }
                let s = c.settings_mut();
                if let Some(v) = memory_mib {
                    s.memory_mib = v;
                    s.memory_mode = glidepod_core::MemoryMode::Manual;
                }
                if let Some(mode) = memory_mode {
                    s.memory_mode = match mode {
                        MemoryModeChoice::Automatic => glidepod_core::MemoryMode::Automatic,
                        MemoryModeChoice::Manual => glidepod_core::MemoryMode::Manual,
                    };
                }
                if let Some(v) = cpus {
                    s.vcpus = v;
                }
                if let Some(v) = refresh_rate {
                    s.refresh_rate_hz = v;
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
        Operation::RegisterGpu { directory, driver } => {
            use glidepod_runtime::gpu::VulkanDriver;
            let driver = match driver {
                VulkanDriverChoice::Moltenvk => VulkanDriver::MoltenVk,
                VulkanDriverChoice::Kosmickrisp => VulkanDriver::KosmicKrisp,
            };
            let gpu = store()?.register_gpu_with_driver(&directory, driver)?;
            (serde_json::to_value(gpu)?, "Experimental GPU renderer registered. Select Metal explicitly; game compatibility remains unqualified.".into(), true)
        }
        Operation::GpuDriver { driver } => {
            if let Some(driver) = driver {
                store()?.select_gpu_driver(match driver {
                    VulkanDriverChoice::Moltenvk => glidepod_runtime::gpu::VulkanDriver::MoltenVk,
                    VulkanDriverChoice::Kosmickrisp => {
                        glidepod_runtime::gpu::VulkanDriver::KosmicKrisp
                    }
                })?;
            }
            let registry = store()?.gpu_registry()?;
            let label = registry
                .selected_driver
                .map(|d| d.label())
                .unwrap_or("None registered");
            (
                serde_json::to_value(&registry)?,
                format!("Vulkan driver: {label}. Applies to Metal rendering on the next start."),
                true,
            )
        }
        Operation::VulkanStress {
            width,
            height,
            objects,
            seconds,
            validation,
        } => {
            let report = store()?
                .guest()?
                .vulkan_stress(width, height, objects, seconds, validation)?;
            let message = format!(
                "Offscreen Vulkan · completion p95 {} ms · {} completed frames/s (not display FPS)",
                report["guest"]["completion_ms"]["p95"],
                report["guest"]["completed_frames_per_second"]
            );
            (report, message, true)
        }
        Operation::GraphicsStress {
            workload,
            width,
            height,
            seconds,
        } => {
            use glidepod_runtime::graphics_stress::{StressRequest, Workload};
            let workload = match workload {
                StressChoice::DrawCalls => Workload::DrawCalls,
                StressChoice::Fragment => Workload::Fragment,
                StressChoice::Texture => Workload::Texture,
            };
            let report = store()?.guest()?.graphics_stress(StressRequest {
                workload,
                width,
                height,
                seconds,
            })?;
            let message = format!(
                "Offscreen GLES · {} · completion p95 {} ms · {} completed frames/s (not display FPS)",
                workload.name(),
                report["guest"]["completion_ms"]["p95"],
                report["guest"]["completed_frames_per_second"]
            );
            (report, message, true)
        }
        Operation::RuntimeWorker => return glidepod_runtime::experimental::run_worker(&store()?),
        Operation::BackgroundWorker => return glidepod_runtime::background::run(store()?),
        Operation::BackgroundQuit => {
            store()?.quit_background()?;
            (
                serde_json::json!({"stopped":true}),
                "Background service stopped.".into(),
                true,
            )
        }
        Operation::BackgroundStatus => (
            serde_json::to_value(store()?.background_status()?)?,
            "Background status".into(),
            true,
        ),
        Operation::BackgroundStart => {
            let store = store()?;
            store.start_background_vm()?;
            (
                serde_json::to_value(store.background_status()?)?,
                "Android is starting in the background.".into(),
                true,
            )
        }
        Operation::BackgroundApp { package, disable } => {
            glidepod_runtime::guest::package_id(&package)?;
            let store = store()?;
            let c = store.load()?;
            let c = store.update(c.revision, |c| {
                c.background_apps.retain(|p| p != &package);
                if !disable {
                    c.background_apps.push(package);
                }
            })?;
            (
                serde_json::to_value(c)?,
                "Background app preference saved.".into(),
                true,
            )
        }
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
            let status = store()?.stop_background_vm()?;
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
            let store = store()?;
            let apps = store.cached_apps()?;
            let app = apps
                .iter()
                .find(|a| a.package_id == package)
                .ok_or_else(|| anyhow::anyhow!("Refresh the app catalog first"))?;
            let path = glidepod_macos::launchers::create_for_store(
                &app.package_id,
                &app.label,
                app.icon_path.as_deref(),
                store.root(),
            )?;
            (
                serde_json::json!({"path":path}),
                format!("Created {}", path.display()),
                true,
            )
        }
        Operation::SyncLaunchers => {
            let catalog = store()?.guest()?.desktop_catalog()?;
            let report = catalog.launchers;
            let message = report.warning().unwrap_or_else(|| {
                format!(
                    "{} Mac app entries available; {} removed.",
                    report.available, report.removed
                )
            });
            (serde_json::to_value(report)?, message, true)
        }
        Operation::Install { path } => {
            let guest = store()?.guest()?;
            let receipt = guest.install(&path, None)?;
            let catalog = guest.desktop_catalog();
            let message = format!(
                "Installed {}{}",
                receipt.package_id,
                if catalog.is_err() {
                    "; refresh the app list to load metadata"
                } else {
                    ""
                }
            );
            let mut value = serde_json::to_value(receipt)?;
            if let Ok(catalog) = catalog {
                value["mac_launchers"] = serde_json::to_value(&catalog.launchers)?;
                let message = catalog
                    .launchers
                    .warning()
                    .map(|w| format!("{message}. {w}"))
                    .unwrap_or(message);
                (value, message, true)
            } else {
                (value, message, true)
            }
        }
        Operation::Uninstall { package } => {
            let guest = store()?.guest()?;
            guest.uninstall(&package)?;
            let report = guest.desktop_catalog()?.launchers;
            let message = report
                .warning()
                .map(|w| format!("Removed {package}. {w}"))
                .unwrap_or_else(|| format!("Removed {package}"));
            (
                serde_json::json!({"removed":package,"mac_launchers":report}),
                message,
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
                for rgba in frame.pixels.as_chunks::<4>().0 {
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
