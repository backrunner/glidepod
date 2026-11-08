//! Window-independent monitoring. Disk/socket reads and process sampling stay off AppKit.
use crate::{AppSession, open_center};
use glidepod_core::VmState;
use glidepod_macos::{
    process_usage::{Sampler, Usage},
    tray::{Action, AppEntry, Snapshot, Tray},
};
use glidepod_runtime::{Store, experimental::RuntimeStatus};
use gpui::App;
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct OpenApp {
    pub package_id: String,
    pub title: String,
    pub icon_path: Option<std::path::PathBuf>,
    pub epoch: u64,
}

pub(crate) struct Service {
    native: Tray,
    windows: Arc<Mutex<Vec<OpenApp>>>,
    stop: mpsc::Sender<()>,
}

impl Service {
    pub fn show(&self) {
        self.native.show();
    }

    pub fn new(store: Store) -> anyhow::Result<Self> {
        let native = Tray::new()?;
        let windows = Arc::new(Mutex::new(Vec::new()));
        let observed = windows.clone();
        let (stop, stopped) = mpsc::channel();
        std::thread::Builder::new()
            .name("glidepod-tray".into())
            .spawn(move || {
                let mut sampler = Sampler::default();
                loop {
                    let snapshot = match store.runtime_status() {
                        Ok(status) => {
                            let usage = if status.active() {
                                status.engine_pid.and_then(|pid| {
                                    let runtime = store.experimental_runtime().ok().flatten()?;
                                    sampler.sample(pid, &runtime.engine).ok()
                                })
                            } else {
                                None
                            };
                            if usage.is_none() {
                                sampler.reset();
                            }
                            snapshot(&status, &observed.lock().unwrap(), usage.as_ref())
                        }
                        Err(error) => {
                            sampler.reset();
                            Snapshot {
                                state: "Status unavailable".into(),
                                detail: format!("{error:#}"),
                                cpu: "—".into(),
                                memory: "—".into(),
                                allocation: "—".into(),
                                app_count: None,
                                apps: Vec::new(),
                                valid_for_seconds: 5,
                            }
                        }
                    };
                    glidepod_macos::tray::publish(&snapshot);
                    if !matches!(
                        stopped.recv_timeout(Duration::from_secs(1)),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
            })?;
        Ok(Self {
            native,
            windows,
            stop,
        })
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

fn memory(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024. * 1024. * 1024.))
    } else {
        format!("{:.0} MiB", bytes as f64 / (1024. * 1024.))
    }
}

fn snapshot(status: &RuntimeStatus, windows: &[OpenApp], usage: Option<&Usage>) -> Snapshot {
    let mut apps: Vec<_> = windows
        .iter()
        .filter(|app| matches!(status.vm, VmState::Running { epoch } if epoch == app.epoch))
        .map(|app| AppEntry {
            package_id: app.package_id.clone(),
            title: app.title.clone(),
            icon_path: app.icon_path.clone(),
        })
        .collect();
    apps.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then(a.package_id.cmp(&b.package_id))
    });
    let usage = usage.filter(|_| status.active());
    Snapshot {
        state: status.label().into(),
        detail: status.message.clone(),
        cpu: usage
            .map(|u| {
                u.cpu_percent
                    .map(|p| format!("{p:.1}%"))
                    .unwrap_or_else(|| "Measuring…".into())
            })
            .unwrap_or_else(|| "—".into()),
        memory: usage
            .map(|u| memory(u.footprint_bytes))
            .unwrap_or_else(|| "—".into()),
        allocation: status
            .resources
            .as_ref()
            .filter(|_| status.active())
            .map(|r| {
                format!(
                    "{} vCPUs · {}",
                    r.vcpus,
                    memory(u64::from(r.memory_mib) * 1024 * 1024)
                )
            })
            .unwrap_or_else(|| "—".into()),
        app_count: Some(apps.len()),
        apps,
        valid_for_seconds: 5,
    }
}

/// Called from the application's existing timer, even when the center is closed.
pub(crate) fn poll(cx: &mut App) {
    let handles = cx.global::<AppSession>().windows.clone();
    let windows = handles
        .iter()
        .filter_map(|(package, handle)| {
            handle.read(cx).ok().map(|window| OpenApp {
                package_id: package.clone(),
                title: window.title.clone(),
                icon_path: window.icon_path.clone(),
                epoch: window.epoch,
            })
        })
        .collect();
    let service = &cx.global::<AppSession>().tray;
    *service.windows.lock().unwrap() = windows;
    let actions = service.native.drain();
    for action in actions {
        match action {
            Action::OpenCenter => {
                let _ = open_center(cx);
                cx.activate(true);
            }
            Action::FocusApp { package_id } => {
                if let Some(handle) = handles.get(&package_id) {
                    let _ = handle.update(cx, |_, window, cx| {
                        cx.activate(true);
                        window.activate_window();
                    });
                }
            }
            Action::Quit => {
                crate::quit_glidepod(cx);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires GLIDEPOD_TEST_STORE pointing to a running disposable Android VM"]
    fn live_vm_process_metrics() {
        let store = Store::new(
            std::env::var_os("GLIDEPOD_TEST_STORE")
                .expect("Disposable store")
                .into(),
        )
        .unwrap();
        let status = store.runtime_status().unwrap();
        assert!(matches!(status.vm, VmState::Running { .. }));
        let runtime = store.experimental_runtime().unwrap().unwrap();
        let pid = status.engine_pid.unwrap();
        let mut sampler = Sampler::default();
        assert!(
            sampler
                .sample(pid, &runtime.engine)
                .unwrap()
                .cpu_percent
                .is_none()
        );
        std::thread::sleep(Duration::from_millis(250));
        let usage = sampler.sample(pid, &runtime.engine).unwrap();
        assert!(usage.cpu_percent.is_some_and(|p| p.is_finite() && p >= 0.));
        assert!(usage.footprint_bytes > 0);
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "engine_pid": pid, "usage": usage, "snapshot": snapshot(&status, &[], Some(&usage))
            }))
            .unwrap()
        );
    }

    #[test]
    fn counts_current_vm_windows_and_clears_stopped_measurements() {
        let mut status = RuntimeStatus {
            vm: VmState::Running { epoch: 2 },
            configured: true,
            profile: None,
            effective_root: false,
            effective_renderer: None,
            vulkan_driver: None,
            effective_network: None,
            resources: None,
            fingerprint: None,
            engine_pid: Some(100),
            endpoints: None,
            message: "Ready".into(),
        };
        let windows = vec![
            OpenApp {
                package_id: "test.old".into(),
                title: "Old".into(),
                icon_path: None,
                epoch: 1,
            },
            OpenApp {
                package_id: "test.b".into(),
                title: "Zebra".into(),
                icon_path: None,
                epoch: 2,
            },
            OpenApp {
                package_id: "test.a".into(),
                title: "Clock".into(),
                icon_path: Some("/tmp/clock.png".into()),
                epoch: 2,
            },
        ];
        let usage = Usage {
            cpu_percent: Some(125.5),
            footprint_bytes: 1536 * 1024 * 1024,
            resident_bytes: 2048 * 1024 * 1024,
        };
        let live = snapshot(&status, &windows, Some(&usage));
        assert_eq!(live.app_count, Some(2));
        assert_eq!(live.apps[0].title, "Clock");
        assert_eq!(
            live.apps[0].icon_path.as_deref(),
            Some(std::path::Path::new("/tmp/clock.png"))
        );
        assert_eq!(live.cpu, "125.5%");
        assert_eq!(live.memory, "1.50 GiB");
        assert_eq!(snapshot(&status, &windows, None).cpu, "—");
        for state in [
            VmState::Stopped,
            VmState::MissingImage,
            VmState::Failed {
                message: "Lost".into(),
            },
        ] {
            status.vm = state;
            let idle = snapshot(&status, &windows, Some(&usage));
            assert_eq!(idle.app_count, Some(0));
            assert!(idle.apps.is_empty());
            assert_eq!(idle.cpu, "—");
            assert_eq!(idle.memory, "—");
        }
    }
}
