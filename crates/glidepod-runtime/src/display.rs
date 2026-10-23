//! Per-app guest displays and bounded copied frames. This is not zero-copy GPU sharing.
use crate::{
    adb::quote,
    guest::{Guest, package_id},
};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use fs2::FileExt as LockExt;
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{FileExt, MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::PathBuf,
    sync::{Mutex, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn control(guest: &Guest, command: &str) -> Result<String> {
    guest.verify_epoch()?;
    let mut socket = UnixStream::connect(&guest.endpoints.control)?;
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    socket.set_write_timeout(Some(Duration::from_secs(3)))?;
    writeln!(socket, "{command}")?;
    let mut reply = String::new();
    BufReader::new(socket).take(4097).read_line(&mut reply)?;
    ensure!(
        reply.trim() == "ok",
        "Display control failed: {}",
        reply.trim()
    );
    Ok(reply)
}
#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub package_id: String,
    pub scanout: u32,
    pub display_id: u32,
    pub epoch: u64,
    pub width: u32,
    pub height: u32,
}
pub struct AppWindow {
    pub guest: Guest,
    pub info: WindowInfo,
    _slot: File,
    _package: File,
    source: Mutex<Option<FrameSource>>,
    pub(crate) input_open: AtomicBool,
}
struct FrameSource {
    file: File,
    generation: u32,
}
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub sequence: u64,
    pub rgba: Vec<u8>,
}
/// The copied GPUI presentation path uses BGRA pixels. Keep diagnostic work identical to UI work.
pub fn swap_red_blue(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
}
impl Guest {
    pub fn open_app(&self, package: &str, width: u32, height: u32) -> Result<AppWindow> {
        package_id(package)?;
        self.prepare_input()?;
        ensure!(
            (320..=2048).contains(&width) && (320..=2048).contains(&height),
            "Display size must be between 320 and 2048 pixels"
        );
        let locks = self.store.root().join("developer/windows");
        fs::create_dir_all(&locks)?;
        let lock = |name: &str| -> Result<File> {
            let f = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(locks.join(name))?;
            f.try_lock_exclusive()?;
            Ok(f)
        };
        let package_lock =
            lock(&format!("{package}.lock")).context("This app already has an open window")?;
        let (scanout, slot) = (1..16)
            .find_map(|n| lock(&format!("display-{n}.lock")).ok().map(|f| (n, f)))
            .context("All 15 app displays are in use")?;
        let component = self.shell(&format!("cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.LAUNCHER {}", quote(package)))?;
        let component = component
            .lines()
            .last()
            .context("App has no launcher activity")?
            .trim();
        ensure!(
            component.starts_with(&format!("{package}/"))
                && !component.contains(char::is_whitespace),
            "App has no launcher activity"
        );
        self.shell("su 0 cmd overlay fabricate --target android --name glidepod_displays android:bool/config_localDisplaysMirrorContent 0x12 0x0 && su 0 cmd overlay enable com.android.shell:glidepod_displays")?;
        self.shell("wm dismiss-keyguard")?;
        control(self, &format!("display {scanout} {width} {height}"))?;
        let mut window = AppWindow {
            guest: self.clone(),
            info: WindowInfo {
                package_id: package.into(),
                scanout,
                display_id: 0,
                epoch: self.endpoints.epoch,
                width,
                height,
            },
            _slot: slot,
            _package: package_lock,
            source: Mutex::new(None),
            input_open: AtomicBool::new(false),
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let physical = self.shell("dumpsys SurfaceFlinger --display-id")?;
            let port = format!("port={scanout}");
            let id = physical
                .lines()
                .find(|line| line.split_whitespace().any(|s| s == port))
                .and_then(|line| line.split_whitespace().nth(1));
            if let Some(id) = id {
                let displays = self.shell("cmd display get-displays")?;
                if let Some(display) = logical_display(&displays, &format!("local:{id}")) {
                    window.info.display_id = display;
                    break;
                }
            }
            ensure!(
                Instant::now() < deadline,
                "Android did not create an independent app display"
            );
            std::thread::sleep(Duration::from_millis(150));
        }
        self.shell(&format!("wm density 240 -d {}", window.info.display_id))?;
        let result = self.shell(&format!(
            "am start -W --display {} -f 0x18000000 -n {}",
            window.info.display_id,
            quote(component)
        ))?;
        ensure!(
            !result.contains("Error:"),
            "Android could not open the app: {result}"
        );
        Ok(window)
    }
}
fn logical_display(displays: &str, unique: &str) -> Option<u32> {
    displays
        .lines()
        .find(|line| {
            line.starts_with("Display id ") && line.contains(&format!("uniqueId \"{unique}\""))
        })?
        .strip_prefix("Display id ")?
        .split(':')
        .next()?
        .parse()
        .ok()
}
impl AppWindow {
    /// Display-targeted input avoids the global keyboard and touch device focus race.
    pub fn tap(&self, x: u32, y: u32) -> Result<()> {
        ensure!(
            x < self.info.width && y < self.info.height,
            "Input outside app canvas"
        );
        self.guest
            .shell(&format!("input -d {} tap {x} {y}", self.info.display_id))?;
        Ok(())
    }
    pub fn swipe(&self, from: (u32, u32), to: (u32, u32), millis: u32) -> Result<()> {
        ensure!(
            from.0 < self.info.width
                && to.0 < self.info.width
                && from.1 < self.info.height
                && to.1 < self.info.height
                && millis <= 2000,
            "Invalid pointer gesture"
        );
        self.guest.shell(&format!(
            "input -d {} swipe {} {} {} {} {}",
            self.info.display_id,
            from.0,
            from.1,
            to.0,
            to.1,
            millis.max(1)
        ))?;
        Ok(())
    }
    pub fn key(&self, android_key: u32) -> Result<()> {
        ensure!(android_key <= 304, "Invalid Android key");
        self.guest.shell(&format!(
            "input -d {} keyevent {android_key}",
            self.info.display_id
        ))?;
        Ok(())
    }
    pub fn text(&self, text: &str) -> Result<()> {
        ensure!(
            text.len() <= 16384 && !text.contains('\0'),
            "Text exceeds the input limit"
        );
        let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        let command = format!(
            "am broadcast --receiver-foreground -a dev.glidepod.input.COMMIT -p dev.glidepod.input --es package {} --es text {}",
            quote(&self.info.package_id),
            quote(&encoded)
        );
        // Window focus reaches the IME asynchronously. Retry only explicit rejection;
        // a successful commit is never repeated, and every attempt rechecks the package.
        let deadline = Instant::now() + Duration::from_millis(700);
        loop {
            let result = self.guest.shell_timeout(&command, Duration::from_secs(2))?;
            if result.contains("Broadcast completed: result=1") {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Click an editable field in this app before typing or pasting"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
    pub fn edit_action(&self, action: &str) -> Result<Option<String>> {
        ensure!(
            ["copy", "cut", "select_all"].contains(&action),
            "Invalid edit action"
        );
        let reply = self.guest.shell(&format!("am broadcast --receiver-foreground -a dev.glidepod.input.COMMIT -p dev.glidepod.input --es package {} --es action {}",quote(&self.info.package_id),quote(action)))?;
        ensure!(
            reply.contains("Broadcast completed: result=1"),
            "Select text in an editable field before using this shortcut"
        );
        if action == "select_all" {
            return Ok(None);
        }
        let encoded = reply
            .split("data=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .context("No selected text returned")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        Ok(Some(String::from_utf8(bytes)?))
    }
    pub fn frame(&self, last_sequence: u64) -> Result<Option<Frame>> {
        self.guest.verify_epoch()?;
        let mut source = self
            .source
            .lock()
            .map_err(|_| anyhow::anyhow!("Frame reader failed"))?;
        // A new layout unlinks the previous generation. Re-subscribe instead of
        // keeping an open descriptor to an indefinitely frozen old frame.
        if source
            .as_ref()
            .is_some_and(|s| s.file.metadata().map(|m| m.nlink() == 0).unwrap_or(true))
        {
            *source = None;
        }
        if source.is_none() {
            let mut socket = UnixStream::connect(&self.guest.endpoints.control)?;
            socket.set_read_timeout(Some(Duration::from_secs(2)))?;
            writeln!(socket, "subscribe")?;
            let mut reader = BufReader::new(socket);
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let mut line = String::new();
                reader.by_ref().take(8193).read_line(&mut line)?;
                ensure!(
                    !line.is_empty() && line.len() <= 8192 && Instant::now() < deadline,
                    "No app display frames available"
                );
                let fields = line.trim_end().splitn(8, ' ').collect::<Vec<_>>();
                if fields.len() == 8
                    && fields[0] == "frames"
                    && fields[1].parse::<u32>()? == self.info.scanout
                {
                    let path = PathBuf::from(fields[7]).canonicalize()?;
                    ensure!(
                        path.starts_with(self.guest.endpoints.frames.canonicalize()?),
                        "Frame path outside owned instance"
                    );
                    *source = Some(FrameSource {
                        file: File::open(path)?,
                        generation: fields[2].parse()?,
                    });
                    break;
                }
            }
        }
        let source = source.as_ref().unwrap();
        read_frame(&source.file, source.generation, last_sequence)
    }
}
impl Drop for AppWindow {
    fn drop(&mut self) {
        let _ = control(&self.guest, &format!("display {} off", self.info.scanout));
    }
}
fn read_frame(file: &File, generation: u32, previous: u64) -> Result<Option<Frame>> {
    let mut header = [0u8; 64];
    file.read_exact_at(&mut header, 0)?;
    let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let sequence = u64::from_le_bytes(header[48..56].try_into().unwrap());
    ensure!(
        &header[..4] == b"AVOF" && word(4) == 1 && word(8) == generation,
        "Frame generation changed"
    );
    let (width, height, stride, format, buffers, offset, size, index) = (
        word(12),
        word(16),
        word(20),
        word(24),
        word(28),
        word(32),
        word(36),
        word(40),
    );
    ensure!(
        width > 0
            && height > 0
            && width <= 4096
            && height <= 4096
            && stride == width * 4
            && format <= 1
            && buffers == 3
            && offset == 4096
            && size >= stride * height
            && size <= 64 * 1024 * 1024
            && index < 3,
        "Invalid frame layout"
    );
    if sequence == 0 || sequence == previous || index as u64 != sequence % 3 {
        return Ok(None);
    }
    let mut rgba = vec![0; (stride * height) as usize];
    file.read_exact_at(
        &mut rgba,
        u64::from(offset) + u64::from(index) * u64::from(size),
    )?;
    let mut after = [0u8; 64];
    file.read_exact_at(&mut after, 0)?;
    if after != header {
        return Ok(None);
    }
    if format == 0 {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    Ok(Some(Frame {
        width,
        height,
        sequence,
        rgba,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_identity_is_exact() {
        let s = "Display id 2: DisplayInfo{uniqueId \"local:123\"}\nDisplay id 3: DisplayInfo{uniqueId \"local:1234\"}";
        assert_eq!(logical_display(s, "local:123"), Some(2));
        assert_eq!(logical_display(s, "local:12"), None);
    }
    #[test]
    fn corrupt_frame_does_not_allocate() {
        let mut f = tempfile::tempfile().unwrap();
        f.write_all(&[255; 64]).unwrap();
        assert!(read_frame(&f, 1, 0).is_err());
    }
}
