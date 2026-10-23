//! Compact macOS management views. Runtime capabilities are never inferred from preferences.
use super::*;
use gpui_component::TitleBar;

impl Center {
    fn caption(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(11.))
            .text_color(rgb(palette(self.config.dark_mode).muted))
            .child(text.into())
    }

    fn group(&self) -> Div {
        let p = palette(self.config.dark_mode);
        div()
            .w_full()
            .bg(rgb(p.panel))
            .border_1()
            .border_color(rgb(p.line))
            .rounded(px(9.))
            .overflow_hidden()
    }

    fn section(&self, title: &'static str, content: impl IntoElement) -> Div {
        div()
            .mb(px(22.))
            .child(
                div()
                    .mb(px(9.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(12.))
                    .child(title),
            )
            .child(content)
    }

    fn row(&self, label: &'static str, control: impl IntoElement) -> Div {
        div()
            .min_h(px(46.))
            .px(px(14.))
            .py(px(10.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(20.))
            .child(div().flex_shrink_0().child(label))
            .child(control)
    }

    fn divider(&self) -> Div {
        div()
            .mx(px(14.))
            .h(px(1.))
            .bg(rgb(palette(self.config.dark_mode).line))
    }

    fn empty(&self, icon: &str, title: &'static str, detail: &'static str) -> Div {
        let p = palette(self.config.dark_mode);
        div()
            .flex_1()
            .min_h(px(280.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .child(
                symbol(icon)
                    .size(px(34.))
                    .text_color(rgb(p.muted))
                    .mb(px(4.)),
            )
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(self.caption(detail))
    }

    fn apps(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.selected_app.is_some() {
            return self.app_information(cx);
        }
        let query = self.query.to_lowercase();
        let mut apps = self
            .apps
            .iter()
            .filter(|a| {
                (a.label.to_lowercase().contains(&query) || a.package_id.contains(&query))
                    && (self.filter != 1 || a.game)
                    && (self.page != Page::Running || self.windows.contains_key(&a.package_id))
            })
            .collect::<Vec<_>>();
        if self.filter == 2 {
            apps.sort_by_key(|a| std::cmp::Reverse(a.updated));
        }
        let ready = matches!(self.runtime.vm, VmState::Running { .. });
        let filters = div().flex().gap(px(4.)).children(
            ["All apps", "Games", "Updated"]
                .into_iter()
                .enumerate()
                .map(|(i, label)| {
                    Button::new(("filter", i))
                        .label(label)
                        .small()
                        .ghost()
                        .when(self.filter == i, |b| {
                            b.bg(rgb(palette(self.config.dark_mode).selected))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.filter = i;
                            cx.notify();
                        }))
                }),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(filters)
                    .child(
                        button("reload-apps", "Refresh")
                            .disabled(!ready || self.working)
                            .on_click(cx.listener(|this, _, _, cx| this.load_apps(cx))),
                    ),
            )
            .child(self.caption(format!(
                "{} apps{}",
                apps.len(),
                if ready { "" } else { " · Android is stopped" }
            )))
            .when(apps.is_empty(), |d| {
                d.child(self.empty(
                    "apps",
                    "No apps to show",
                    "Start Android, then add an APK or refresh the app list.",
                ))
            })
            .children(apps.into_iter().enumerate().map(|(i, app)| {
                let package = app.package_id.clone();
                let info = package.clone();
                let remove = package.clone();
                let close = package.clone();
                let launcher = app.clone();
                let running = self.windows.contains_key(&package);
                self.group().child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .p(px(12.))
                        .children(
                            app.icon_path
                                .clone()
                                .map(|path| img(path).size(px(36.)).rounded(px(7.))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(app.label.clone()),
                                )
                                .child(self.caption(app.package_id.clone())),
                        )
                        .child(
                            Button::new(("open-app", i))
                                .label(if running { "Show" } else { "Open" })
                                .small()
                                .disabled(!ready || self.working)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_app(package.clone(), cx)
                                })),
                        )
                        .when(running, |d| {
                            d.child(
                                Button::new(("close-app", i))
                                    .label("Close")
                                    .small()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(handle) = this.windows.remove(&close) {
                                            let _ = handle.update(cx, |_, w, _| w.remove_window());
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            Button::new(("launcher", i))
                                .label("Add to Mac")
                                .small()
                                .disabled(self.working)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.add_launcher(launcher.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new(("app-info", i))
                                .label("Info")
                                .small()
                                .disabled(!ready || self.working)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.load_details(info.clone(), cx)
                                })),
                        )
                        .when(!app.system, |d| {
                            d.child(
                                Button::new(("remove-app", i))
                                    .label("Remove…")
                                    .small()
                                    .ghost()
                                    .disabled(!ready || self.working)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.remove_app(remove.clone(), window, cx)
                                    })),
                            )
                        }),
                )
            }))
            .into_any_element()
    }

    fn app_information(&self, cx: &mut Context<Self>) -> AnyElement {
        let package = self.selected_app.clone().unwrap();
        let refresh = package.clone();
        let settings = package.clone();
        let ready = matches!(self.runtime.vm, VmState::Running { .. }) && !self.working;
        let heading = div()
            .flex()
            .justify_between()
            .mb(px(18.))
            .child(
                button("back-apps", "‹ Apps")
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selected_app = None;
                        this.app_details = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(button("refresh-info", "Refresh").disabled(!ready).on_click(
                        cx.listener(move |this, _, _, cx| this.load_details(refresh.clone(), cx)),
                    ))
                    .child(
                        button("android-app-settings", "Android Settings…")
                            .disabled(!ready)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.android_settings(settings.clone(), cx)
                            })),
                    ),
            );
        let mut content = div().w_full().max_w(px(680.)).child(heading);
        let Some(d) = self.app_details.as_ref() else {
            return content
                .child(self.caption(if self.working {
                    "Loading app information…"
                } else {
                    "Start Android and refresh app information."
                }))
                .into_any_element();
        };
        let mutable = ready
            && !d.system
            && !d.shared_uid
            && !matches!(
                package.as_str(),
                "dev.glidepod.input" | "dev.glidepod.widgets" | "dev.glidepod.notifications"
            );
        content = content
            .child(
                div()
                    .text_size(px(19.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(d.label.clone()),
            )
            .child(
                self.caption(format!("{} · {}", d.package_id, d.version))
                    .mb(px(22.)),
            );
        let mut storage = self.group();
        if let Some(s) = &d.storage {
            for (label, value) in [
                ("App", s.app_bytes),
                ("User data", s.user_data_bytes()),
                ("Cache", s.cache_bytes),
                ("Total", s.total_bytes()),
            ] {
                storage = storage
                    .child(self.row(label, self.caption(super::management_ui::bytes(value))))
                    .child(self.divider());
            }
        } else {
            storage = storage.child(
                self.caption(
                    d.storage_error
                        .clone()
                        .unwrap_or_else(|| "Storage statistics unavailable".into()),
                )
                .p(px(14.)),
            );
        }
        let cache = package.clone();
        let data = package.clone();
        storage = storage.child(
            div()
                .p(px(12.))
                .flex()
                .gap(px(8.))
                .child(
                    button("clear-app-cache", "Clear Cache")
                        .disabled(!mutable)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.clear_storage(
                                cache.clone(),
                                glidepod_runtime::management::ClearStorage::Cache,
                                w,
                                cx,
                            )
                        })),
                )
                .child(
                    button("clear-app-data", "Clear Data…")
                        .disabled(!mutable)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.clear_storage(
                                data.clone(),
                                glidepod_runtime::management::ClearStorage::Data,
                                w,
                                cx,
                            )
                        })),
                ),
        );
        content=content.child(self.section("Storage",storage)).child(self.caption("Android-reported sizes. Clearing files does not shrink the Mac VM disk automatically.").mt(px(-12.)).mb(px(20.)));
        let permissions = self
            .group()
            .children(d.permissions.iter().enumerate().map(|(i, p)| {
                let package = package.clone();
                let permission = p.name.clone();
                let grant = !p.granted;
                div().when(i > 0, |v| v.child(self.divider())).child(
                    div()
                        .p(px(14.))
                        .flex()
                        .gap(px(12.))
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .child(p.label.clone())
                                .when(p.fixed, |v| v.child(self.caption("Managed by Android"))),
                        )
                        .child(
                            Button::new(("permission", i))
                                .label(if p.granted { "Allowed" } else { "Not allowed" })
                                .small()
                                .disabled(!mutable || p.fixed)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.change_permission(
                                        package.clone(),
                                        permission.clone(),
                                        grant,
                                        cx,
                                    )
                                })),
                        ),
                )
            }))
            .when(d.permissions.is_empty(), |v| {
                v.child(self.caption("No runtime permissions requested.").p(px(14.)))
            });
        content.child(self.section("Permissions",permissions))
            .child(self.caption("Changes apply inside Android. Camera, microphone and location are not available in this build. Use Android Settings for notification channels and special access."))
            .when(d.shared_uid,|v|v.child(self.caption("This app shares its Android identity. Sizes may include other packages; manage it in Android Settings.").mt(px(12.))))
            .into_any_element()
    }

    fn android(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(self.config.dark_mode);
        let settings = self.config.settings();
        let profiles = div().flex().gap(px(3.)).children(
            [ProfileId::Personal, ProfileId::Developer]
                .into_iter()
                .enumerate()
                .map(|(i, profile)| {
                    Button::new(("profile", i))
                        .label(profile.label())
                        .small()
                        .disabled(self.runtime.active() || self.working)
                        .when(self.config.active_profile == profile, |b| b.primary())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_settings(|c| c.active_profile = profile, cx)
                        }))
                }),
        );
        let memory = div()
            .flex()
            .gap(px(3.))
            .children([2048, 4096, 6144, 8192].into_iter().map(|m| {
                Button::new(("memory", m as usize))
                    .label(format!("{} GB", m / 1024))
                    .small()
                    .ghost()
                    .when(settings.memory_mib == m, |b| b.bg(rgb(p.selected)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_settings(|c| c.settings_mut().memory_mib = m, cx)
                    }))
            }));
        let cpus = div()
            .flex()
            .gap(px(3.))
            .children([2, 4, 6, 8].into_iter().map(|n| {
                Button::new(("cpus", n as usize))
                    .label(n.to_string())
                    .small()
                    .ghost()
                    .when(settings.vcpus == n, |b| b.bg(rgb(p.selected)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_settings(|c| c.settings_mut().vcpus = n, cx)
                    }))
            }));
        let runtime = self
            .group()
            .child(
                self.row(
                    "Status",
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(self.caption(self.runtime.label()))
                        .child(if self.runtime.active() {
                            button("stop-android", "Stop Android")
                                .disabled(
                                    self.working
                                        || matches!(self.runtime.vm, VmState::Stopping { .. }),
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.stop_android(cx)))
                        } else {
                            button("start-android", "Start Android")
                                .primary()
                                .disabled(
                                    self.working
                                        || !self.runtime.configured
                                        || self.config.active_profile != ProfileId::Developer,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.start_android(cx)))
                        }),
                ),
            )
            .child(self.divider())
            .child(
                self.row(
                    "Runtime",
                    button(
                        "register-runtime",
                        if self.runtime.configured {
                            "Replace Runtime…"
                        } else {
                            "Register Runtime…"
                        },
                    )
                    .disabled(self.working || self.runtime.active())
                    .on_click(cx.listener(|this, _, _, cx| this.choose_runtime(cx))),
                ),
            )
            .child(self.divider())
            .child(
                self.row(
                    "Image",
                    button("verify-image", "Verify Image…")
                        .disabled(self.working)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_image(cx))),
                ),
            );
        let resources = self
            .group()
            .child(self.row("Profile", profiles))
            .child(self.divider())
            .child(self.row("Memory", memory))
            .child(self.divider())
            .child(self.row("Processors", cpus));
        div()
            .w_full()
            .max_w(px(680.))
            .child(self.section("Runtime", runtime))
            .child(
                self.caption(self.runtime.message.clone())
                    .mt(px(-12.))
                    .mb(px(24.)),
            )
            .child(self.section("Storage",self.group()
                .child(self.row("Android data volume",button("refresh-storage","Refresh").disabled(self.working || !matches!(self.runtime.vm,VmState::Running{..})).on_click(cx.listener(|this,_,_,cx|this.load_storage(cx)))))
                .children(self.storage.as_ref().map(|s|self.caption(format!("{} available of {}",super::management_ui::bytes(s.available_bytes),super::management_ui::bytes(s.total_bytes))).px(px(14.)).pb(px(12.))))))
            .child(self.section("Resources", resources))
            .child(
                self.caption("Saved separately for each profile. Changes apply at the next start.")
                    .mt(px(-12.)),
            )
            .child(
                div().mt(px(24.)).child(
                    self.section(
                        "Developer access",
                        self.group()
                            .child(self.row(
                                "Guest root",
                                self.caption(if self.runtime.effective_root {
                                    "Verified · userdebug console"
                                } else if self.runtime.active() {
                                    "Waiting for verification"
                                } else {
                                    "Built into the experimental image"
                                }),
                            ))
                            .child(self.divider())
                            .child(self.row("CLI root shortcut", button("root-commands", if self.config.developer.root_requested {"Enabled"} else {"Disabled"}).disabled(self.config.active_profile != ProfileId::Developer).on_click(cx.listener(|this,_,_,cx|this.update_settings(|c|c.developer.root_requested = !c.developer.root_requested,cx)))))
                            .child(self.divider())
                            .child(self.row(
                                "Renderer",
                                self.caption(match self.runtime.effective_renderer { Some(Renderer::Metal) => "Metal · experimental gfxstream", Some(_) => "Software · virtio-gpu 2D", None => "Not running" }),
                            )),
                    ),
                ),
            )
            .child(
                self.caption(
                    "Root commands apply to the guest only. Disabling commands does not remove su from this userdebug image.",
                )
                .mt(px(-12.)),
            )
            .into_any_element()
    }

    fn graphics(&self, cx: &mut Context<Self>) -> AnyElement {
        let device = self.host.metal_device.as_deref().unwrap_or("Unavailable");
        let host = self
            .group()
            .child(self.row("GPU", self.caption(device.to_owned())))
            .child(self.divider())
            .child(self.row(
                "Hardware virtualization",
                self.caption(if self.host.hypervisor_supported {
                    "Available"
                } else {
                    "Unavailable"
                }),
            ))
            .child(self.divider())
            .child(
                self.row(
                    "Host checks",
                    button(
                        "probe-host",
                        if self.working {
                            "Checking…"
                        } else {
                            "Run Checks"
                        },
                    )
                    .disabled(self.working)
                    .on_click(cx.listener(|this, _, _, cx| this.probe(cx))),
                ),
            );
        let renderers = div().flex().gap(px(3.)).children(
            [
                (Renderer::Automatic, "Automatic"),
                (Renderer::Metal, "Metal"),
                (Renderer::Software, "Software"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (renderer, label))| {
                Button::new(("renderer", i))
                    .label(label)
                    .small()
                    .when(self.config.settings().renderer == renderer, |b| b.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_settings(|c| c.settings_mut().renderer = renderer, cx)
                    }))
            }),
        );
        let results = self.probes.as_ref().map(|report| {
            let rows = report.probes.iter().enumerate().map(|(i, result)| {
                div()
                    .when(i > 0, |d| d.child(self.divider()))
                    .child(
                        self.row(
                            if result.name == "hvf_arm64_execution" {
                                "ARM64 execution"
                            } else {
                                "Metal compute"
                            },
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    symbol(if result.passed { "check" } else { "close" })
                                        .size(px(13.))
                                        .text_color(rgb(palette(self.config.dark_mode).accent)),
                                )
                                .child(if result.passed { "Passed" } else { "Failed" }),
                        ),
                    )
                    .when(self.show_diagnostics, |d| {
                        d.child(self.caption(result.detail.clone()).px(px(14.)).pb(px(14.)))
                    })
            });
            self.section(
                "Results",
                div().child(self.group().children(rows)).child(
                    button(
                        "diagnostic-details",
                        if self.show_diagnostics {
                            "Hide Details"
                        } else {
                            "Show Details"
                        },
                    )
                    .ghost()
                    .mt(px(6.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_diagnostics = !this.show_diagnostics;
                        cx.notify();
                    })),
                ),
            )
        });
        div().w_full().max_w(px(680.))
            .child(self.section("This Mac", host)).children(results)
            .child(self.section("Android 3D check",self.group()
                .child(self.row("Textured 3D · 720 × 1080",div().flex().gap(px(8.))
                    .child(button("benchmark-3d","Run 3D Check").disabled(self.working || !matches!(self.runtime.vm,VmState::Running{..})).on_click(cx.listener(|this,_,_,cx|this.run_benchmark(glidepod_runtime::benchmark::GraphicsApi::Gles,2,cx))))
                    .child(button("benchmark-gles3","Check GLES 3").disabled(self.working || !matches!(self.runtime.vm,VmState::Running{..})).on_click(cx.listener(|this,_,_,cx|this.run_benchmark(glidepod_runtime::benchmark::GraphicsApi::Gles,3,cx))))))
                .child(self.row("Direct Vulkan",button("benchmark-vulkan","Check Vulkan").disabled(self.working || !matches!(self.runtime.vm,VmState::Running{..})).on_click(cx.listener(|this,_,_,cx|this.run_benchmark(glidepod_runtime::benchmark::GraphicsApi::Vulkan,2,cx)))))
                .children(self.benchmark.as_ref().map(|r|div().px(px(14.)).pb(px(12.))
                    .child(self.caption(format!("Last check · {:?} · {:?} · {:.1} app frames/s · interval p95 {:.1} ms",r.request.api,r.renderer,r.mean_fps,r.frame_interval.p95_ms)))
                    .child(self.caption(format!("{} objects · {} samples · {} × {}",r.request.objects,r.request.samples,r.request.width,r.request.height)))
                    .child(self.caption(if r.request.api == glidepod_runtime::benchmark::GraphicsApi::Vulkan {"Vulkan pixels, compute, synchronization and presentation checks passed."} else if r.guest["gles3_probe"]["correctness"]["passed"] == true {"GLES 3 pixel and readback checks passed."} else if r.guest["gles3_probe"]["context_created"] == true {"GLES 3 is available; correctness checks have not passed."} else {"GLES 3 is unavailable on this renderer."}))
                    .child(self.caption("App rendering and copied frames only; Mac display smoothness and game compatibility are not measured."))))))
            .child(self.section("Experimental GPU",self.group().child(self.row("External renderer",button("register-gpu","Register Renderer…").disabled(self.working || self.runtime.active()).on_click(cx.listener(|this,_,_,cx|this.choose_gpu(cx)))))))
            .child(self.section("Android rendering", self.group().child(self.row("Preference", renderers))))
            .child(self.caption("Automatic uses Software. Metal requires a registered external renderer and applies on restart. Check GLES 3 and Vulkan independently; game compatibility is pending.").mt(px(-12.)))
            .into_any_element()
    }

    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let appearance = div().flex().gap(px(3.)).children(
            [(false, "Light"), (true, "Dark")]
                .into_iter()
                .enumerate()
                .map(|(i, (dark, label))| {
                    Button::new(("appearance", i))
                        .label(label)
                        .small()
                        .when(self.config.dark_mode == dark, |b| b.primary())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_settings(|c| c.dark_mode = dark, cx);
                            apply_theme(this.config.dark_mode, cx);
                        }))
                }),
        );
        let general = self
            .group()
            .child(self.row("Appearance", appearance))
            .child(self.divider())
            .child(
                self.row(
                    "Android audio · next start",
                    button(
                        "audio-output",
                        if self.config.audio_enabled {
                            "On"
                        } else {
                            "Off"
                        },
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_settings(|c| c.audio_enabled = !c.audio_enabled, cx)
                    })),
                ),
            )
            .child(self.divider())
            .child(
                self.row(
                    "Local data",
                    button("show-data", "Show in Finder")
                        .on_click(cx.listener(|this, _, _, cx| cx.reveal_path(this.store.root()))),
                ),
            )
            .child(self.divider())
            .child(self.row(
                "Preferences",
                button("refresh-settings", "Reload").on_click(
                    cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                ),
            ));
        div()
            .w_full()
            .max_w(px(680.))
            .child(self.section("General", general))
            .child(self.section("Notifications",self.group()
                .child(self.row("Android app notifications",button("notification-forwarding",if self.config.notifications_enabled {"On"}else{"Off"})
                    .disabled(self.working || (!self.config.notifications_enabled && !matches!(self.runtime.vm,VmState::Running{..})))
                    .on_click(cx.listener(|this,_,_,cx|this.toggle_notifications(cx)))))
                .child(self.caption(cx.global::<AppSession>().notifications.status.lock().unwrap().clone()).px(px(14.)).pb(px(12.)))
                .child(self.caption("Uses Android notification access and macOS notification permission. Private content stays hidden; click opens the app, dismissal clears dismissible Android notifications.").px(px(14.)).pb(px(12.)))))
            .child(
                self.section(
                    "About",
                    self.group()
                        .child(self.row("GlidePod", self.caption("0.1.0 · Development")))
                        .child(self.divider())
                        .child(self.row("License", self.caption("Apache-2.0"))),
                ),
            )
            .child(self.caption("Use Add to Mac in Apps to create launch entries. Native WidgetKit widgets are not available yet."))
            .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let p = palette(self.config.dark_mode);
        let rows = PAGES
            .into_iter()
            .filter(|page| *page != Page::Settings)
            .enumerate()
            .map(|(i, page)| {
                Button::new(("nav", i))
                    .label(page.label())
                    .icon(Icon::default().path(format!("icons/{}.svg", page.icon())))
                    .ghost()
                    .w_full()
                    .h(px(32.))
                    .justify_start()
                    .px(px(9.))
                    .rounded(px(6.))
                    .when(self.page == page, |b| b.bg(rgb(p.selected)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.page = page;
                        window.focus(&this.focus);
                        cx.notify();
                    }))
            });
        div()
            .w(px(184.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(p.side))
            .border_r_1()
            .border_color(rgb(p.line))
            .flex()
            .flex_col()
            .p(px(10.))
            .child(self.caption("Library").px(px(9.)).py(px(8.)))
            .child(div().flex().flex_col().gap(px(3.)).children(rows))
            .child(div().flex_1())
            .child(
                Button::new("settings-nav")
                    .label("Settings")
                    .icon(Icon::default().path("icons/settings.svg"))
                    .ghost()
                    .w_full()
                    .h(px(32.))
                    .justify_start()
                    .px(px(9.))
                    .rounded(px(6.))
                    .when(self.page == Page::Settings, |b| b.bg(rgb(p.selected)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.page = Page::Settings;
                        window.focus(&this.focus);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .mt(px(12.))
                    .pt(px(12.))
                    .px(px(8.))
                    .border_t_1()
                    .border_color(rgb(p.line))
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(self.config.active_profile.label()),
                    )
                    .child(
                        self.caption(format!("Android {}", self.runtime.label().to_lowercase()))
                            .mt(px(4.))
                            .mb(px(4.)),
                    ),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> TitleBar {
        let p = palette(self.config.dark_mode);
        let search = div().w(px(208.)).h(px(28.)).child(
            Input::new(&self.search)
                .small()
                .min_h(px(28.))
                .cleanable(true)
                .prefix(
                    Icon::default()
                        .path("icons/search.svg")
                        .text_color(rgb(p.muted)),
                )
                .suffix(self.caption("⌘F")),
        );
        TitleBar::new()
            .h(px(52.))
            .bg(rgb(p.side))
            .border_color(rgb(p.line))
            .pr(px(16.))
            .child(div().w(px(104.)).flex_shrink_0())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(14.))
                            .child(self.page.label()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .when(self.page == Page::Apps, |d| d.child(search))
                            .child(
                                button("open-apk", "Add APK…")
                                    .icon(Icon::default().path("icons/plus.svg"))
                                    .disabled(self.working || self.apk.is_some())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_apk(&ChooseApk, window, cx)
                                    })),
                            ),
                    ),
            )
    }

    fn apk_sheet(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(self.config.dark_mode);
        let report = self.apk.as_ref().unwrap();
        let size = if report.bytes < 1024 * 1024 {
            format!("{:.1} KB", report.bytes as f64 / 1024.)
        } else {
            format!("{:.1} MB", report.bytes as f64 / 1_048_576.)
        };
        let metadata = self
            .group()
            .child(
                self.row(
                    "Version",
                    self.caption(
                        report
                            .manifest
                            .version_name
                            .clone()
                            .unwrap_or_else(|| "Not declared".into()),
                    ),
                ),
            )
            .child(self.divider())
            .child(self.row(
                "Architecture",
                self.caption(if report.native_abis.is_empty() {
                    "No native libraries".into()
                } else {
                    report.native_abis.join(", ")
                }),
            ))
            .child(self.divider())
            .child(
                self.row(
                    "Minimum Android API",
                    self.caption(
                        report
                            .manifest
                            .min_sdk
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "Not declared".into()),
                    ),
                ),
            )
            .child(self.divider())
            .child(self.row("Size", self.caption(size)));
        let buttons = div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .mt(px(18.))
            .child(
                button("close-apk", "Cancel")
                    .disabled(self.working)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apk = None;
                        window.focus(&this.focus);
                        cx.notify();
                    })),
            )
            .child(
                button(
                    "apk-setup",
                    if self.working {
                        "Installing…"
                    } else if matches!(self.runtime.vm, VmState::Running { .. }) {
                        "Install"
                    } else {
                        "Set Up Android…"
                    },
                )
                .disabled(self.working || !report.arm64_candidate)
                .primary()
                .on_click(cx.listener(|this, _, window, cx| {
                    if matches!(this.runtime.vm, VmState::Running { .. }) {
                        this.install_apk(cx);
                        return;
                    }
                    this.apk = None;
                    this.page = Page::Android;
                    window.focus(&this.focus);
                    cx.notify();
                })),
            );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000032))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("apk-review")
                    .w(px(460.))
                    .max_h(px(550.))
                    .overflow_y_scroll()
                    .p(px(24.))
                    .rounded(px(12.))
                    .bg(rgb(p.bg))
                    .border_1()
                    .border_color(rgb(p.line))
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(symbol("file").size(px(28.)).text_color(rgb(p.accent)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(report.filename.clone()),
                                    )
                                    .child(
                                        self.caption(report.manifest.package_id.clone()).mt(px(5.)),
                                    ),
                            ),
                    )
                    .child(div().mt(px(20.)).child(metadata))
                    .child(
                        self.caption("Android will verify the APK signature during installation.")
                            .mt(px(12.)),
                    )
                    .when(!report.arm64_candidate, |d| {
                        d.child(
                            self.caption("This APK has no ARM64 native libraries.")
                                .mt(px(8.)),
                        )
                    })
                    .child(
                        self.caption(
                            "Updates preserve app data. Android rejects incompatible signatures.",
                        )
                        .mt(px(14.)),
                    )
                    .children(
                        self.notice
                            .as_ref()
                            .map(|message| self.caption(message.clone()).mt(px(10.))),
                    )
                    .child(buttons),
            )
            .into_any_element()
    }
}

impl Render for Center {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if (self.page != Page::Apps || self.apk.is_some())
            && self.search.focus_handle(cx).is_focused(window)
        {
            window.focus(&self.focus);
        }
        let p = palette(self.config.dark_mode);
        let content = match self.page {
            Page::Apps => self.apps(cx),
            Page::Android => self.android(cx),
            Page::Graphics => self.graphics(cx),
            Page::Settings => self.settings(cx),
            Page::Running => self.apps(cx),
            Page::Widgets => div().flex().flex_col().gap(px(12.))
                .child(self.section("Android widgets",self.group().child(self.row("Live widget panel",button("open-widgets","Open Widgets").disabled(self.working || !matches!(self.runtime.vm,VmState::Running{..})).on_click(cx.listener(|this,_,_,cx|this.open_widgets(cx)))))))
                .child(self.caption("Choose and configure installed widget providers in the panel. Android must stay running for live updates."))
                .child(self.caption("Native WidgetKit desktop widgets are not available in this build."))
                .into_any_element(),
        };
        div()
            .id("glidepod-center")
            .key_context("GlidePod")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .text_color(rgb(p.text))
            .bg(rgb(p.bg))
            .on_action(cx.listener(Self::choose_apk))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::clear_search))
            .on_action(cx.listener(Self::refresh))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.inspect(path.clone(), cx);
                }
            }))
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id("content")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .p(px(24.))
                                    .child(content),
                            )
                            .children(self.notice.as_ref().map(|notice| {
                                self.caption(notice.clone())
                                    .px(px(24.))
                                    .py(px(10.))
                                    .border_t_1()
                                    .border_color(rgb(p.line))
                            })),
                    ),
            )
            .when(self.apk.is_some(), |d| d.child(self.apk_sheet(cx)))
    }
}
