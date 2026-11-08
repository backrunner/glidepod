use super::*;
use glidepod_runtime::google_apps::{Action, Progress};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
pub(super) struct GoogleJob {
    pub busy: bool,
    pub message: String,
    pub cancel: Arc<AtomicBool>,
    pub revision: u64,
}
impl Center {
    pub(super) fn manage_google(
        &mut self,
        action: Option<Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.working {
            return;
        }
        if action == Some(Action::Remove) {
            let prompt=window.prompt(PromptLevel::Warning,"Remove Google services?",Some("This removes Google Play services, Play Store and their Android account data. Other Android apps and their data are preserved. Android will stop during removal."),&["Remove","Cancel"],cx);
            cx.spawn(async move |this, cx| {
                if prompt.await == Ok(0) {
                    let _ = this.update(cx, |this, cx| this.start_google_job(action, cx));
                }
            })
            .detach();
        } else {
            self.start_google_job(action, cx);
        }
    }
    fn start_google_job(&mut self, action: Option<Action>, cx: &mut Context<Self>) {
        // A removal confirmation can resolve after another UI operation starts.
        if self.working {
            return;
        }
        let job = cx.global::<AppSession>().google.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut state = job.lock().unwrap();
            if state.busy {
                return;
            }
            state.busy = true;
            state.message = "Preparing Google services…".into();
            state.cancel = cancel.clone();
            state.revision += 1;
        }
        for (_, handle) in self.windows.drain() {
            let _ = handle.update(cx, |_, w, _| w.remove_window());
        }
        self.working = true;
        let store = self.store.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                // Explicitly pause the background owner before system maintenance.
                if store.runtime_status()?.active() {
                    store.stop_background_vm()?;
                }
                if let Some(action) = action {
                    let helper = std::env::current_exe()?.with_file_name("glidepodctl");
                    store.manage_google_apps(&helper, action, &cancel, &mut |p: Progress| {
                        let mut state = job.lock().unwrap();
                        state.message = if let Some(percent) =
                            p.completed.saturating_mul(100).checked_div(p.total)
                        {
                            format!("{} {}%", p.message, percent)
                        } else {
                            p.message
                        };
                        state.revision += 1;
                    })
                } else {
                    store.recover_google_apps()
                }
            })();
            let mut state = job.lock().unwrap();
            state.busy = false;
            state.revision += 1;
            state.message = match result {
                Ok(()) => {
                    if action == Some(Action::Install) {
                        "Google services installed. Start Android and open Play Store to sign in."
                            .into()
                    } else if action == Some(Action::Remove) {
                        "Google services removed.".into()
                    } else if action == Some(Action::Enable) {
                        "Google services are on. Start Android to use them.".into()
                    } else if action == Some(Action::Disable) {
                        "Google services are off. Account and app data are preserved.".into()
                    } else {
                        "Android recovery completed.".into()
                    }
                }
                Err(e) => format!("{e:#}"),
            };
        });
        cx.notify();
    }
    pub(super) fn cancel_google(&self, cx: &mut Context<Self>) {
        cx.global::<AppSession>()
            .google
            .lock()
            .unwrap()
            .cancel
            .store(true, Ordering::Relaxed);
    }
}
