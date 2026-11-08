//! Background-owned relay without a private history; Notification Center owns retention.
use crate::{Store, notifications::AndroidNotification};
use glidepod_core::VmState;
use glidepod_macos::notifications::command;
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct NotificationBridge {
    pub status: Arc<Mutex<String>>,
    pub opens: Arc<Mutex<Vec<(u64, AndroidNotification)>>>,
}
fn matches_event(
    event: &serde_json::Value,
    epoch: u64,
    hash: &str,
    n: &AndroidNotification,
) -> bool {
    event["epoch"] == epoch
        && event["id"] == n.host_id(epoch)
        && event["package_id"] == n.package_id
        && event["version"] == hash
}
impl NotificationBridge {
    pub fn new(store: Store, _incoming: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        let status = Arc::new(Mutex::new("Notification forwarding is off.".into()));
        let output = status.clone();
        let opens = Arc::new(Mutex::new(Vec::new()));
        let pending = opens.clone();
        std::thread::spawn(move || {
            let mut epoch = 0;
            let mut seen = HashMap::<String, (String, AndroidNotification)>::new();
            let mut authorization = -1;
            let mut checked = Instant::now() - Duration::from_secs(10);
            let mut cleared = false;
            let mut revoked_epoch = None;
            let mut baseline_ms = None;
            let mut prepared_epoch = None;
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
                        prepared_epoch = None;
                        return Ok("Notification forwarding is off.".into());
                    }
                    revoked_epoch = None;
                    if running.is_none() {
                        return Ok("Start Android to receive system notifications.".into());
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
                                seen.retain(|_, (_, n)| n.host_id(epoch) != id);
                            }
                            delivery_error = Some(anyhow::anyhow!("{}", event["message"]));
                            continue;
                        }
                        let key = event["key"].as_str().unwrap_or("");
                        let Some((hash, n)) = seen.get(key) else {
                            continue;
                        };
                        if !matches_event(event, epoch, hash, n) {
                            continue;
                        }
                        if event["kind"] == "open" {
                            if n.can_open {
                                let mut q = pending.lock().unwrap();
                                if q.len() < 32 {
                                    q.push((epoch, n.clone()));
                                }
                            }
                        } else if event["kind"] == "dismiss"
                            && n.clearable
                            && let Err(e) = guest.dismiss_notification(n)
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
                    if prepared_epoch != Some(epoch) {
                        guest.set_notification_access(true)?;
                        prepared_epoch = Some(epoch);
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
                        .map(|(_, (_, n))| n.host_id(epoch))
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
                        .filter(|(key, n)| {
                            seen.get(*key)
                                .is_none_or(|(hash, _)| hash != &n.content_hash())
                        })
                        .collect::<Vec<_>>();
                    changed.sort_by_key(|(key, n)| (seen.contains_key(*key), n.posted_ms));
                    // Bounded delivery; initial Android history enters Notification Center quietly.
                    for (key, n) in changed.into_iter().take(8) {
                        command(
                            json!({"op":"show","id":n.host_id(epoch),"epoch":epoch,"version":n.content_hash(),"key":n.key,"package_id":n.package_id,"app_label":n.app_label,"title":n.title,"body":n.body,"clearable":n.clearable,"group":n.group,"alerting":n.alerting && !n.ongoing && !n.group_summary && !seen.contains_key(key) && n.posted_ms>baseline}),
                        )?;
                        let mut identity = n.clone();
                        identity.title = String::new();
                        identity.body = String::new();
                        seen.insert(key.clone(), (n.content_hash(), identity));
                    }
                    Ok("Forwarding all available Android notifications to this Mac.".into())
                };
                *output.lock().unwrap() = match cycle() {
                    Ok(s) => s,
                    Err(e) => format!("Notifications: {e:#}"),
                };
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        Self { status, opens }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_actions_must_match_the_delivered_notification_version() {
        let mut n: AndroidNotification = serde_json::from_value(json!({
            "key":"0|test.app|1", "package_id":"test.app", "app_label":"Test",
            "title":"Original", "body":"First version", "posted_ms":100,
            "clearable":true, "redacted":false, "alerting":true, "ongoing":false,
            "group_summary":false, "group":"test.app", "user_id":0, "can_open":true
        }))
        .unwrap();
        let event = json!({"epoch":12, "id":n.host_id(12), "package_id":n.package_id,
            "version":n.content_hash()});
        assert!(matches_event(&event, 12, &n.content_hash(), &n));
        assert!(!matches_event(&event, 13, &n.content_hash(), &n));
        n.body = "Replaced content".into();
        assert!(!matches_event(&event, 12, &n.content_hash(), &n));
        let mut missing = event;
        missing.as_object_mut().unwrap().remove("version");
        assert!(!matches_event(&missing, 12, &n.content_hash(), &n));
    }
}
