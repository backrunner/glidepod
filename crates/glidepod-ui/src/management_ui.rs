use super::*;
use anyhow::Context as _;
use glidepod_runtime::management::ClearStorage;
pub(super) fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", n as f64 / (1024. * 1024. * 1024.))
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024. * 1024.))
    } else {
        format!("{:.1} KB", n as f64 / 1024.)
    }
}
impl Center {
    pub(super) fn toggle_notifications(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        self.working = true;
        let enabled = !self.config.notifications_enabled;
        let store = self.store.clone();
        let sender = self.sender.clone();
        if enabled
            && let Err(e) =
                glidepod_macos::notifications::command(serde_json::json!({"op":"request"}))
        {
            self.working = false;
            self.notice = Some(e.to_string());
            cx.notify();
            return;
        }
        std::thread::spawn(move || {
            let action = || -> anyhow::Result<String> {
                let guest = store.guest();
                if enabled {
                    guest
                        .as_ref()
                        .map_err(|e| anyhow::anyhow!("{e:#}"))?
                        .set_notification_access(true)?;
                } else if let Ok(g) = &guest {
                    g.set_notification_access(false)?;
                }
                let save = || -> anyhow::Result<()> {
                    let c = store.load()?;
                    store.update(c.revision, |c| c.notifications_enabled = enabled)?;
                    Ok(())
                };
                if let Err(e) = save() {
                    if enabled && let Ok(g) = &guest {
                        g.set_notification_access(false).context("Could not roll back Android notification access after saving preferences failed")?;
                    }
                    return Err(e);
                }
                Ok(if enabled {
                    "Notification forwarding enabled."
                } else {
                    "Notification forwarding disabled."
                }
                .into())
            };
            let _ = sender.send(Work::Image(action().map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    pub(super) fn load_details(&mut self, package: String, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        let VmState::Running { epoch } = self.runtime.vm else {
            return;
        };
        self.selected_app = Some(package.clone());
        self.app_details = None;
        self.working = true;
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.guest().and_then(|g| g.app_details(&package));
            let _ = sender.send(Work::Details(
                package,
                epoch,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
        cx.notify();
    }
    pub(super) fn load_storage(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        let VmState::Running { epoch } = self.runtime.vm else {
            return;
        };
        self.working = true;
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.guest().and_then(|g| g.storage_overview());
            let _ = sender.send(Work::Storage(epoch, result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn mutate_details(
        &mut self,
        package: String,
        action: impl FnOnce(
            glidepod_runtime::guest::Guest,
            &str,
        ) -> anyhow::Result<glidepod_runtime::management::AppDetails>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let VmState::Running { epoch } = self.runtime.vm else {
            return;
        };
        self.working = true;
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.guest().and_then(|g| action(g, &package));
            let _ = sender.send(Work::Details(
                package,
                epoch,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
        cx.notify();
    }
    fn close_managed_window(&mut self, package: &str, cx: &mut Context<Self>) {
        if let Some(w) = self.windows.remove(package) {
            let _ = w.update(cx, |_, w, _| w.remove_window());
        }
    }
    pub(super) fn clear_storage(
        &mut self,
        package: String,
        kind: ClearStorage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.working {
            return;
        }
        if matches!(kind, ClearStorage::Cache) {
            self.mutate_details(package, move |g, p| g.clear_app_storage(p, kind), cx);
            return;
        }
        let label = self
            .app_details
            .as_ref()
            .map(|d| d.label.as_str())
            .unwrap_or(&package);
        let prompt=window.prompt(PromptLevel::Warning,&format!("Clear all data for {label}?"),Some("This permanently removes the app's Android files, accounts and settings. The app stays installed."),&["Clear Data","Cancel"],cx);
        cx.spawn(async move |this, cx| {
            if prompt.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    this.close_managed_window(&package, cx);
                    this.mutate_details(package, move |g, p| g.clear_app_storage(p, kind), cx);
                });
            }
        })
        .detach();
    }
    pub(super) fn change_permission(
        &mut self,
        package: String,
        permission: String,
        grant: bool,
        cx: &mut Context<Self>,
    ) {
        if self.working {
            return;
        }
        if !grant {
            self.close_managed_window(&package, cx);
        }
        self.mutate_details(
            package,
            move |g, p| g.set_permission(p, &permission, grant),
            cx,
        );
    }
    pub(super) fn android_settings(&mut self, package: String, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        self.working = true;
        let existing = self.windows.get("com.android.settings").and_then(|h| {
            h.update(cx, |v, w, _| {
                w.activate_window();
                v.display_id
            })
            .ok()
        });
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            if let Some(display) = existing {
                let r = store
                    .guest()
                    .and_then(|g| g.open_android_app_settings(&package, display))
                    .map(|_| "Android Settings opened.".into());
                let _ = sender.send(Work::Image(r.map_err(|e| format!("{e:#}"))));
            } else {
                let r = store.guest().and_then(|g| {
                    let w = g.open_app("com.android.settings", 720, 1080)?;
                    g.open_android_app_settings(&package, w.info.display_id)?;
                    Ok(w)
                });
                let _ = sender.send(Work::Opened(
                    "com.android.settings".into(),
                    "Android Settings".into(),
                    r.map_err(|e| format!("{e:#}")),
                ));
            }
        });
        cx.notify();
    }
    pub(super) fn run_benchmark(
        &mut self,
        api: glidepod_runtime::benchmark::GraphicsApi,
        gles_version: u32,
        cx: &mut Context<Self>,
    ) {
        if self.working {
            return;
        }
        self.working = true;
        self.benchmark = None;
        self.notice = Some("Running the Android 3D check…".into());
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let r = store.guest().and_then(|g| {
                g.benchmark(glidepod_runtime::benchmark::BenchmarkRequest {
                    api,
                    gles_version,
                    ..Default::default()
                })
            });
            let _ = sender.send(Work::Benchmark(
                r.map(Box::new).map_err(|e| format!("{e:#}")),
            ));
        });
        cx.notify();
    }
}
