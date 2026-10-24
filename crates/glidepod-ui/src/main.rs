mod app_window;
mod assets;
mod controls;
mod management_ui;
mod notification_bridge;
mod views;
mod window_input;
use controls::Button;
use glidepod_core::{ProfileId, Renderer, VmState};
use glidepod_installer::ApkReport;
use glidepod_macos::{HostReport, ProbeReport};
use glidepod_runtime::experimental::{ExperimentalRuntime, RuntimeOwner, RuntimeStatus};
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
    [Quit, ChooseApk, FocusSearch, ClearSearch, Refresh]
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
    Page::Android,
    Page::Graphics,
    Page::Widgets,
    Page::Settings,
];
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
}
fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: 0x202923,
            side: 0x242e27,
            panel: 0x28322b,
            text: 0xe0e7dd,
            muted: 0x98a596,
            line: 0x374239,
            selected: 0x3a503e,
            accent: 0x9ab9a1,
        }
    } else {
        Palette {
            bg: 0xfbfcfa,
            side: 0xf0f3ef,
            panel: 0xffffff,
            text: 0x24342e,
            muted: 0x69776e,
            line: 0xe0e6df,
            selected: 0xdce7dc,
            accent: 0x315e4b,
        }
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
    theme.font_size = px(12.);
    theme.radius = px(6.);
    theme.shadow = false;
    theme.colors.background = rgb(p.panel).into();
    theme.colors.foreground = rgb(p.text).into();
    theme.colors.input = rgb(p.line).into();
    theme.colors.muted_foreground = rgb(if dark { 0x8c9e8e } else { 0x839087 }).into();
    theme.colors.ring = rgb(0x709982).into();
    theme.colors.primary = rgb(p.accent).into();
    theme.colors.primary_hover = rgb(if dark { 0xb0c9b4 } else { 0x284e3e }).into();
    theme.colors.primary_active = rgb(p.accent).into();
    theme.colors.primary_foreground = rgb(if dark { 0x192d20 } else { 0xffffff }).into();
    theme.colors.secondary = rgb(if dark { 0x373a37 } else { 0xf0f1ef }).into();
    theme.colors.secondary_hover = rgb(if dark { 0x454945 } else { 0xe6e8e4 }).into();
    theme.colors.secondary_active = rgb(if dark { 0x4d524d } else { 0xdde1da }).into();
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
    Started(Result<RuntimeOwner, String>),
    Stopped(Result<RuntimeStatus, String>),
    Catalog(Result<Vec<glidepod_runtime::guest::AndroidApp>, String>),
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
    Opened(
        String,
        String,
        Result<glidepod_runtime::display::AppWindow, String>,
    ),
}
struct AppSession {
    store: Store,
    incoming: Arc<Mutex<Vec<PathBuf>>>,
    owner: Option<RuntimeOwner>,
    center: Option<WindowHandle<Root>>,
    windows: std::collections::HashMap<String, WindowHandle<app_window::AndroidWindow>>,
    notifications: notification_bridge::NotificationBridge,
}
impl Global for AppSession {}
struct Center {
    store: Store,
    config: Configuration,
    host: HostReport,
    runtime: RuntimeStatus,
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
        let runtime = store.runtime_status().expect("Read runtime status");
        Self {
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
    fn poll(&mut self, cx: &mut Context<Self>) {
        while let Ok(work) = self.receiver.try_recv() {
            if !matches!(&work, Work::Runtime(..) | Work::Catalog(..)) {
                self.working = false;
            }
            match work {
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
                        self.notice = Some(
                            "3D check complete. Results exclude native window presentation.".into(),
                        );
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
                            if let VmState::Running { epoch } = status.vm
                                && self.catalog_epoch != Some(epoch)
                            {
                                self.catalog_epoch = Some(epoch);
                                self.app_details = None;
                                self.storage = None;
                                self.load_apps(cx);
                            }
                            self.runtime = status;
                            if !self.runtime.active() {
                                cx.global_mut::<AppSession>().owner.take();
                            }
                        }
                        Err(e) => self.notice = Some(e),
                    }
                }
                Work::Started(Ok(owner)) => {
                    cx.global_mut::<AppSession>().owner = Some(owner);
                    self.notice = None;
                }
                Work::Started(Err(e)) => self.notice = Some(e),
                Work::Stopped(Ok(status)) => {
                    self.runtime = status;
                    self.notice = None;
                }
                Work::Stopped(Err(e)) => self.notice = Some(e),
                Work::Catalog(Ok(apps)) => {
                    self.apps = apps;
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
                        session,
                        title,
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
            });
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
            let result = store.guest().and_then(|g| g.catalog());
            let _ = sender.send(Work::Catalog(result.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }
    fn add_launcher(&mut self, app: glidepod_runtime::guest::AndroidApp, cx: &mut Context<Self>) {
        self.working = true;
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = glidepod_macos::launchers::create(
                &app.package_id,
                &app.label,
                app.icon_path.as_deref(),
            )
            .map(|p| format!("Added to {}", p.display()));
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
            let result = store.guest().and_then(|g| g.open_app(&package, 720, 1080));
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
            let result = std::env::current_exe()
                .map_err(anyhow::Error::from)
                .and_then(|p| store.start_experimental(&p.with_file_name("glidepodctl")));
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
        let owned = cx.global_mut::<AppSession>().owner.take().is_some();
        let store = self.store.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = store.stop_vm().or_else(|e| {
                if owned {
                    store.runtime_status()
                } else {
                    Err(e)
                }
            });
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
    fn choose_gpu(&mut self, cx: &mut Context<Self>) {
        if self.working || self.picking_file || self.runtime.active() {
            return;
        }
        self.picking_file = true;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Register experimental GPU renderer".into()),
        });
        cx.spawn(async move |this,cx| {
            let result = paths.await; let _ = this.update(cx,|this,_|this.picking_file = false);
            if let Ok(Ok(Some(paths))) = result && let Some(path) = paths.into_iter().next() { let _ = this.update(cx,|this,cx| {
                this.working = true; let store = this.store.clone(); let sender = this.sender.clone();
                std::thread::spawn(move || { let result = store.register_gpu(&path).map(|_|"Experimental GPU renderer registered. Select Metal for the next start.".into()); let _ = sender.send(Work::Image(result.map_err(|e|format!("{e:#}")))); }); cx.notify();
            }); }
        }).detach();
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
                apply_theme(self.config.dark_mode, cx);
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
        cx.set_global(AppSession {
            store,
            incoming: incoming.clone(),
            owner: None,
            center: None,
            windows: Default::default(),
            notifications,
        });
        apply_theme(config.dark_mode, cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-o", ChooseApk, Some("GlidePod")),
            KeyBinding::new("cmd-k", FocusSearch, Some("GlidePod")),
            KeyBinding::new("cmd-f", FocusSearch, Some("GlidePod")),
            KeyBinding::new("escape", ClearSearch, Some("GlidePod")),
            KeyBinding::new("cmd-r", Refresh, Some("GlidePod")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![
            Menu {
                name: "GlidePod".into(),
                items: vec![MenuItem::action("Quit GlidePod", Quit)],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("Add APK…", ChooseApk),
                    MenuItem::action("Refresh", Refresh),
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
                    let pending = cx
                        .global::<AppSession>()
                        .incoming
                        .lock()
                        .map(|q| !q.is_empty())
                        .unwrap_or(false);
                    let existing = cx.global::<AppSession>().center;
                    if pending && existing.is_none_or(|h| h.update(cx, |_, _, _| {}).is_err()) {
                        let _ = open_center(cx);
                    }
                });
            }
        })
        .detach();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
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
