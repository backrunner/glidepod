//! A native Dock process per Android window. The window and its guest lease stay
//! in GlidePod so focus, IME and gamepad ownership remain serialized.
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Focus,
    Close,
    Unavailable(String),
}

pub struct Session {
    events: mpsc::Receiver<Event>,
    stopped: Arc<AtomicBool>,
}

/// Short, stable paths fit Darwin's sockaddr_un even for long Android IDs.
pub(crate) fn socket_path(package: &str) -> Result<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    let root = PathBuf::from(format!("/tmp/glidepod-dock-{uid}"));
    match fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(&root)?;
    ensure!(
        meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0,
        "Dock session directory must be private and owned by this user"
    );
    let digest = hex::encode(Sha256::digest(package.as_bytes()));
    Ok(root.join(format!("{}.sock", &digest[..32])))
}

impl Session {
    pub fn start(package: &str, label: &str, icon: Option<&Path>) -> Result<Self> {
        let path = socket_path(package)?;
        let session = Self::listen(&path)?;
        let bundle = crate::launchers::create(package, label, icon)?;
        let output = Command::new("/usr/bin/open")
            .args(["-g", "-a"])
            .arg(bundle)
            .args(["--args", "--glidepod-managed"])
            .output()?;
        ensure!(
            output.status.success(),
            "Cannot open app Dock entry: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(session)
    }

    fn listen(path: &Path) -> Result<Self> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path.with_extension("lock"))?;
        lock.try_lock_exclusive()
            .context("This app already has a Dock session")?;
        // The lock, not a connection probe, establishes that an old socket is stale.
        if path.exists() {
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let (sender, events) = mpsc::sync_channel(16);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let path = path.to_owned();
        std::thread::spawn(move || {
            let _lease = SocketLease { path, _lock: lock };
            let started = Instant::now();
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        serve(stream, &sender, &stop);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => {
                        let _ = sender.try_send(Event::Unavailable(e.to_string()));
                        break;
                    }
                }
                if started.elapsed() > Duration::from_secs(15) {
                    let _ = sender.try_send(Event::Unavailable(
                        "The app's Dock entry did not connect".into(),
                    ));
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        Ok(Self { events, stopped })
    }

    pub fn drain(&self) -> impl Iterator<Item = Event> + '_ {
        self.events.try_iter()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

struct SocketLease {
    path: PathBuf,
    _lock: File,
}
impl Drop for SocketLease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn serve(mut stream: UnixStream, sender: &mpsc::SyncSender<Event>, stop: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    if stream.write_all(b"ready\n").is_err() {
        return;
    }
    let mut pending = Vec::new();
    let mut buffer = [0; 64];
    while !stop.load(Ordering::Acquire) {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                for byte in &buffer[..n] {
                    if *byte == b'\n' {
                        match pending.as_slice() {
                            b"focus" => {
                                let _ = sender.try_send(Event::Focus);
                            }
                            b"quit" => {
                                // Do not drop a quit when the focus queue is full.
                                while !stop.load(Ordering::Acquire) {
                                    match sender.try_send(Event::Close) {
                                        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => break,
                                        Err(mpsc::TrySendError::Full(_)) => {}
                                    }
                                    std::thread::sleep(Duration::from_millis(20));
                                }
                                return;
                            }
                            _ => return,
                        }
                        pending.clear();
                    } else {
                        pending.push(*byte);
                        if pending.len() > 16 {
                            return;
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
    }
    // Force Quit of the Dock helper also closes only its leased app window.
    while !stop.load(Ordering::Acquire) {
        match sender.try_send(Event::Close) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => break,
            Err(mpsc::TrySendError::Full(_)) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connect(path: &Path) -> UnixStream {
        let mut stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut ready = [0; 6];
        stream.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"ready\n");
        stream
    }
    #[test]
    fn commands_are_scoped_and_duplicate_session_cannot_replace_socket() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let a = root.path().join("a.sock");
        let b = root.path().join("b.sock");
        let first = Session::listen(&a).unwrap();
        let second = Session::listen(&b).unwrap();
        assert!(Session::listen(&a).is_err());
        let mut client = connect(&a);
        let _other = connect(&b);
        client.write_all(b"fo").unwrap();
        client.write_all(b"cus\nquit\n").unwrap();
        assert_eq!(
            first.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Focus
        );
        assert_eq!(
            first.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Close
        );
        assert!(second.events.try_recv().is_err());
    }
    #[test]
    fn window_drop_disconnects_helper_and_helper_crash_closes_window() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let path = root.path().join("app.sock");
        let session = Session::listen(&path).unwrap();
        let mut client = connect(&path);
        drop(session);
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
        let deadline = Instant::now() + Duration::from_secs(2);
        while path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let session = Session::listen(&path).unwrap();
        drop(connect(&path));
        assert_eq!(
            session.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Close
        );
    }
}
