//! Window input lifecycle and bounded motion coalescing, independent of frame acquisition.
use glidepod_runtime::{
    display::AppWindow,
    input::{InputEvent, InputSession},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static NEXT_LEASE: AtomicU64 = AtomicU64::new(1);
static ACTIVE_LEASE: AtomicU64 = AtomicU64::new(0);
// Serializes Android focus and IME commands, which operate on the focused editor.
static EDITOR: Mutex<Option<std::sync::Weak<Mutex<InputSession>>>> = Mutex::new(None);
// Keep the old session reachable until its final release is serialized with new focus.
// Otherwise the last Arc could disappear before InputSession::drop resets the shared pad.
struct InputCleanup(Arc<Mutex<InputSession>>);
impl Drop for InputCleanup {
    fn drop(&mut self) {
        let mut editor = EDITOR.lock().unwrap();
        let _ = self.0.lock().unwrap().send(InputEvent::Reset);
        if editor
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|active| Arc::ptr_eq(&active, &self.0))
        {
            *editor = None;
        }
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Device(InputEvent),
    Motion(InputEvent),
    Text(String),
    Edit(&'static str),
}
pub enum Update {
    Ready,
    Closed,
    Error(String),
    Clipboard(String),
}
struct Queue {
    items: VecDeque<(u64, Action)>,
    closed: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}
pub struct WindowInput {
    shared: Arc<Shared>,
    lease: u64,
    pub updates: mpsc::Receiver<Update>,
    pub stopped: Arc<AtomicBool>,
}
fn replaceable(a: &Action, b: &Action) -> bool {
    match (a, b) {
        (
            Action::Motion(InputEvent::Touch { points: a }),
            Action::Motion(InputEvent::Touch { points: b }),
        ) => a.iter().map(|p| p.0).eq(b.iter().map(|p| p.0)),
        (
            Action::Device(InputEvent::Pad { buttons: a, .. }),
            Action::Device(InputEvent::Pad { buttons: b, .. }),
        ) => a == b,
        (
            Action::Motion(InputEvent::Mouse { state: a }),
            Action::Motion(InputEvent::Mouse { state: b }),
        ) => a.2 == b.2 && a.3 == 0. && a.4 == 0. && b.3 == 0. && b.4 == 0.,
        _ => false,
    }
}
impl WindowInput {
    pub fn new(window: Arc<AppWindow>) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                items: VecDeque::new(),
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let stopped = Arc::new(AtomicBool::new(false));
        let (tx, updates) = mpsc::channel();
        let queue = shared.clone();
        std::thread::spawn(move || {
            let run = || -> anyhow::Result<()> {
                let input = Arc::new(Mutex::new(window.open_input()?));
                let _cleanup = InputCleanup(input.clone());
                let mut owner = 0;
                let mut heartbeat = Instant::now();
                loop {
                    let item = {
                        let mut q = queue.queue.lock().unwrap();
                        if q.items.is_empty() && !q.closed {
                            q = queue
                                .wake
                                .wait_timeout(q, Duration::from_millis(16))
                                .unwrap()
                                .0;
                        }
                        if q.closed {
                            break;
                        }
                        q.items.pop_front()
                    };
                    if owner != 0 && owner != ACTIVE_LEASE.load(Ordering::SeqCst) {
                        let _editor = EDITOR.lock().unwrap();
                        input.lock().unwrap().send(InputEvent::Reset)?;
                        owner = 0;
                    }
                    if let Some((lease, action)) = item {
                        if matches!(action, Action::Device(InputEvent::Reset)) {
                            let _editor = EDITOR.lock().unwrap();
                            input.lock().unwrap().send(InputEvent::Reset)?;
                            owner = 0;
                            continue;
                        }
                        if lease == 0 || lease != ACTIVE_LEASE.load(Ordering::SeqCst) {
                            continue;
                        }
                        match action {
                            Action::Device(InputEvent::Focus) => {
                                let mut editor = EDITOR.lock().unwrap();
                                if lease == ACTIVE_LEASE.load(Ordering::SeqCst) {
                                    // Release the previous display BEFORE changing Android focus.
                                    if let Some(previous) =
                                        editor.as_ref().and_then(std::sync::Weak::upgrade)
                                        && !Arc::ptr_eq(&previous, &input)
                                    {
                                        previous.lock().unwrap().send(InputEvent::Reset)?;
                                    }
                                    input.lock().unwrap().send(InputEvent::Focus)?;
                                    *editor = Some(Arc::downgrade(&input));
                                    owner = lease;
                                    let _ = tx.send(Update::Ready);
                                }
                            }
                            Action::Device(event) | Action::Motion(event) => {
                                let _editor = EDITOR.lock().unwrap();
                                if owner == lease && lease == ACTIVE_LEASE.load(Ordering::SeqCst) {
                                    input.lock().unwrap().send(event)?;
                                }
                            }
                            Action::Text(text) => {
                                let _editor = EDITOR.lock().unwrap();
                                if owner == lease && lease == ACTIVE_LEASE.load(Ordering::SeqCst) {
                                    // The IME independently checks the focused editor's package.
                                    match window.text(&text) {
                                        Ok(()) => {
                                            let _ = tx.send(Update::Ready);
                                        }
                                        Err(e) => {
                                            let _ = tx.send(Update::Error(e.to_string()));
                                        }
                                    }
                                }
                            }
                            Action::Edit(action) => {
                                let _editor = EDITOR.lock().unwrap();
                                if owner == lease && lease == ACTIVE_LEASE.load(Ordering::SeqCst) {
                                    match window.edit_action(action) {
                                        Ok(Some(text)) => {
                                            let _ = tx.send(Update::Ready);
                                            let _ = tx.send(Update::Clipboard(text));
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            let _ = tx.send(Update::Error(e.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if heartbeat.elapsed() >= Duration::from_millis(250) {
                        let mut session = input.lock().unwrap();
                        session.send(InputEvent::Ping)?;
                        if !session.task_present {
                            let _ = tx.send(Update::Closed);
                            break;
                        }
                        heartbeat = Instant::now();
                    }
                }
                Ok(())
            };
            if let Err(e) = run() {
                let _ = tx.send(Update::Error(format!(
                    "Input disconnected: {e:#}. Reopen this window."
                )));
            }
            queue.queue.lock().unwrap().closed = true;
        });
        Self {
            shared,
            lease: 0,
            updates,
            stopped,
        }
    }
    pub fn active(&self) -> bool {
        self.lease != 0 && self.lease == ACTIVE_LEASE.load(Ordering::SeqCst)
    }
    pub fn activate(&mut self) {
        self.lease = NEXT_LEASE.fetch_add(1, Ordering::SeqCst);
        ACTIVE_LEASE.store(self.lease, Ordering::SeqCst);
        let mut q = self.shared.queue.lock().unwrap();
        q.items.clear();
        q.items
            .push_back((self.lease, Action::Device(InputEvent::Focus)));
        self.shared.wake.notify_one();
    }
    pub fn deactivate(&mut self) {
        let _ = ACTIVE_LEASE.compare_exchange(self.lease, 0, Ordering::SeqCst, Ordering::SeqCst);
        self.lease = 0;
        let mut q = self.shared.queue.lock().unwrap();
        q.items.clear();
        q.items.push_back((0, Action::Device(InputEvent::Reset)));
        self.shared.wake.notify_one();
    }
    pub fn send(&mut self, action: Action) -> Result<(), String> {
        if !self.active() {
            return Ok(());
        }
        let mut q = self.shared.queue.lock().unwrap();
        if q.closed {
            return Err("Input disconnected. Reopen this window.".into());
        }
        if let Some((lease, last)) = q.items.back_mut()
            && *lease == self.lease
            && replaceable(last, &action)
        {
            *last = action;
            return Ok(());
        }
        if q.items.len() >= 256 {
            drop(q);
            self.deactivate();
            return Err("Input queue stalled. Click this window to reconnect input.".into());
        }
        q.items.push_back((self.lease, action));
        self.shared.wake.notify_one();
        Ok(())
    }
}
impl Drop for WindowInput {
    fn drop(&mut self) {
        self.deactivate();
        self.stopped.store(true, Ordering::Release);
        self.shared.queue.lock().unwrap().closed = true;
        self.shared.wake.notify_one();
    }
}
/// GPUI logical keys to Android key codes. Native text composition is handled separately.
pub fn keycode(key: &str) -> Option<u32> {
    if key.len() == 1 {
        let b = key.as_bytes()[0].to_ascii_lowercase();
        if b.is_ascii_lowercase() {
            return Some(29 + u32::from(b - b'a'));
        }
        if b.is_ascii_digit() {
            return Some(7 + u32::from(b - b'0'));
        }
    }
    Some(match key {
        "backspace" => 67,
        "delete" => 112,
        "enter" => 66,
        "escape" => 4,
        "tab" => 61,
        "space" | " " => 62,
        "left" => 21,
        "right" => 22,
        "up" => 19,
        "down" => 20,
        "home" => 122,
        "end" => 123,
        "pageup" => 92,
        "pagedown" => 93,
        "-" => 69,
        "=" => 70,
        "[" => 71,
        "]" => 72,
        "\\" => 73,
        ";" => 74,
        "'" => 75,
        "," => 55,
        "." => 56,
        "/" => 76,
        "`" => 68,
        "f1" => 131,
        "f2" => 132,
        "f3" => 133,
        "f4" => 134,
        "f5" => 135,
        "f6" => 136,
        "f7" => 137,
        "f8" => 138,
        "f9" => 139,
        "f10" => 140,
        "f11" => 141,
        "f12" => 142,
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_motion_with_unchanged_contacts_and_buttons_can_coalesce() {
        let t = |id, x| {
            Action::Motion(InputEvent::Touch {
                points: vec![(id, x, 10.)],
            })
        };
        assert!(replaceable(&t(0, 1.), &t(0, 2.)));
        let down = Action::Device(InputEvent::Touch {
            points: vec![(0, 1., 10.)],
        });
        assert!(
            !replaceable(&down, &t(0, 2.)),
            "Touch down must retain its original hit-test position"
        );
        assert!(!replaceable(
            &Action::Device(InputEvent::Mouse {
                state: (1., 1., 1, 0., 0.)
            }),
            &Action::Motion(InputEvent::Mouse {
                state: (2., 2., 1, 0., 0.)
            })
        ));
        assert!(!replaceable(&t(0, 1.), &t(1, 2.)));
        assert!(!replaceable(
            &t(0, 1.),
            &Action::Device(InputEvent::Touch { points: vec![] })
        ));
        let pad = |b| {
            Action::Device(InputEvent::Pad {
                buttons: b,
                axes: [0.; 8],
            })
        };
        assert!(!replaceable(&pad(0), &pad(1)));
        assert!(replaceable(&pad(1), &pad(1)));
        assert!(!replaceable(
            &Action::Text("a".into()),
            &Action::Text("b".into())
        ));
    }
    #[test]
    fn keyboard_covers_gameplay_and_navigation() {
        assert_eq!(keycode("w"), Some(51));
        assert_eq!(keycode("space"), Some(62));
        assert_eq!(keycode("left"), Some(21));
        assert_eq!(keycode("unknown"), None);
    }
    #[test]
    #[ignore = "requires GLIDEPOD_TEST_STORE and two installed acceptance fixtures"]
    fn real_focus_lease_routing() -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        use glidepod_runtime::{Store, input::GamepadState};
        use std::{fs, path::PathBuf, thread};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let data =
            PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing test store")?)
                .canonicalize()?;
        ensure!(
            data.starts_with(root.join(".cache")),
            "Use a disposable store"
        );
        let store = Store::new(data)?;
        let guest = store.guest()?;
        for package in ["dev.glidepod.fixture", "dev.glidepod.fixture.secondary"] {
            guest.developer_shell(
                &format!(
                    "am force-stop {package}; su 0 rm -f /data/user/0/{package}/files/events.jsonl"
                ),
                false,
            )?;
        }
        let a = Arc::new(guest.open_app("dev.glidepod.fixture", 720, 1080)?);
        let b = Arc::new(guest.open_app("dev.glidepod.fixture.secondary", 800, 1000)?);
        let mut ia = WindowInput::new(a.clone());
        let mut ib = WindowInput::new(b.clone());
        let wait_ready = |input: &WindowInput| -> anyhow::Result<()> {
            loop {
                match input.updates.recv_timeout(Duration::from_secs(8))? {
                    Update::Ready => return Ok(()),
                    Update::Error(e) => anyhow::bail!(e),
                    Update::Clipboard(_) => {}
                    Update::Closed => anyhow::bail!("App task closed unexpectedly"),
                }
            }
        };
        let send = |input: &mut WindowInput, action: Action| -> anyhow::Result<()> {
            input.send(action).map_err(anyhow::Error::msg)
        };
        let key = |code, down| {
            Action::Device(InputEvent::Key {
                code,
                down,
                meta: 0,
            })
        };
        ia.activate();
        wait_ready(&ia).context("Initial A focus")?;
        send(&mut ia, Action::Text("Alpha · 中文".into()))?;
        wait_ready(&ia).context("A Unicode commit")?;
        a.edit_action("select_all")?;
        ensure!(
            a.edit_action("copy")?.as_deref() == Some("Alpha · 中文"),
            "A Unicode text/clipboard mismatch"
        );
        a.key(22)?;
        send(&mut ia, key(51, true))?;
        send(
            &mut ia,
            Action::Device(InputEvent::Touch {
                points: vec![(0, 300., 700.)],
            }),
        )?;
        send(
            &mut ia,
            Action::Device(
                GamepadState {
                    buttons: 1,
                    axes: [0.7, 0., 0., 0., 0., 0., 0., 0.],
                }
                .into(),
            ),
        )?;
        thread::sleep(Duration::from_millis(300));
        ib.activate();
        wait_ready(&ib).context("B focus")?;
        send(&mut ib, Action::Text("Beta 😀".into()))?;
        wait_ready(&ib).context("B Unicode commit")?;
        b.edit_action("select_all")?;
        ensure!(
            b.edit_action("copy")?.as_deref() == Some("Beta 😀"),
            "B Unicode text/clipboard mismatch"
        );
        b.key(22)?;
        send(&mut ia, key(54, true))?; // Stale inactive producer must be rejected before its queue.
        send(&mut ib, key(52, true))?;
        send(&mut ib, key(52, false))?;
        send(
            &mut ib,
            Action::Device(
                GamepadState {
                    buttons: 2,
                    ..Default::default()
                }
                .into(),
            ),
        )?;
        thread::sleep(Duration::from_millis(300));
        // Switch rapidly with input still queued. Only the last lease may keep sending.
        for _ in 0..8 {
            ia.activate();
            send(&mut ia, key(53, true))?;
            ib.activate();
            send(&mut ia, key(54, true))?;
        }
        wait_ready(&ib).context("B focus")?;
        send(&mut ib, key(36, true))?;
        send(&mut ib, key(36, false))?;
        thread::sleep(Duration::from_millis(300));
        ib.deactivate();
        send(&mut ib, key(55, true))?;
        ia.activate();
        wait_ready(&ia).context("A focus before close")?;
        send(&mut ia, key(51, true))?;
        send(
            &mut ia,
            Action::Device(
                GamepadState {
                    buttons: 1,
                    ..Default::default()
                }
                .into(),
            ),
        )?;
        thread::sleep(Duration::from_millis(100));
        drop(ia);
        ib.activate();
        wait_ready(&ib).context("B focus during A close")?;
        send(
            &mut ib,
            Action::Device(
                GamepadState {
                    buttons: 2,
                    ..Default::default()
                }
                .into(),
            ),
        )?;
        thread::sleep(Duration::from_millis(100));
        ib.deactivate();
        drop(ib);
        thread::sleep(Duration::from_millis(300));
        let logs = |w: &AppWindow| -> anyhow::Result<Vec<serde_json::Value>> {
            let data = w.guest.developer_shell(
                &format!(
                    "su 0 cat /data/user/0/{}/files/events.jsonl",
                    w.info.package_id
                ),
                false,
            )?;
            Ok(data
                .lines()
                .map(serde_json::from_str)
                .collect::<std::result::Result<_, _>>()?)
        };
        let ea = logs(&a)?;
        let eb = logs(&b)?;
        let has = |events: &[serde_json::Value], code: u32, action: u32| {
            events
                .iter()
                .any(|e| e["kind"] == "key" && e["key"] == code && e["action"] == action)
        };
        ensure!(
            has(&ea, 51, 0) && has(&ea, 51, 1) && !has(&eb, 51, 0),
            "Focus transfer failed to release the old window's key"
        );
        ensure!(
            has(&ea, 96, 0) && has(&ea, 96, 1) && !has(&eb, 96, 0),
            "Controller leaked or did not release on focus transfer"
        );
        ensure!(
            ea.iter().any(|e| e["kind"] == "touch" && e["action"] == 3),
            "Touch did not cancel on focus transfer"
        );
        ensure!(
            has(&eb, 52, 0) && has(&eb, 52, 1) && !has(&ea, 52, 0),
            "Input leaked to the old window"
        );
        ensure!(
            !has(&ea, 54, 0) && !has(&eb, 54, 0) && !has(&eb, 55, 0),
            "Inactive window accepted input"
        );
        ensure!(
            has(&eb, 36, 0) && has(&eb, 36, 1),
            "Rapid focus transfer stranded the final lease"
        );
        ensure!(
            has(&eb, 97, 0) && has(&eb, 97, 1),
            "Final controller did not release"
        );
        ensure!(
            ea.iter()
                .filter(|e| e["kind"] == "key" && e["key"] == 96 && e["action"] == 0)
                .count()
                == 2,
            "Close handoff was not exercised with a held controller button"
        );
        for ev in [&ea, &eb] {
            let mut held = std::collections::HashSet::new();
            for e in ev {
                if e["kind"] == "key" {
                    let k = e["key"].as_u64().unwrap();
                    if e["action"] == 0 {
                        held.insert(k);
                    } else {
                        held.remove(&k);
                    }
                }
            }
            ensure!(held.is_empty(), "Window input left held keys: {held:?}");
        }
        let out = root.join("evidence/g3/host-focus");
        fs::create_dir_all(&out)?;
        fs::write(out.join("a-events.json"), serde_json::to_vec_pretty(&ea)?)?;
        fs::write(out.join("b-events.json"), serde_json::to_vec_pretty(&eb)?)?;
        fs::write(
            out.join("report.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"passed":true,"epoch":guest.endpoints.epoch,"rapid_focus_changes":16,"checks":["host focus lease invalidation","Unicode IME commit/clipboard after focus transfer","stale producer rejection","release keys/controller and cancel touch on focus loss","rapid switch with queued input","final window remains interactive","active close with held key/controller during focus transfer","deactivation and close leave no keys held"],"native_ui_events_verified":false}),
            )?,
        )?;
        Ok(())
    }
}
