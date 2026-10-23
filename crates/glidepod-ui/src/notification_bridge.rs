//! App-lifetime notification relay. No notification bodies are written to disk.
use glidepod_core::VmState;
use glidepod_macos::notifications::command;
use glidepod_runtime::{Store, notifications::AndroidNotification};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct NotificationBridge {
    pub status: Arc<Mutex<String>>,
}
impl NotificationBridge {
    pub fn new(store: Store, incoming: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        let status = Arc::new(Mutex::new("Notification forwarding is off.".into()));
        let output = status.clone();
        std::thread::spawn(move || {
            let mut epoch = 0;
            let mut seen = HashMap::<String, AndroidNotification>::new();
            let mut authorization = -1;
            let mut checked = Instant::now() - Duration::from_secs(10);
            let mut cleared = false;
            let mut revoked_epoch = None;
            let mut baseline_ms = None;
            loop {
                let mut cycle = || -> anyhow::Result<String> {
                    let config = store.load()?;
                    if checked.elapsed() >= Duration::from_secs(5) {
                        authorization = command(json!({"op":"status"}))?["authorization"]
                            .as_i64()
                            .unwrap_or(-1);
                        checked = Instant::now();
                    }
                    if !cleared {
                        command(json!({"op":"clear"}))?;
                        cleared = true;
                    }
                    let runtime = store.runtime_status()?;
                    let running = if let VmState::Running { epoch } = runtime.vm {
                        Some(epoch)
                    } else {
                        None
                    };
                    if !config.notifications_enabled || running != Some(epoch) {
                        if !seen.is_empty() {
                            command(json!({"op":"clear"}))?;
                            seen.clear();
                        }
                        epoch = running.unwrap_or(0);
                        baseline_ms = None;
                    }
                    if !config.notifications_enabled {
                        if running.is_some() && revoked_epoch != running {
                            let g = store.guest()?;
                            if g.notification_access_enabled()? {
                                g.set_notification_access(false)?;
                            }
                            revoked_epoch = running;
                        }
                        return Ok("Notification forwarding is off.".into());
                    }
                    revoked_epoch = None;
                    if running.is_none() {
                        return Ok("Start Android to receive app notifications.".into());
                    }
                    let guest = store.guest()?;
                    let mut delivery_error = None;
                    for event in command(json!({"op":"events"}))?
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        if event["kind"] == "error" {
                            if let Some(id) = event["id"].as_str() {
                                seen.retain(|_, n| n.host_id(epoch) != id);
                            }
                            delivery_error = Some(anyhow::anyhow!("{}", event["message"]));
                            continue;
                        }
                        let key = event["key"].as_str().unwrap_or("");
                        let Some(n) = seen.get(key) else {
                            continue;
                        };
                        if event["epoch"] != epoch
                            || event["id"] != n.host_id(epoch)
                            || event["package_id"] != n.package_id
                        {
                            continue;
                        }
                        if event["kind"] == "open" {
                            let mut q = incoming.lock().unwrap();
                            if q.len() < 32 {
                                q.push(PathBuf::from(format!(
                                    "glidepod://launch/{}",
                                    n.package_id
                                )));
                            }
                        } else if event["kind"] == "dismiss"
                            && n.clearable
                            && let Err(e) = guest.dismiss_notification(key)
                        {
                            delivery_error = Some(e);
                        }
                    }
                    if let Some(e) = delivery_error {
                        return Err(e);
                    }
                    if authorization != 2 && authorization != 3 {
                        return Ok(if authorization == 0 {
                            "Allow notifications in the macOS permission prompt."
                        } else {
                            "Notifications are disabled in macOS System Settings."
                        }
                        .into());
                    }
                    let snapshot = guest.notifications()?;
                    if !snapshot.connected {
                        if !guest.notification_access_enabled()? {
                            command(json!({"op":"clear"}))?;
                            seen.clear();
                            return Ok("Android notification access is off. Turn forwarding off and on to reconnect.".into());
                        }
                        return Ok("Waiting for Android notification access…".into());
                    }
                    let next: HashMap<_, _> = snapshot
                        .items
                        .into_iter()
                        .map(|n| (n.key.clone(), n))
                        .collect();
                    let removed: Vec<_> = seen
                        .iter()
                        .filter(|(k, _)| !next.contains_key(*k))
                        .map(|(_, n)| n.host_id(epoch))
                        .collect();
                    if !removed.is_empty() {
                        command(json!({"op":"remove","ids":removed}))?;
                    }
                    seen.retain(|key, _| next.contains_key(key));
                    let baseline = *baseline_ms.get_or_insert_with(|| {
                        next.values().map(|n| n.posted_ms).max().unwrap_or(0)
                    });
                    let mut changed = next
                        .iter()
                        .filter(|(key, n)| seen.get(*key) != Some(*n))
                        .collect::<Vec<_>>();
                    changed.sort_by_key(|(_, n)| n.posted_ms);
                    // Bounded delivery; initial Android history enters Notification Center quietly.
                    for (key, n) in changed.into_iter().take(8) {
                        command(
                            json!({"op":"show","id":n.host_id(epoch),"epoch":epoch,"key":n.key,"package_id":n.package_id,"app_label":n.app_label,"title":n.title,"body":n.body,"clearable":n.clearable,"alerting":n.alerting && n.posted_ms>baseline}),
                        )?;
                        seen.insert(key.clone(), n.clone());
                    }
                    Ok("Forwarding Android app notifications to this Mac.".into())
                };
                *output.lock().unwrap() = match cycle() {
                    Ok(s) => s,
                    Err(e) => format!("Notifications: {e:#}"),
                };
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        Self { status }
    }
}
