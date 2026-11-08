//! Bounded, epoch-scoped read-only observations; no cached content or mutation retries.
use crate::{
    adb::{Adb, quote},
    guest::{Endpoints, Guest},
};
use anyhow::{Context, Result, ensure};
use std::time::{Duration, Instant};

const LIMIT: usize = 2 * 1024 * 1024;
const IDLE: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(crate) struct Pool {
    session: Option<Session>,
}
impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderPollPool")
            .field("connected", &self.session.is_some())
            .finish()
    }
}
struct Session {
    endpoints: Endpoints,
    stream: Adb,
    used: Instant,
    #[cfg(test)]
    pid: u64,
}

pub(crate) fn operation(authority: &str, method: &str, arg: Option<&str>) -> Option<&'static str> {
    match (authority, method, arg) {
        ("dev.glidepod.files.bridge", "poll", Some("{}")) => Some("files"),
        ("dev.glidepod.input.clipboard", "read", None) => Some("clipboard"),
        _ => None,
    }
}

fn response(stream: &mut Adb) -> Result<serde_json::Value> {
    let mut bytes = Vec::new();
    loop {
        let chunk = stream
            .read_chunk()?
            .context("Android observation channel closed")?;
        ensure!(
            bytes.len() + chunk.len() <= LIMIT,
            "Provider response exceeds limit"
        );
        bytes.extend(chunk);
        if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
            ensure!(
                end + 1 == bytes.len(),
                "Unexpected trailing provider response"
            );
            return Ok(serde_json::from_slice(&bytes[..end])?);
        }
    }
}

pub(crate) fn query(guest: &Guest, operation: &str) -> Result<String> {
    let mut pool = guest
        .store
        .polls
        .lock()
        .map_err(|_| anyhow::anyhow!("Provider channel failed"))?;
    // Check after acquiring the shared channel so a waiting caller cannot use an old epoch.
    if let Err(error) = guest.verify_epoch() {
        if pool
            .session
            .as_ref()
            .is_some_and(|s| s.endpoints == guest.endpoints)
        {
            pool.session = None;
        }
        return Err(error);
    }
    if pool
        .session
        .as_ref()
        .is_some_and(|s| s.endpoints != guest.endpoints || s.used.elapsed() >= IDLE)
    {
        pool.session = None;
    }
    let result = (|| {
        if pool.session.is_none() {
            let remote = guest.prepare_bridge()?;
            let mut stream = Adb::open(
                &guest.endpoints.vsock,
                &format!(
                    "exec:CLASSPATH={} app_process / dev.glidepod.bridge.Poll",
                    quote(&remote)
                ),
                Duration::from_secs(5),
            )?;
            stream.coalesce_acks();
            let ready = response(&mut stream)?;
            ensure!(ready["ready"] == 1, "Invalid observation protocol");
            pool.session = Some(Session {
                endpoints: guest.endpoints.clone(),
                stream,
                used: Instant::now(),
                #[cfg(test)]
                pid: ready["pid"].as_u64().context("Missing helper PID")?,
            });
        }
        let session = pool.session.as_mut().unwrap();
        session.stream.renew_timeout(Duration::from_secs(2));
        session
            .stream
            .write_all(format!("{operation}\n").as_bytes())?;
        let reply = response(&mut session.stream)?;
        ensure!(
            reply["ok"] == true && reply["value"].is_object(),
            "Android observation failed: {}",
            reply["error"]
        );
        session.used = Instant::now();
        Ok(serde_json::to_string(&reply["value"])?)
    })();
    // Reconnect on the next observation, never hide a failed call or reuse a dead Binder.
    if result.is_err() {
        pool.session = None;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an explicitly selected disposable GLIDEPOD_TEST_STORE"]
    fn real_poll_recovers_from_process_exit_idle_expiry_and_stale_epoch() -> Result<()> {
        use std::path::PathBuf;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let data = PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing store")?)
            .canonicalize()?;
        ensure!(
            data.starts_with(root.join(".cache")),
            "Use a disposable store"
        );
        let store = crate::Store::new(data)?;
        let guest = store.guest()?;
        guest.prepare_input()?;
        guest.file_events()?;
        guest.clipboard()?;
        let pid = || store.polls.lock().unwrap().session.as_ref().unwrap().pid;
        let first = pid();
        // Both read paths and Store clones reuse one helper.
        store.clone().guest()?.file_events()?;
        assert_eq!(first, pid());
        let mut stale = guest.clone();
        stale.endpoints.epoch += 1;
        assert!(stale.file_events().is_err());
        assert_eq!(first, pid());
        guest.developer_shell(&format!("kill -9 {first}"), false)?;
        std::thread::sleep(Duration::from_millis(100));
        assert!(guest.file_events().is_err());
        assert!(store.polls.lock().unwrap().session.is_none());
        guest.file_events()?;
        assert_ne!(first, pid());
        let second = pid();
        // The helper must not retain provider references indefinitely without a caller.
        std::thread::sleep(Duration::from_secs(17));
        let gone = guest.developer_shell(
            &format!("if kill -0 {second} 2>/dev/null; then echo alive; else echo gone; fi"),
            false,
        )?;
        ensure!(
            gone.trim() == "gone",
            "Idle observation helper remained alive"
        );
        guest.clipboard()?;
        assert_ne!(second, pid());
        Ok(())
    }
    #[test]
    fn only_fixed_read_only_operations_are_admitted() {
        assert_eq!(
            operation("dev.glidepod.files.bridge", "poll", Some("{}")),
            Some("files")
        );
        assert_eq!(
            operation("dev.glidepod.input.clipboard", "read", None),
            Some("clipboard")
        );
        for (authority, method, arg) in [
            ("dev.glidepod.input.clipboard", "set", None),
            ("dev.glidepod.files.bridge", "commit", Some("{}")),
            (
                "dev.glidepod.files.bridge",
                "poll",
                Some("{\"extra\":true}"),
            ),
            ("other", "read", None),
        ] {
            assert_eq!(operation(authority, method, arg), None);
        }
    }
}
