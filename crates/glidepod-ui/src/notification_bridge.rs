//! UI client of the notification agent; closing every window leaves delivery intact.
use glidepod_runtime::{Store, notifications::AndroidNotification};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
pub struct NotificationBridge {
    pub status: Arc<Mutex<String>>,
    pub runtime: Arc<Mutex<String>>,
    pub opens: Arc<Mutex<Vec<(u64, AndroidNotification)>>>,
}
impl NotificationBridge {
    pub fn new(store: Store, _incoming: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        let status = Arc::new(Mutex::new("Connecting to background service…".into()));
        let opens = Arc::new(Mutex::new(Vec::new()));
        let runtime = Arc::new(Mutex::new(String::new()));
        let runtime_output = runtime.clone();
        let output = status.clone();
        let pending = opens.clone();
        std::thread::spawn(move || {
            loop {
                let result = store
                    .ensure_background()
                    .and_then(|_| store.background_events());
                match result {
                    Ok(s) => {
                        *output.lock().unwrap() = s.notifications;
                        let memory = s
                            .memory
                            .as_ref()
                            .map(|m| {
                                format!(
                                    " Android memory budget: {} MiB.",
                                    s.runtime
                                        .resources
                                        .as_ref()
                                        .map(|r| r.memory_mib)
                                        .unwrap_or(0)
                                        .saturating_sub(m.actual_mib)
                                )
                            })
                            .unwrap_or_default();
                        *runtime_output.lock().unwrap() = format!("{}{memory}", s.reason);
                        let mut q = pending.lock().unwrap();
                        let capacity = 32usize.saturating_sub(q.len());
                        q.extend(s.opens.into_iter().take(capacity));
                    }
                    Err(e) => {
                        let message = format!("Background service: {e:#}");
                        *output.lock().unwrap() = message.clone();
                        *runtime_output.lock().unwrap() = message;
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
        Self {
            status,
            runtime,
            opens,
        }
    }
}
