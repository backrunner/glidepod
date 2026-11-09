mod app_window;
mod assets;
mod controls;
mod google_apps_ui;
mod host_files;
mod management_ui;
mod notification_bridge;
mod touch_gestures;
mod tray;
mod views;
mod window_input;
use controls::Button;
use glidepod_core::{ProfileId, Renderer, VmState};
use glidepod_installer::ApkReport;
use glidepod_macos::{HostReport, ProbeReport};
use glidepod_runtime::experimental::{ExperimentalRuntime, RuntimeStatus};
use glidepod_runtime::{Configuration, Store};
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, Icon, Root, Theme, ThemeMode,
    input::{Input, InputEvent, InputState},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

actions!(
    glidepod,
    [
        Quit,
        CloseWindow,
        ShowVmStatus,
        ShowSettings,
        ChooseApk,
        FocusSearch,
        ClearSearch,
        Refresh
    ]
);
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Apps,
    Running,
    Android,
    Graphics,
    Widgets,
    Settings,
}
impl Page {
    fn label(self) -> &'static str {
        match self {
            Self::Apps => "Apps",
            Self::Running => "Running",
            Self::Android => "Android",
            Self::Graphics => "Graphics",
            Self::Widgets => "Widgets",
            Self::Settings => "Settings",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Apps => "apps",
            Self::Running => "running",
            Self::Android => "android",
            Self::Graphics => "graphics",
            Self::Widgets => "widgets",
            Self::Settings => "settings",
        }
    }
}
const PAGES: [Page; 6] = [
    Page::Apps,
    Page::Running,
    Page::Widgets,
    Page::Android,
    Page::Graphics,
    Page::Settings,
];
// Shared semantic colors keep every control and surface consistent.
#[derive(Clone, Copy)]
struct Palette {
    bg: u32,
    side: u32,
    panel: u32,
    text: u32,
    muted: u32,
    line: u32,
    selected: u32,
    accent: u32,
    accent_hover: u32,
    accent_text: u32,
    control_border: u32,
    hover: u32,
}
fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: 0x1c1c1e,
            side: 0x252527,
            panel: 0x2c2c2e,
            text: 0xf2f2f7,
            muted: 0xaeaeb6,
            line: 0x3a3a3c,
            selected: 0x3c4542,
            accent: 0x9bcab4,
            accent_hover: 0xb7dcc9,
            accent_text: 0x162c22,
            control_border: 0x535357,
            hover: 0x3a3a3e,
        }
    } else {
        Palette {
            bg: 0xf6f6f8,
            side: 0xeeeef0,
            panel: 0xffffff,
            text: 0x202124,
            muted: 0x66666e,
            line: 0xe2e2e6,
            selected: 0xdde8e2,
            accent: 0x28634c,
            accent_hover: 0x1c503b,
            accent_text: 0xffffff,
            control_border: 0xc9c9cf,
            hover: 0xebebef,
        }
    }
}
fn effective_dark(config: &Configuration, appearance: WindowAppearance) -> bool {
    if config.follow_system_appearance {
        matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        )
    } else {
        config.dark_mode
    }
}
fn apply_theme(dark: bool, cx: &mut App) {
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let p = palette(dark);
    let theme = Theme::global_mut(cx);
    theme.font_size = px(13.);
    theme.radius = px(8.);
    theme.shadow = false;
    theme.colors.background = rgb(p.panel).into();
    theme.colors.foreground = rgb(p.text).into();
    theme.colors.input = rgb(p.control_border).into();
    theme.colors.muted_foreground = rgb(p.muted).into();
    theme.colors.ring = rgb(p.accent).into();
    theme.colors.primary = rgb(p.accent).into();
    theme.colors.primary_hover = rgb(p.accent_hover).into();
    theme.colors.primary_active = rgb(p.accent).into();
    theme.colors.primary_foreground = rgb(p.accent_text).into();
    theme.colors.secondary = rgb(p.side).into();
    theme.colors.secondary_hover = rgb(p.hover).into();
    theme.colors.secondary_active = rgb(p.selected).into();
    theme.colors.secondary_foreground = rgb(p.text).into();
}
fn symbol(name: &str) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(17.))
        .flex_shrink_0()
}
fn button(id: &'static str, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label)
}
enum Work {
    Apk(Result<ApkReport, String>),
    Probe(Result<ProbeReport, String>),
    Image(Result<String, String>),
    Runtime(u64, Result<RuntimeStatus, String>),
    GpuCatalog(Result<glidepod_runtime::gpu::GpuRegistry, String>),
    Started(Result<(), String>),
    Stopped(Result<RuntimeStatus, String>),
    Catalog(Result<glidepod_runtime::guest::DesktopCatalog, String>),
    MacEntries(Result<glidepod_macos::launchers::SyncReport, String>),
    Details(
        String,
        u64,
        Result<glidepod_runtime::management::AppDetails, String>,
    ),
    Storage(
        u64,
        Result<glidepod_runtime::management::StorageOverview, String>,
    ),
    Benchmark(Result<Box<glidepod_runtime::benchmark::BenchmarkReport>, String>),
    Installed(Result<String, String>),
    Opened(String, String, Result<OpenedApp, String>),
}
struct OpenedApp {
    window: glidepod_runtime::display::AppWindow,
    dock: glidepod_macos::dock::Session,
    icon_path: Option<PathBuf>,
}
fn with_dock(
    window: glidepod_runtime::display::AppWindow,
    package: &str,
    title: &str,
    icon: Option<&std::path::Path>,
) -> anyhow::Result<OpenedApp> {
    let dock = glidepod_macos::dock::Session::start(package, title, icon)?;
    Ok(OpenedApp {
        window,
        dock,
        icon_path: icon.map(std::path::Path::to_path_buf),
    })
}
struct AppSession {
    store: Store,
    tray: tray::Service,
    incoming: Arc<Mutex<Vec<PathBuf>>>,
    center: Option<WindowHandle<Root>>,
    windows: std::collections::HashMap<String, WindowHandle<app_window::AndroidWindow>>,
    notifications: notification_bridge::NotificationBridge,
    google: Arc<Mutex<google_apps_ui::GoogleJob>>,
}
impl Global for AppSession {}
struct Center {
    store: Store,
    config: Configuration,
    dark: bool,
    host: HostReport,
    runtime: RuntimeStatus,
    gpus: glidepod_runtime::gpu::GpuRegistry,
    runtime_polling: bool,
    runtime_revision: u64,
    last_runtime_poll: std::time::Instant,
    page: Page,
    search: Entity<InputState>,
    focus: FocusHandle,
    query: String,
    filter: usize,
    show_diagnostics: bool,
    notice: Option<String>,
    apk: Option<ApkReport>,
    apk_path: Option<PathBuf>,
    apps: Vec<glidepod_runtime::guest::AndroidApp>,
    selected_app: Option<String>,
    app_details: Option<glidepod_runtime::management::AppDetails>,
    storage: Option<glidepod_runtime::management::StorageOverview>,
    benchmark: Option<glidepod_runtime::benchmark::BenchmarkReport>,
    windows: std::collections::HashMap<String, WindowHandle<app_window::AndroidWindow>>,
    catalog_epoch: Option<u64>,
    pending_launch: Option<String>,
    probes: Option<ProbeReport>,
    working: bool,
    picking_file: bool,
    receiver: mpsc::Receiver<Work>,
    sender: mpsc::Sender<Work>,
    incoming: Arc<Mutex<Vec<PathBuf>>>,
    google_revision: u64,
    google_installed: bool,
    google_enabled: bool,
    google_recovery: bool,
    location_latitude: Entity<InputState>,
    location_longitude: Entity<InputState>,
    location_message: String,
    network_message: String,
    bridge_interfaces: Vec<String>,
    _subscriptions: Vec<Subscription>,
}
impl Center {
    fn new(
        store: Store,
        config: Configuration,
        incoming: Arc<Mutex<Vec<PathBuf>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let location_latitude = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Latitude")
                .default_value(
                    config
                        .location
                        .point
                        .as_ref()
                        .map(|p| p.latitude.to_string())
                        .unwrap_or_default(),
                )
        });
        let location_longitude = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Longitude")
                .default_value(
                    config
                        .location
                        .point
                        .as_ref()
                        .map(|p| p.longitude.to_string())
                        .unwrap_or_default(),
                )
        });
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search apps")
                .clean_on_escape()
        });
        let subscription = cx.subscribe_in(&search, window, |this, input, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                this.selected_app = None;
                this.app_details = None;
                cx.notify();
            }
            if matches!(event, InputEvent::PressEnter { .. })
                && this.apk.is_none()
                && matches!(this.runtime.vm, VmState::Running { .. })
                && !input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                })
                && let Some(package) = this.visible_apps().first().map(|a| a.package_id.clone())
            {
                this.open_app(package, cx);
            }
        });
        let (sender, receiver) = mpsc::channel();
        let runtime = store.runtime_status().expect("Read runtime status");
        if !runtime.active() {
            let cached_store = store.clone();
            let cached_sender = sender.clone();
            std::thread::spawn(move || {
                let result = cached_store
                    .cached_apps()
                    .and_then(|apps| cached_store.sync_mac_apps(&apps, false));
                let _ = cached_sender.send(Work::MacEntries(result.map_err(|e| format!("{e:#}"))));
            });
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            gpus: store.gpu_registry().unwrap_or_default(),
            location_latitude,
            location_longitude,
            location_message: String::new(),
            network_message: String::new(),
            bridge_interfaces: glidepod_macos::network::bridge_interfaces().unwrap_or_default(),
            google_revision: 0,
            google_installed: store.google_apps_installation().ok().flatten().is_some(),
            google_enabled: store
                .google_apps_installation()
                .ok()
                .flatten()
                .is_some_and(|i| i.enabled),
            google_recovery: store.google_apps_recovery_pending(),
            apps: store.cached_apps().unwrap_or_default(),
            selected_app: None,
            app_details: None,
            storage: None,
            benchmark: std::fs::read(store.root().join("developer/benchmark/latest.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok()),
            apk_path: None,
            windows: cx.global::<AppSession>().windows.clone(),
            catalog_epoch: None,
            pending_launch: None,
            runtime,
            runtime_polling: false,
            runtime_revision: 0,
            last_runtime_poll: std::time::Instant::now(),
            dark: effective_dark(&config, cx.window_appearance()),
            store,
            config,
            host: glidepod_macos::host_report(),
            page: Page::Apps,
            search,
            focus,
            query: String::new(),
            filter: 0,
            show_diagnostics: false,
            notice: None,
            apk: None,
            probes: None,
            working: false,
            picking_file: false,
            receiver,
            sender,
            incoming,
            _subscriptions: vec![subscription],
        }
    }
    fn sync_appearance(&mut self, cx: &mut Context<Self>) {
        let dark = effective_dark(&self.config, cx.window_appearance());
        if dark != self.dark {
            self.dark = dark;
            apply_theme(dark, cx);
            cx.notify();
        }
    }
    fn poll(&mut self, cx: &mut Context<Self>) {
        self.sync_appearance(cx);
        while let Ok(work) = self.receiver.try_recv() {
            if !matches!(
                &work,
                Work::Runtime(..) | Work::GpuCatalog(..) | Work::Catalog(..) | Work::MacEntries(..)
            ) {
                self.working = false;
            }
            match work {
                Work::GpuCatalog(result) => match result {
                    Ok(gpus) => self.gpus = gpus,
                    Err(error) => self.notice = Some(error),
                },
                Work::Details(package, epoch, result) => {
                    if self.selected_app.as_deref() == Some(&package)
                        && matches!(self.runtime.vm,VmState::Running{epoch:e} if e==epoch)
                    {
                        match result {
                            Ok(d) => {
                                self.app_details = Some(d);
                                self.notice = None;
                            }
                            Err(e) => self.notice = Some(e),
                        }
                    }
                }
                Work::Storage(epoch, result) => {
                    if matches!(self.runtime.vm,VmState::Running{epoch:e} if e==epoch) {
                        match result {
                            Ok(s) => {
                                self.storage = Some(s);
                                self.notice = None;
                            }
                            Err(e) => self.notice = Some(e),
                        }
                    }
                }
                Work::Benchmark(result) => match result {
                    Ok(r) => {
                        self.benchmark = Some(*r);
                        self.notice = Some("Graphics check complete.".into());
                    }
                    Err(e) => self.notice = Some(e),
                },
                Work::Runtime(revision, result) => {
                    self.runtime_polling = false;
                    if revision != self.runtime_revision {
                        continue;
                    }
                    match result {
                        Ok(status) => {
                            self.network_message = if let VmState::Running { epoch } = status.vm {
                                self.store
                                    .network_startup_status(epoch)
                                    .map(|s| s.message)
                                    .unwrap_or_default()
                            } else {
                                String::new()
                            };
                            self.location_message = std::fs::read(
                                self.store.root().join("location-status.json"),
                            )
                            .ok()
                            .filter(|b| b.len() < 8192)
                            .and_then(|b| {
                                serde_json::from_slice::<glidepod_runtime::location::Status>(&b)
                                    .ok()
                            })
                            .filter(|s| {
                                matches!(status.vm, VmState::Running { epoch } if epoch == s.epoch)
                                    && s.source == self.config.location.source
                            })
                            .map(|s| s.message)
                            .unwrap_or_default();
                            if let VmState::Running { epoch } = status.vm
                                && self.catalog_epoch != Some(epoch)
                            {
                                self.catalog_epoch = Some(epoch);
                                self.app_details = None;
                                self.storage = None;
                                self.load_apps(cx);
                            }
                            self.runtime = status;
                        }
                        Err(e) => self.notice = Some(e),
                    }
                }
                Work::Started(Ok(())) => {
                    self.notice = None;
                }
                Work::Started(Err(e)) => self.notice = Some(e),
                Work::Stopped(Ok(status)) => {
                    self.runtime = status;
                    self.notice = None;
                }
                Work::Stopped(Err(e)) => self.notice = Some(e),
                Work::Catalog(Ok(catalog)) => {
                    self.apps = catalog.apps;
                    if let Some(warning) = catalog.launchers.warning() {
                        self.notice = Some(warning);
                    }
                }
                Work::MacEntries(result) => {
                    if let Some(warning) = result.map(|r| r.warning()).unwrap_or_else(Some) {
                        self.notice = Some(warning);
                    }
                }
                Work::Catalog(Err(e)) => {
                    self.notice = Some(e);
                }
                Work::Installed(Ok(message)) => {
                    self.apk = None;
                    self.apk_path = None;
                    self.notice = Some(message);
                    self.load_apps(cx);
                }
                Work::Installed(Err(e)) => self.notice = Some(e),
                Work::Opened(package, title, Ok(session)) => {
                    match app_window::open(
                        session.window,
                        session.dock,
                        title,
                        session.icon_path,
                        self.apps.iter().any(|a| a.package_id == package && a.game),
                        cx,
                    ) {
                        Ok(window) => {
                            self.update_settings(|c| c.record_opened_app(&package), cx);
                            self.windows.insert(package, window);
                            self.notice = None;
                        }
                        Err(e) => self.notice = Some(format!("{e:#}")),
                    }
                }
                Work::Opened(_, _, Err(e)) => self.notice = Some(e),
                Work::Apk(Ok(r)) => {
                    self.apk = Some(r);
                    self.notice = None;
                }
                Work::Probe(Ok(r)) => {
                    self.probes = Some(r);
                    self.page = Page::Graphics;
                }
                Work::Image(Ok(r)) => {
                    self.notice = Some(r);
                    if let Ok(gpus) = self.store.gpu_registry() {
                        self.gpus = gpus;
                    }
                    if let Ok(c) = self.store.load() {
                        self.config = c;
                    }
                }
                Work::Apk(Err(e)) | Work::Probe(Err(e)) | Work::Image(Err(e)) => {
                    self.notice = Some(e)
                }
            }
            cx.notify();
        }
        let google = cx.global::<AppSession>().google.clone();
        {
            let state = google.lock().unwrap();
            if state.revision != self.google_revision {
                self.google_revision = state.revision;
                self.working = state.busy;
                self.notice = Some(state.message.clone());
                if !state.busy {
                    let installation = self.store.google_apps_installation().ok().flatten();
                    self.google_installed = installation.is_some();
                    self.google_enabled = installation.is_some_and(|i| i.enabled);
                    self.google_recovery = self.store.google_apps_recovery_pending();
                    self.apps = self.store.cached_apps().unwrap_or_default();
                    self.catalog_epoch = None;
                }
                cx.notify();
            }
            if state.busy {
                self.working = true;
            }
        }
        self.windows
            .retain(|_, window| window.update(cx, |_, _, _| {}).is_ok());
        cx.global_mut::<AppSession>().windows = self.windows.clone();
        if !self.working
            && matches!(self.runtime.vm, VmState::Running { .. })
            && let Some(package) = self.pending_launch.take()
        {
            self.open_app(package, cx);
        }
        if !self.working
            && !self.runtime_polling
            && self.last_runtime_poll.elapsed() > Duration::from_secs(1)
        {
            self.runtime_polling = true;
            self.last_runtime_poll = std::time::Instant::now();
            let store = self.store.clone();
            let sender = self.sender.clone();
            let revision = self.runtime_revision;
            std::thread::spawn(move || {
                let _ = sender.send(Work::Runtime(
                    revision,
                    store.runtime_status().map_err(|e| format!("{e:#}")),
                ));
                let _ = sender.send(Work::GpuCatalog(
                    store.gpu_registry().map_err(|e| format!("{e:#}")),
                ));
            });
        }
        if !self.working {
            let pending = cx.global::<AppSession>().notifications.opens.clone();
            let open = {
                let mut q = pending.lock().unwrap();
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            };
            if let Some((epoch, n)) = open
                && matches!(self.runtime.vm,VmState::Running{epoch:e} if e==epoch)
            {
                self.open_notification(n, cx);
            }
        }
        if !self.working && self.apk.is_none() {
            let path = self.incoming.lock().ok().and_then(|mut q| {
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            });
            if let Some(path) = path {
                let name = path.to_string_lossy();
                if let Some(package) = name.strip_prefix("glidepod://launch/") {
                    if glidepod_runtime::guest::package_id(package).is_ok() {
                        self.pending_launch = Some(package.into());
                        if !self.runtime.active() {
                            self.start_android(cx);
                        }
                    }
                } else {
                    self.inspect(path, cx);
                }
            }
        }
    }
    fn load_apps(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.guest().and_then(|g| g.desktop_catalog());
            let _ = sender.send(Work::Catalog(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn add_launcher(&mut self, app: glidepod_runtime::guest::AndroidApp, cx: &mut Context<Self>) {
        self.working = true;
        let sender = self.sender.clone();
        let store = self.store.clone();
        std::thread::spawn(move || {
            let result = glidepod_macos::launchers::create_for_store(
                &app.package_id,
                &app.label,
                app.icon_path.as_deref(),
                store.root(),
            )
            .and_then(|p| {
                let status = std::process::Command::new("/usr/bin/open")
                    .arg("-R")
                    .arg(&p)
                    .status()?;
                anyhow::ensure!(status.success(), "Cannot show the app entry in Finder");
                Ok(format!("Shown in Finder: {}", p.display()))
            });
            let _ = sender.send(Work::Image(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn open_widgets(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        self.working = true;
        self.pending_launch = Some("dev.glidepod.widgets".into());
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = glidepod_runtime::guest::resource("glidepod-widgets.apk")
                .and_then(|p| store.guest()?.install(&p, None))
                .map(|_| "Widgets are ready.".into());
            let _ = sender.send(Work::Installed(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn install_apk(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        let Some(path) = self.apk_path.clone() else {
            return;
        };
        let Some(report) = &self.apk else {
            return;
        };
        let hash = report.sha256.clone();
        let store = self.store.clone();
        let sender = self.sender.clone();
        self.working = true;
        self.notice = Some("Installing APK…".into());
        std::thread::spawn(move || {
            let result = store
                .guest()
                .and_then(|g| g.install(&path, Some(&hash)))
                .map(|r| format!("Installed {}", r.package_id));
            let _ = sender.send(Work::Installed(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn open_notification(
        &mut self,
        n: glidepod_runtime::notifications::AndroidNotification,
        cx: &mut Context<Self>,
    ) {
        self.working = true;
        let store = self.store.clone();
        let sender = self.sender.clone();
        let existing = self.windows.get(&n.package_id).and_then(|w| {
            w.update(cx, |v, w, _| {
                w.activate_window();
                v.display_id
            })
            .ok()
        });
        std::thread::spawn(move || {
            if let Some(display) = existing {
                let result = store
                    .guest()
                    .and_then(|g| g.activate_notification(&n, display))
                    .map(|_| "Android notification opened.".into());
                let _ = sender.send(Work::Image(result.map_err(|e| format!("{e:#}"))));
            } else {
                let result = store.guest().and_then(|g| {
                    let window = g.open_notification(&n, 720, 1080)?;
                    let icon = g
                        .catalog()
                        .ok()
                        .and_then(|apps| apps.into_iter().find(|a| a.package_id == n.package_id))
                        .and_then(|a| a.icon_path);
                    with_dock(window, &n.package_id, &n.app_label, icon.as_deref())
                });
                // Reuse the notifier's window on later clicks, even when a system
                // notification opened a different package such as Android Settings.
                // AppWindow still owns cleanup for the actual target task.
                let package = n.package_id;
                let _ = sender.send(Work::Opened(
                    package,
                    n.app_label,
                    result.map_err(|e| format!("{e:#}")),
                ));
            }
        });
        cx.notify();
    }
    fn open_app(&mut self, package: String, cx: &mut Context<Self>) {
        if let Some(window) = self.windows.get(&package)
            && window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            self.update_settings(|c| c.record_opened_app(&package), cx);
            return;
        }
        if self.working {
            return;
        }
        let title = self
            .apps
            .iter()
            .find(|a| a.package_id == package)
            .map(|a| a.label.clone())
            .unwrap_or_else(|| {
                if package == "dev.glidepod.widgets" {
                    "GlidePod Widgets".into()
                } else {
                    package.clone()
                }
            });
        self.working = true;
        self.notice = Some(format!("Opening {title}…"));
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let mut title = title;
            let result = store.guest().and_then(|g| {
                let window = g.open_app(&package, 720, 1080)?;
                let app = g.catalog()?.into_iter().find(|a| a.package_id == package);
                if let Some(app) = &app {
                    title.clone_from(&app.label);
                }
                let icon = app.as_ref().and_then(|a| a.icon_path.as_deref());
                with_dock(window, &package, &title, icon)
            });
            let _ = sender.send(Work::Opened(
                package,
                title,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
        cx.notify();
    }
    fn remove_app(&mut self, package: String, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = window.prompt(
            PromptLevel::Warning,
            "Remove this app?",
            Some("This deletes its Android app data."),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    this.working = true;
                    let store = this.store.clone();
                    let sender = this.sender.clone();
                    if let Some(window) = this.windows.remove(&package) {
                        let _ = window.update(cx, |_, window, _| window.remove_window());
                    }
                    std::thread::spawn(move || {
                        let result = store
                            .guest()
                            .and_then(|g| g.uninstall(&package))
                            .map(|_| format!("Removed {package}"));
                        let _ = sender.send(Work::Installed(result.map_err(|e| format!("{e:#}"))));
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn start_android(&mut self, cx: &mut Context<Self>) {
        if self.working || self.runtime.active() {
            return;
        }
        self.runtime_revision += 1;
        self.working = true;
        self.notice = Some("Starting Android…".into());
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.start_background_vm();
            let _ = sender.send(Work::Started(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn stop_android(&mut self, cx: &mut Context<Self>) {
        if self.working || !self.runtime.active() {
            return;
        }
        self.runtime_revision += 1;
        self.working = true;
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.stop_background_vm();
            let _ = sender.send(Work::Stopped(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn choose_runtime(&mut self, cx: &mut Context<Self>) {
        if self.working || self.picking_file || self.runtime.active() {
            return;
        }
        self.picking_file = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Register experimental runtime".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            let _ = this.update(cx, |this, _| this.picking_file = false);
            if let Ok(Ok(Some(paths))) = result && let Some(path) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| {
                    this.working = true;
                    this.notice = Some("Verifying local runtime files…".into());
                    let store = this.store.clone(); let sender = this.sender.clone();
                    std::thread::spawn(move || {
                        let result = (|| -> anyhow::Result<String> {
                            let file = std::fs::File::open(path)?;
                            anyhow::ensure!(file.metadata()?.len() <= 65536, "Runtime manifest exceeds limit");
                            let runtime: ExperimentalRuntime = serde_json::from_reader(file)?;
                            store.register_experimental(&runtime)?;
                            Ok("Experimental runtime registered. Select Developer to start Android.".into())
                        })();
                        let _ = sender.send(Work::Image(result.map_err(|e| format!("{e:#}"))));
                    });
                    cx.notify();
                });
            }
        }).detach();
    }
    fn select_gpu(&mut self, driver: glidepod_runtime::gpu::VulkanDriver, cx: &mut Context<Self>) {
        if self.working || self.runtime.active() {
            return;
        }
        self.working = true;
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store
                .select_gpu_driver(driver)
                .map(|_| format!("{} selected for the next Metal start.", driver.label()));
            let _ = sender.send(Work::Image(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn choose_gpu(&mut self, driver: glidepod_runtime::gpu::VulkanDriver, cx: &mut Context<Self>) {
        if self.working || self.picking_file || self.runtime.active() {
            return;
        }
        self.picking_file = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(format!("Register {} renderer", driver.label()).into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            let _ = this.update(cx, |this, _| this.picking_file = false);
            if let Ok(Ok(Some(paths))) = result
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.working = true;
                    let store = this.store.clone();
                    let sender = this.sender.clone();
                    std::thread::spawn(move || {
                        let result = store.register_gpu_with_driver(&path, driver).map(|_| {
                            format!(
                                "{} registered and selected for the next Metal start.",
                                driver.label()
                            )
                        });
                        let _ = sender.send(Work::Image(result.map_err(|e| format!("{e:#}"))));
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn inspect(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.apk.is_some() {
            return;
        }
        if self.working {
            self.notice = Some("Wait for the current check to finish.".into());
            cx.notify();
            return;
        }
        self.working = true;
        self.notice = Some("Reading APK metadata…".into());
        self.apk_path = Some(path.clone());
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = glidepod_installer::inspect(&path).map_err(|e| format!("{e:#}"));
            let _ = sender.send(Work::Apk(result));
        });
        cx.notify();
    }
    fn choose_apk(&mut self, _: &ChooseApk, _: &mut Window, cx: &mut Context<Self>) {
        if self.working || self.picking_file || self.apk.is_some() {
            return;
        }
        self.picking_file = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Review APK".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            let _ = this.update(cx, |this, _| this.picking_file = false);
            match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |this, cx| this.inspect(path, cx));
                    }
                }
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.notice = Some(e.to_string());
                        cx.notify();
                    });
                }
                _ => {}
            }
        })
        .detach();
    }
    fn choose_image(&mut self, cx: &mut Context<Self>) {
        if self.working || self.picking_file || self.apk.is_some() {
            return;
        }
        self.picking_file = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Verify image manifest".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            let _ = this.update(cx, |this, _| this.picking_file = false);
            if let Ok(Ok(Some(paths))) = result
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.working = true;
                    self::start_image_check(path, this.sender.clone());
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn probe(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        self.working = true;
        self.notice = None;
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<ProbeReport> {
                let path = std::env::current_exe()?.with_file_name("glidepodctl");
                let mut child = std::process::Command::new(path)
                    .args(["--json", "probe"])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?;
                let deadline = std::time::Instant::now() + Duration::from_secs(30);
                while child.try_wait()?.is_none() {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        anyhow::bail!(
                            "Host checks timed out after 30 seconds. The diagnostic process was stopped."
                        );
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let out = child.wait_with_output()?;
                if out.stdout.len() > 1024 * 1024 {
                    anyhow::bail!("Probe response exceeded the limit");
                }
                let report = serde_json::from_slice::<ProbeReport>(&out.stdout).map_err(|_| {
                    anyhow::anyhow!(
                        "Cannot read host probe results: {}",
                        String::from_utf8_lossy(&out.stderr)
                    )
                })?;
                anyhow::ensure!(
                    matches!(out.status.code(), Some(0 | 2)),
                    "Diagnostic process exited unexpectedly"
                );
                Ok(report)
            })();
            let _ = sender.send(Work::Probe(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn update_settings(&mut self, change: impl FnOnce(&mut Configuration), cx: &mut Context<Self>) {
        match self.store.update(self.config.revision, change) {
            Ok(c) => {
                self.config = c;
                self.sync_appearance(cx);
                self.apps = self.store.cached_apps().unwrap_or_default();
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
        cx.notify();
    }
    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.runtime.vm, VmState::Running { .. }) {
            self.load_apps(cx);
        }
        if self.apk.is_some() {
            return;
        }
        match self.store.load() {
            Ok(c) => {
                self.config = c;
                self.sync_appearance(cx);
                self.notice = None;
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
        cx.notify();
    }
    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.apk.is_some() {
            return;
        }
        self.page = Page::Apps;
        self.selected_app = None;
        self.app_details = None;
        self.search.update(cx, |input, cx| input.focus(window, cx));
        // The search field may have just been mounted by switching to Apps.
        // Wait for its action handler to enter the rendered focus tree.
        let focus = self.search.read(cx).focus_handle(cx);
        window.on_next_frame(move |window, cx| {
            if focus.is_focused(window) {
                window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
            }
        });
        cx.notify();
    }
    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.working && self.apk.is_some() {
            return;
        }
        if self.apk.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }
}

fn start_image_check(path: PathBuf, sender: mpsc::Sender<Work>) {
    std::thread::spawn(move || {
        let result=glidepod_runtime::verify_image(&path).map(|r|format!("Verified {} files for {}. Signature verification and Android boot qualification remain pending.",r.files_verified,r.image_id)).map_err(|e|format!("{e:#}"));
        let _ = sender.send(Work::Image(result));
    });
}
fn main() -> anyhow::Result<()> {
    let store = Store::default_location()?;
    let config = store.load()?;
    let incoming = Arc::new(Mutex::new(
        std::env::args_os()
            .skip(1)
            .take(32)
            .map(PathBuf::from)
            .collect::<Vec<_>>(),
    ));
    let app = Application::new().with_assets(assets::Assets);
    let files = incoming.clone();
    app.on_open_urls(move |urls| {
        if let Ok(mut queue) = files.lock() {
            for url in urls {
                if url.starts_with("glidepod://launch/") && queue.len() < 32 {
                    queue.push(PathBuf::from(&url));
                    continue;
                }
                if let Ok(url) = gpui::http_client::Url::parse(&url)
                    && let Ok(path) = url.to_file_path()
                    && queue.len() < 32
                {
                    queue.push(path);
                }
            }
        }
    });
    app.on_reopen(|cx| {
        let _ = open_center(cx);
        cx.activate(true);
    });
    app.run(move |cx| {
        gpui_component::init(cx);
        let notifications =
            notification_bridge::NotificationBridge::new(store.clone(), incoming.clone());
        let tray = tray::Service::new(store.clone()).expect("Create GlidePod menu bar item");
        cx.set_global(AppSession {
            store,
            tray,
            incoming: incoming.clone(),
            center: None,
            windows: Default::default(),
            notifications,
            google: Arc::new(Mutex::new(google_apps_ui::GoogleJob::default())),
        });
        apply_theme(effective_dark(&config, cx.window_appearance()), cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-,", ShowSettings, Some("GlidePod")),
            KeyBinding::new("cmd-o", ChooseApk, Some("GlidePod")),
            KeyBinding::new("cmd-k", FocusSearch, Some("GlidePod")),
            KeyBinding::new("cmd-f", FocusSearch, Some("GlidePod")),
            KeyBinding::new("escape", ClearSearch, Some("GlidePod")),
            KeyBinding::new("cmd-r", Refresh, Some("GlidePod")),
        ]);
        cx.on_action(|_: &Quit, cx| quit_glidepod(cx));
        cx.on_action(|_: &CloseWindow, cx| {
            if let Some(handle) = cx.active_window() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
        cx.on_action(|_: &ShowVmStatus, cx| cx.global::<AppSession>().tray.show());
        cx.set_menus(vec![
            Menu {
                name: "GlidePod".into(),
                items: vec![
                    MenuItem::action("VM Status", ShowVmStatus),
                    MenuItem::action("Settings…", ShowSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit GlidePod", Quit),
                ],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("Add APK…", ChooseApk),
                    MenuItem::action("Refresh", Refresh),
                    MenuItem::separator(),
                    MenuItem::action("Close Window", CloseWindow),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Cut", gpui_component::input::Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", gpui_component::input::Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", gpui_component::input::Paste, OsAction::Paste),
                    MenuItem::os_action(
                        "Select All",
                        gpui_component::input::SelectAll,
                        OsAction::SelectAll,
                    ),
                ],
            },
        ]);
        open_center(cx).expect("Open GlidePod window");
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let _ = cx.update(|cx| {
                    tray::poll(cx);
                    let pending = cx
                        .global::<AppSession>()
                        .incoming
                        .lock()
                        .map(|q| !q.is_empty())
                        .unwrap_or(false);
                    let notification_pending = !cx
                        .global::<AppSession>()
                        .notifications
                        .opens
                        .lock()
                        .unwrap()
                        .is_empty();
                    let existing = cx.global::<AppSession>().center;
                    if (pending || notification_pending)
                        && existing.is_none_or(|h| h.update(cx, |_, _, _| {}).is_err())
                    {
                        let _ = open_center(cx);
                    }
                });
            }
        })
        .detach();
        // The lightweight agent takes over the menu bar when the last window closes.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() && !cx.global::<AppSession>().google.lock().unwrap().busy {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}

fn open_center(cx: &mut App) -> anyhow::Result<()> {
    if !cx.has_global::<AppSession>() {
        return Ok(());
    }
    if let Some(handle) = cx.global::<AppSession>().center
        && handle.update(cx, |_, w, _| w.activate_window()).is_ok()
    {
        return Ok(());
    }
    let store = cx.global::<AppSession>().store.clone();
    let incoming = cx.global::<AppSession>().incoming.clone();
    let config = store.load()?;
    let bounds = Bounds::centered(None, size(px(1000.), px(680.)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(820.), px(560.))),
            titlebar: Some(TitlebarOptions {
                title: Some("GlidePod".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.), px(19.))),
            }),
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(|cx| Center::new(store, config, incoming, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        },
    )?;
    cx.global_mut::<AppSession>().center = Some(handle);
    Ok(())
}

fn quit_glidepod(cx: &mut App) {
    let store = cx.global::<AppSession>().store.clone();
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { store.quit_background() })
            .await;
        let _ = cx.update(|cx| {
            if let Err(error) = result {
                let _ = open_center(cx);
                if let Some(handle) = cx.global::<AppSession>().center {
                    let _ = handle.update(cx, |_, window, cx| {
                        let prompt = window.prompt(
                            PromptLevel::Warning,
                            "Could not finish quitting GlidePod",
                            Some(&format!("{error:#}")),
                            &["OK"],
                            cx,
                        );
                        cx.spawn(async move |_, _| {
                            let _ = prompt.await;
                        })
                        .detach();
                    });
                }
            } else {
                cx.quit();
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod appearance_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn follows_system_including_vibrant_appearances_only_when_selected() {
        let mut config = Configuration::default();
        for (appearance, expected) in [
            (WindowAppearance::Light, false),
            (WindowAppearance::VibrantLight, false),
            (WindowAppearance::Dark, true),
            (WindowAppearance::VibrantDark, true),
        ] {
            assert_eq!(effective_dark(&config, appearance), expected);
            config.follow_system_appearance = false;
            config.dark_mode = !expected;
            assert_eq!(effective_dark(&config, appearance), !expected);
            config.follow_system_appearance = true;
        }
    }
}
