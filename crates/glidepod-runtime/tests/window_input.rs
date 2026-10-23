//! Opt-in acceptance against two real, independently rendering Android tasks.
use anyhow::{Context, Result, ensure};
use glidepod_runtime::{
    Store,
    display::AppWindow,
    input::{GamepadState, InputEvent},
};
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
fn events(w: &AppWindow) -> Result<Vec<serde_json::Value>> {
    let s = w.guest.developer_shell(
        &format!(
            "su 0 cat /data/user/0/{}/files/events.jsonl",
            w.info.package_id
        ),
        false,
    )?;
    Ok(s.lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?)
}
fn has(v: &[serde_json::Value], kind: &str, key: u32, action: u32) -> bool {
    v.iter()
        .any(|e| e["kind"] == kind && e["key"] == key && e["action"] == action)
}
#[test]
#[ignore = "requires GLIDEPOD_TEST_STORE and a running disposable Android VM"]
fn independent_render_and_device_input() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let data =
        PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing test store")?)
            .canonicalize()?;
    ensure!(
        data.starts_with(root.join(".cache")),
        "Use disposable test store"
    );
    let store = Store::new(data)?;
    let guest = store.guest()?;
    for name in ["glidepod-fixture.apk", "glidepod-fixture-secondary.apk"] {
        guest.install(&root.join("target/android").join(name), None)?;
    }
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
    ensure!(
        a.info.display_id != b.info.display_id && a.info.scanout != b.info.scanout,
        "Displays share identity"
    );
    thread::sleep(Duration::from_secs(1));
    let mut ia = a.open_input()?;
    let mut ib = b.open_input()?;
    ensure!(a.open_input().is_err(), "Duplicate input channel admitted");
    ia.send(InputEvent::Focus)?;
    let start = Instant::now();
    ia.send(InputEvent::Key {
        code: 51,
        down: true,
        meta: 0,
    })?;
    ia.send(InputEvent::Key {
        code: 38,
        down: true,
        meta: 0,
    })?;
    ia.send(InputEvent::Key {
        code: 51,
        down: true,
        meta: 0,
    })?;
    ia.send(InputEvent::Key {
        code: 51,
        down: false,
        meta: 0,
    })?;
    ia.send(InputEvent::Key {
        code: 38,
        down: false,
        meta: 0,
    })?;
    ia.send(InputEvent::Touch {
        points: vec![(0, 300., 600.)],
    })?;
    ia.send(InputEvent::Touch {
        points: vec![(0, 330., 650.), (1, 500., 650.)],
    })?;
    ia.send(InputEvent::Touch {
        points: vec![(1, 510., 660.)],
    })?;
    ia.send(InputEvent::Touch { points: vec![] })?;
    ia.send(InputEvent::Mouse {
        state: (350., 700., 2, 0., 0.),
    })?;
    ia.send(InputEvent::Mouse {
        state: (350., 700., 0, -0.5, 1.),
    })?;
    ia.send(
        GamepadState {
            buttons: 1,
            axes: [0.5, -0.7, 0.2, 0.3, 0.4, 0.8, -1., 1.],
        }
        .into(),
    )?;
    ia.send(InputEvent::Key {
        code: 29,
        down: true,
        meta: 1,
    })?;
    ia.send(InputEvent::Touch {
        points: vec![(0, 300., 600.)],
    })?;
    ia.send(InputEvent::Reset)?;
    ib.send(InputEvent::Focus)?;
    ib.send(InputEvent::Key {
        code: 52,
        down: true,
        meta: 0,
    })?;
    ib.send(InputEvent::Key {
        code: 52,
        down: false,
        meta: 0,
    })?;
    ib.send(
        GamepadState {
            buttons: 2,
            ..Default::default()
        }
        .into(),
    )?;
    ib.send(InputEvent::Reset)?;
    let elapsed = start.elapsed();
    thread::sleep(Duration::from_millis(300));
    // The pointer must arrive while held, not be replayed after release.
    ia.send(InputEvent::Focus)?;
    ia.send(InputEvent::Touch {
        points: vec![(0, 200., 700.)],
    })?;
    let hold_started = Instant::now();
    for _ in 0..4 {
        thread::sleep(Duration::from_millis(200));
        ia.send(InputEvent::Ping)?;
        ib.send(InputEvent::Ping)?;
    }
    let held_events = events(&a)?;
    ensure!(
        held_events
            .last()
            .is_some_and(|e| e["kind"] == "touch" && e["action"] == 0),
        "Touch down did not arrive before release"
    );
    ia.send(InputEvent::Touch { points: vec![] })?;
    let hold_ms = hold_started.elapsed().as_millis();
    // Both rendering workers run while an input stream is busy with continuous motion.
    let mut latency = Vec::new();
    ia.send(InputEvent::Touch {
        points: vec![(0, 200., 700.)],
    })?;
    for x in 200..260 {
        let sent = Instant::now();
        ia.send(InputEvent::Touch {
            points: vec![(0, x as f32, 700.)],
        })?;
        latency.push(sent.elapsed().as_micros() as u64);
    }
    ia.send(InputEvent::Touch { points: vec![] })?;
    latency.sort_unstable();
    // A dead UI stops heartbeats: the guest releases keys, touch and controller state itself.
    ia.send(InputEvent::Key {
        code: 32,
        down: true,
        meta: 0,
    })?;
    ia.send(InputEvent::Touch {
        points: vec![(0, 200., 700.)],
    })?;
    ia.send(
        GamepadState {
            buttons: 4,
            axes: [0.5, 0., 0., 0., 0., 0., 0., 0.],
        }
        .into(),
    )?;
    thread::sleep(Duration::from_millis(2000));
    let watchdog = events(&a)?;
    ensure!(
        has(&watchdog, "key", 32, 1) && has(&watchdog, "key", 99, 1),
        "Watchdog left held buttons"
    );
    ensure!(
        watchdog
            .iter()
            .rev()
            .take(8)
            .any(|e| e["kind"] == "touch" && e["action"] == 3),
        "Watchdog left contact down"
    );
    ia.send(InputEvent::Focus)?;
    // Closing an input stream releases held state without waiting for the inactivity timer.
    ia.send(InputEvent::Key {
        code: 33,
        down: true,
        meta: 0,
    })?;
    drop(ia);
    thread::sleep(Duration::from_millis(100));
    ensure!(
        has(&events(&a)?, "key", 33, 1),
        "Closing input left a key down"
    );
    let mut ia = a.open_input()?;
    thread::sleep(Duration::from_millis(100));
    let ea = events(&a)?;
    let eb = events(&b)?;
    let renderer = store.runtime_status()?.effective_renderer;
    let out = root
        .join("evidence/g3")
        .join(if renderer == Some(glidepod_core::Renderer::Metal) {
            "metal-input"
        } else {
            "software-input"
        });
    fs::create_dir_all(&out)?;
    fs::write(out.join("a-events.json"), serde_json::to_vec_pretty(&ea)?)?;
    fs::write(out.join("b-events.json"), serde_json::to_vec_pretty(&eb)?)?;
    ensure!(
        has(&ea, "key", 51, 0) && has(&ea, "key", 51, 1) && !has(&eb, "key", 51, 0),
        "Keyboard leaked or missing"
    );
    ensure!(
        ea.iter().any(|e| e["key"] == 51 && e["repeat"] == 1),
        "Key repeat absent"
    );
    ensure!(
        has(&eb, "key", 52, 0) && !has(&ea, "key", 52, 0),
        "Second keyboard leaked or missing"
    );
    ensure!(
        has(&ea, "key", 96, 0) && has(&ea, "key", 96, 1) && !has(&eb, "key", 96, 0),
        "Gamepad A leaked or missing"
    );
    ensure!(
        has(&eb, "key", 97, 0) && has(&eb, "key", 97, 1) && !has(&ea, "key", 97, 0),
        "Gamepad B leaked or missing"
    );
    ensure!(
        ea.iter()
            .any(|e| e["kind"] == "touch" && e["action"] == 5 && e["pointers"] == 2),
        "Multitouch missing"
    );
    ensure!(
        ea.iter().any(|e| e["kind"] == "touch" && e["action"] == 3),
        "Touch cancel missing"
    );
    ensure!(has(&ea, "key", 29, 1), "Held key not released");
    ensure!(
        ea.iter().any(|e| e["kind"] == "motion" && e["action"] == 8),
        "Mouse wheel missing"
    );
    for (w, ev) in [(&a, &ea), (&b, &eb)] {
        ensure!(
            ev.iter().all(|e| e["display"] == w.info.display_id),
            "Display identity mismatch"
        );
    }
    let w: Vec<_> = ea
        .iter()
        .filter(|e| e["kind"] == "key" && e["key"] == 51)
        .collect();
    ensure!(
        w.len() == 3 && w.iter().all(|e| e["down_time"] == w[0]["down_time"]),
        "Key hold/repeat time changed"
    );
    let index = |code, action| {
        ea.iter()
            .position(|e| e["kind"] == "key" && e["key"] == code && e["action"] == action)
            .unwrap()
    };
    ensure!(
        index(51, 0) < index(38, 0) && index(38, 0) < index(51, 1) && index(51, 1) < index(38, 1),
        "Keyboard rollover order lost"
    );
    ensure!(
        ea.iter()
            .any(|e| e["key"] == 29 && e["action"] == 0 && e["meta"] == 1),
        "Shift modifier missing"
    );
    let axes_match = |e: &serde_json::Value, wanted: &[f64]| {
        e["axes"].as_array().is_some_and(|a| {
            a.iter()
                .zip(wanted)
                .all(|(v, w)| (v.as_f64().unwrap_or(999.) - w).abs() < 0.001)
        })
    };
    ensure!(
        ea.iter()
            .any(|e| e["source"] == 16777232
                && axes_match(e, &[0.5, -0.7, 0.2, 0.3, 0.4, 0.8, -1., 1.])),
        "Controller analog values changed"
    );
    ensure!(
        ea.iter()
            .rev()
            .find(|e| e["source"] == 16777232)
            .is_some_and(|e| axes_match(e, &[0.; 8])),
        "Controller axes did not neutralize"
    );
    ensure!(
        ea.iter().any(|e| e["source"] == 8194
            && e["action"] == 8
            && axes_match(e, &[350., 700., 0., 0., 0., 0., 0., 0., 1., -0.5])),
        "Horizontal/vertical scroll axes changed"
    );
    ensure!(
        ea.iter().any(|e| e["source"] == 16777232
            && e["device"].as_i64().is_some_and(|id| id >= 0)
            && e["joystick_device"] == true
            && e["x_range"] == true),
        "Controller device identity/ranges lost"
    );
    let mut sequences = [0, 0];
    let mut counts = [0, 0];
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        ia.send(InputEvent::Ping)?;
        ib.send(InputEvent::Ping)?;
        for (i, w) in [&a, &b].into_iter().enumerate() {
            if let Some(frame) = w.frame(sequences[i])? {
                sequences[i] = frame.sequence;
                counts[i] += 1;
                if counts[i] == 1 {
                    use std::io::Write;
                    let mut capture = std::io::BufWriter::new(fs::File::create(
                        out.join(format!("display-{i}.ppm")),
                    )?);
                    write!(capture, "P6\n{} {}\n255\n", frame.width, frame.height)?;
                    for rgba in frame.rgba.as_chunks::<4>().0 {
                        capture.write_all(&rgba[..3])?;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(30));
    }
    ensure!(
        counts.iter().all(|n| *n >= 8),
        "Independent render stalled: {counts:?}"
    );
    fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"epoch":guest.endpoints.epoch,"displays":[a.info.clone(),b.info.clone()],"frame_updates_2s":counts,"input_batch_ms":elapsed.as_millis(),"guest_gamepad_device":ia.gamepad_device,"renderer":renderer,"hold_ms":hold_ms,"motion_ack_us":{"p50":latency[30],"p95":latency[57],"max":latency[59],"samples":60},"checks":["separate app displays and independent frame sequences","display-targeted held keys and repeat","controller buttons and analog axes","real controller device identity and motion ranges","keyboard rollover and modifiers","touch down delivered before 800ms hold completes","two contacts and pointer transitions","native mouse buttons and wheel","focus reset releases keys, touch and controller","watchdog releases on missing heartbeat","close releases held keys","duplicate input stream rejected"],"native_hardware_verified":false,"passed":true}),
        )?,
    )?;
    Ok(())
}
