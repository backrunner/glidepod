//! Opt-in measurements against an explicitly selected disposable, running guest.
use anyhow::{Context, Result, ensure};
use glidepod_runtime::{Store, display::PixelFormat, input::InputEvent};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn timing(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    json!({"samples": values.len(), "p50_ms": values[values.len()/2],
        "p95_ms": values[values.len()*95/100], "max_ms": values.last(),
        "mean_ms": values.iter().sum::<f64>() / values.len() as f64})
}
fn measure(
    store: &Store,
    label: &str,
    mut operation: impl FnMut() -> Result<()>,
    count: usize,
) -> Result<serde_json::Value> {
    eprintln!("Measuring {label}");
    let pid = store
        .runtime_status()?
        .engine_pid
        .context("Missing engine PID")?;
    let executable = store
        .experimental_runtime()?
        .context("Missing engine")?
        .engine;
    let mut sampler = glidepod_macos::process_usage::Sampler::default();
    sampler.sample(pid, &executable)?;
    let wall = Instant::now();
    let mut values = Vec::new();
    for i in 0..count + 10 {
        let start = Instant::now();
        operation()?;
        if i >= 10 {
            values.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    let usage = sampler.sample(pid, &executable)?;
    let mut result = timing(values);
    result["vmm_usage"] = serde_json::to_value(usage)?;
    result["wall_seconds_including_warmup"] = json!(wall.elapsed().as_secs_f64());
    Ok(result)
}

#[test]
#[ignore = "requires GLIDEPOD_TEST_STORE and GLIDEPOD_PERF_OUTPUT; run without concurrent builds"]
fn host_paths() -> Result<()> {
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
    let out = PathBuf::from(std::env::var_os("GLIDEPOD_PERF_OUTPUT").context("Missing output")?);
    ensure!(!out.exists(), "Never overwrite earlier measurements");
    let store = Store::new(data)?;
    let guest = store.guest()?;
    let status = store.runtime_status()?;
    guest.install(&root.join("target/android/glidepod-fixture.apk"), None)?;
    let window = Arc::new(guest.open_app("dev.glidepod.fixture", 720, 1080)?);
    std::thread::sleep(Duration::from_secs(1));
    let mut input = window.open_input()?;
    input.send(InputEvent::Focus)?;
    let epoch = measure(&store, "epoch", || guest.verify_epoch(), 300)?;
    let file_events = measure(
        &store,
        "file_events",
        || guest.file_events().map(|_| ()),
        30,
    )?;
    let clipboard = measure(&store, "clipboard", || guest.clipboard().map(|_| ()), 30)?;
    let ping = measure(&store, "input_ping", || input.send(InputEvent::Ping), 180)?;
    let mut sequence = 0;
    let mut fresh = Vec::new();
    let mut stale = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        let now = Instant::now();
        if let Some(frame) = window.frame_in_format(sequence, PixelFormat::Bgra)? {
            sequence = frame.sequence;
            fresh.push(now.elapsed().as_secs_f64() * 1000.);
        } else {
            stale.push(now.elapsed().as_secs_f64() * 1000.);
        }
        std::thread::sleep(Duration::from_millis(16).saturating_sub(now.elapsed()));
    }
    ensure!(
        !fresh.is_empty() && !stale.is_empty(),
        "Need both fresh and unchanged frames"
    );
    fs::write(
        out,
        serde_json::to_vec_pretty(&json!({"runtime":status,
        "epoch":epoch, "file_events":file_events, "clipboard":clipboard,
        "input_ping":ping, "fresh_frame":timing(fresh), "unchanged_frame":timing(stale),
        "native_presentation_measured":false}))?,
    )?;
    Ok(())
}

#[test]
#[ignore = "requires the disposable performance guest; run alone with --exact polling_cadence"]
fn polling_cadence() -> Result<()> {
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
    let out = PathBuf::from(std::env::var_os("GLIDEPOD_PERF_OUTPUT").context("Missing output")?);
    ensure!(!out.exists(), "Never overwrite measurements");
    let store = Store::new(data)?;
    let guest = store.guest()?;
    guest.install(
        &root.join("target/android/glidepod-fixture-secondary.apk"),
        None,
    )?;
    let _a = guest.open_app("dev.glidepod.fixture", 720, 1080)?;
    let _b = guest.open_app("dev.glidepod.fixture.secondary", 800, 1000)?;
    std::thread::sleep(Duration::from_secs(2));
    let pid = store
        .runtime_status()?
        .engine_pid
        .context("Missing engine PID")?;
    let exe = store
        .experimental_runtime()?
        .context("Missing engine")?
        .engine;
    let mut sampler = glidepod_macos::process_usage::Sampler::default();
    sampler.sample(pid, &exe)?;
    let start = Instant::now();
    let counts = std::thread::scope(|scope| -> Result<Vec<usize>> {
        let workers = (0..2)
            .map(|index| {
                let guest = guest.clone();
                scope.spawn(move || -> Result<usize> {
                    let mut count = 0;
                    while start.elapsed() < Duration::from_secs(20) {
                        guest.file_events()?;
                        if index == 0 {
                            guest.clipboard()?;
                        }
                        count += 1;
                        // Match the existing per-window worker: wait after each completed cycle.
                        std::thread::sleep(Duration::from_millis(350));
                    }
                    Ok(count)
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|w| w.join().expect("Poll worker panicked"))
            .collect()
    })?;
    let usage = sampler.sample(pid, &exe)?;
    fs::write(
        out,
        serde_json::to_vec_pretty(
            &json!({"scope":"two guest displays and per-window provider polling; no GPUI presentation",
        "wall_seconds":start.elapsed().as_secs_f64(), "completed_cycles_per_window":counts, "vmm_usage":usage}),
        )?,
    )?;
    Ok(())
}
