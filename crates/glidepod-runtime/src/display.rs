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
    pub refresh_rate_hz: u32,
}
impl WindowInfo {
    pub fn frame_period(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.refresh_rate_hz))
    }
}
pub struct AppWindow {
    pub guest: Guest,
    pub info: WindowInfo,
    _slot: File,
    _package: File,
    _activity: File,
    source: Mutex<Option<FrameSource>>,
    pub(crate) input_open: AtomicBool,
}
struct FrameSource {
    file: File,
    generation: u32,
}
/// Bounded, persistent notifications; pixels still come from the latest shared frame.
pub struct FrameNotifications {
    reader: BufReader<UnixStream>,
    pending: String,
    scanout: u32,
}
impl FrameNotifications {
    /// Wait without polling pixels. Preserve fragmented lines across timeouts.
    pub fn wait(&mut self, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            self.reader
                .get_mut()
                .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))?;
            let read = self
                .reader
                .by_ref()
                .take((8193 - self.pending.len()) as u64)
                .read_line(&mut self.pending);
            ensure!(
                self.pending.len() <= 8192,
                "Display notification exceeds size limit"
            );
            match read {
                Ok(0) => anyhow::bail!("Display notification stream closed"),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
                Ok(_) => {}
            }
            if !self.pending.ends_with('\n') {
                continue;
            }
            let line = std::mem::take(&mut self.pending);
            let mut fields = line.split_whitespace();
            let event = fields.next();
            if fields.next().and_then(|v| v.parse::<u32>().ok()) != Some(self.scanout) {
                continue;
            }
            match event {
                Some("frame" | "frames") => return Ok(true),
                Some("frames-off") => anyhow::bail!("App display was removed"),
                _ => {}
            }
        }
        Ok(false)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra,
    Rgba,
}
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub sequence: u64,
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
}
/// Convert between packed RGBA and BGRA, preserving green and alpha.
pub fn swap_red_blue(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        // Whole-word operations vectorize without overlapping byte stores. Use
        // explicit byte order so the same lane permutation also works on big endian.
        let word = u32::from_le_bytes(*pixel);
        *pixel =
            ((word & 0xff00ff00) | ((word & 0xff) << 16) | ((word >> 16) & 0xff)).to_le_bytes();
    }
}
impl Guest {
    fn prepare_independent_displays(&self) -> Result<()> {
        // Re-fabricating an unchanged overlay updates Android's asset paths and
        // recreates other activities, invalidating their Vulkan surfaces. Check
        // the effective resource and serialize first-time setup across clients.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.store.root().join("display-configuration.lock"))?;
        LockExt::lock_exclusive(&lock)?;
        self.verify_epoch()?;
        let lookup = "cmd overlay lookup android android:bool/config_localDisplaysMirrorContent";
        if self.shell(lookup)?.trim() != "false" {
            self.shell("su 0 cmd overlay fabricate --target android --name glidepod_displays android:bool/config_localDisplaysMirrorContent 0x12 0x0 && su 0 cmd overlay enable com.android.shell:glidepod_displays")?;
            ensure!(
                self.shell(lookup)?.trim() == "false",
                "Android did not enable independent app displays"
            );
        }
        Ok(())
    }

    pub fn open_app(&self, package: &str, width: u32, height: u32) -> Result<AppWindow> {
        self.open_target(package, width, height, None)
    }
    pub fn open_notification(
        &self,
        n: &crate::notifications::AndroidNotification,
        width: u32,
        height: u32,
    ) -> Result<AppWindow> {
        self.open_target(&n.package_id, width, height, Some(n))
    }
    fn open_target(
        &self,
        package: &str,
        width: u32,
        height: u32,
        notification: Option<&crate::notifications::AndroidNotification>,
    ) -> Result<AppWindow> {
        package_id(package)?;
        let activity = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.store.root().join("activity.lock"))?;
        LockExt::try_lock_shared(&activity)
            .context("Android is entering standby. Open the app again after it stops.")?;
        self.verify_epoch()?;
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
        self.prepare_independent_displays()?;
        self.shell("wm dismiss-keyguard")?;
        // Use the running VM's settings, not a next-start preference edited while
        // it is running. Primary and app displays must have the same timing.
        let refresh_rate_hz = self
            .store
            .runtime_status()?
            .resources
            .context("Running display settings unavailable")?
            .refresh_rate_hz;
        ensure!(
            [60, 90, 120].contains(&refresh_rate_hz),
            "Invalid running display rate"
        );
        control(
            self,
            &format!("display {scanout} {width} {height} {refresh_rate_hz}"),
        )?;
        let mut window = AppWindow {
            guest: self.clone(),
            info: WindowInfo {
                package_id: package.into(),
                scanout,
                display_id: 0,
                epoch: self.endpoints.epoch,
                width,
                height,
                refresh_rate_hz,
            },
            _slot: slot,
            _package: package_lock,
            _activity: activity,
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
        if let Some(notification) = notification {
            self.activate_notification(notification, window.info.display_id)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let out = self.bridge(
                    "dev.glidepod.bridge.Tasks",
                    &["owner", &window.info.display_id.to_string(), package],
                )?;
                let owner: serde_json::Value = serde_json::from_str(out.trim())?;
                if let Some(target) = owner["package"].as_str().filter(|s| !s.is_empty()) {
                    package_id(target)?;
                    window.info.package_id = target.into();
                    break;
                }
                ensure!(
                    Instant::now() < deadline,
                    "The Android notification action did not open a visible activity"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        } else {
            let component=self.shell(&format!("cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.LAUNCHER {}",quote(package)))?;
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
            let result = self.shell(&format!(
                "am start -W --display {} -f 0x18000000 -n {}",
                window.info.display_id,
                quote(component)
            ))?;
            ensure!(
                !result.contains("Error:"),
                "Android could not open the app: {result}"
            );
        }
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
    /// Count only this package's tasks on its leased display, never the default display.
    pub fn task_count(&self) -> Result<usize> {
        let reply = self.guest.bridge(
            "dev.glidepod.bridge.Tasks",
            &[
                "count",
                &self.info.display_id.to_string(),
                &self.info.package_id,
            ],
        )?;
        let value: serde_json::Value = serde_json::from_str(reply.trim())?;
        value["count"]
            .as_u64()
            .map(|n| n as usize)
            .context("Invalid task count")
    }
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
            ["copy", "cut", "select_all", "paste"].contains(&action),
            "Invalid edit action"
        );
        let reply = self.guest.shell(&format!("am broadcast --receiver-foreground -a dev.glidepod.input.COMMIT -p dev.glidepod.input --es package {} --es action {}",quote(&self.info.package_id),quote(action)))?;
        ensure!(
            reply.contains("Broadcast completed: result=1"),
            "Select text in an editable field before using this shortcut"
        );
        if action == "select_all" || action == "paste" {
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
        self.frame_in_format(last_sequence, PixelFormat::Rgba)
    }
    pub fn subscribe_frames(&self) -> Result<FrameNotifications> {
        self.guest.verify_epoch()?;
        let mut socket = UnixStream::connect(&self.guest.endpoints.control)?;
        socket.set_write_timeout(Some(Duration::from_secs(2)))?;
        writeln!(socket, "subscribe")?;
        Ok(FrameNotifications {
            reader: BufReader::new(socket),
            pending: String::new(),
            scanout: self.info.scanout,
        })
    }
    /// Read directly in the consumer's format. GPUI needs BGRA; captures need RGBA.
    /// Conversion runs on the caller's frame worker, never on the UI thread.
    pub fn frame_in_format(
        &self,
        last_sequence: u64,
        format: PixelFormat,
    ) -> Result<Option<Frame>> {
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
        read_frame(&source.file, source.generation, last_sequence, format)
    }
}
impl Drop for AppWindow {
    fn drop(&mut self) {
        // Removing a display alone reparents its Android tasks to another display.
        // Finish only our package's task first; preserve app data and background services.
        if self.info.display_id != 0 {
            let _ = self.guest.bridge(
                "dev.glidepod.bridge.Tasks",
                &[
                    "close",
                    &self.info.display_id.to_string(),
                    &self.info.package_id,
                ],
            );
        }
        let _ = control(&self.guest, &format!("display {} off", self.info.scanout));
    }
}
fn read_frame(
    file: &File,
    generation: u32,
    previous: u64,
    output_format: PixelFormat,
) -> Result<Option<Frame>> {
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
    let mut pixels = vec![0; (stride * height) as usize];
    file.read_exact_at(
        &mut pixels,
        u64::from(offset) + u64::from(index) * u64::from(size),
    )?;
    let mut after = [0u8; 64];
    file.read_exact_at(&mut after, 0)?;
    if after != header {
        return Ok(None);
    }
    let input_format = if format == 0 {
        PixelFormat::Bgra
    } else {
        PixelFormat::Rgba
    };
    if input_format != output_format {
        swap_red_blue(&mut pixels);
    }
    Ok(Some(Frame {
        width,
        height,
        sequence,
        pixels,
        format: output_format,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_notifications_preserve_fragments_filter_displays_and_bound_input() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut notifications = FrameNotifications {
            reader: BufReader::new(reader),
            pending: String::new(),
            scanout: 2,
        };
        writer.write_all(b"ok\nframe 1 9\nframe 2").unwrap();
        assert!(!notifications.wait(Duration::from_millis(10)).unwrap());
        writer.write_all(b" 10\n").unwrap();
        assert!(notifications.wait(Duration::from_millis(50)).unwrap());
        writer.write_all(b"frames-off 1\nframe 2 11\n").unwrap();
        assert!(notifications.wait(Duration::from_millis(50)).unwrap());
        // macOS socket buffers can be smaller than this payload; feed it while
        // the reader is active instead of deadlocking the test's writer.
        let oversized = std::thread::spawn(move || writer.write_all(&vec![b'x'; 8193]).unwrap());
        assert!(notifications.wait(Duration::from_millis(50)).is_err());
        oversized.join().unwrap();
        let (reader, writer) = UnixStream::pair().unwrap();
        let mut closed = FrameNotifications {
            reader: BufReader::new(reader),
            pending: String::new(),
            scanout: 2,
        };
        drop(writer);
        assert!(closed.wait(Duration::from_millis(50)).is_err());
    }
    #[test]
    fn swizzle_preserves_alpha_green_and_incomplete_pixel_tails() {
        let original: Vec<_> = (0u8..=255)
            .flat_map(|red| {
                (0u8..=255).flat_map(move |blue| [red, red.wrapping_add(blue), blue, red ^ blue])
            })
            .collect();
        for tail in 0..4 {
            let mut bytes = original.clone();
            bytes.extend_from_slice(&[19, 71, 123][..tail]);
            let mut reference = bytes.clone();
            for pixel in reference.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            swap_red_blue(&mut bytes);
            assert_eq!(bytes, reference);
            swap_red_blue(&mut bytes);
            assert_eq!(&bytes[..original.len()], original);
        }
    }
    #[test]
    #[ignore = "synthetic channel conversion only; run alone with --ignored --nocapture"]
    fn swizzle_benchmark() {
        for size in [720 * 1080 * 4, 1080 * 1920 * 4, 2048 * 2048 * 4] {
            let mut pixels = vec![123; size];
            let mut timings = [Vec::new(), Vec::new()];
            for iteration in 0..330 {
                for path in [iteration % 2, 1 - iteration % 2] {
                    let start = Instant::now();
                    let bytes = std::hint::black_box(&mut pixels);
                    if path == 0 {
                        for pixel in bytes.as_chunks_mut::<4>().0 {
                            pixel.swap(0, 2);
                        }
                    } else {
                        swap_red_blue(bytes);
                    }
                    std::hint::black_box(&pixels);
                    if iteration >= 30 {
                        timings[path].push(start.elapsed().as_secs_f64() * 1000.);
                    }
                }
            }
            for (path, times) in timings.iter_mut().enumerate() {
                times.sort_by(f64::total_cmp);
                println!(
                    "{}",
                    serde_json::json!({"scope":"synthetic_channel_swap", "pixel_bytes":size,
                    "path":if path==0 {"byte_swap"} else {"word_permutation"}, "samples":times.len(),
                    "p50_ms":times[150], "p95_ms":times[285]})
                );
            }
        }
    }
    fn frame_file(width: u32, height: u32, format: u32) -> File {
        let file = tempfile::tempfile().unwrap();
        let size = width * height * 4;
        let mut header = [0u8; 64];
        header[..4].copy_from_slice(b"AVOF");
        for (offset, value) in [
            (4, 1u32),
            (8, 7),
            (12, width),
            (16, height),
            (20, width * 4),
            (24, format),
            (28, 3),
            (32, 4096),
            (36, size),
            (40, 1),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        header[48..56].copy_from_slice(&1u64.to_le_bytes());
        file.set_len(4096 + 3 * u64::from(size)).unwrap();
        file.write_all_at(&header, 0).unwrap();
        let pixels: Vec<_> = [11, 37, 203, 149]
            .into_iter()
            .cycle()
            .take(size as usize)
            .collect();
        file.write_all_at(&pixels, 4096 + u64::from(size)).unwrap();
        file
    }
    #[test]
    fn frame_formats_preserve_color_and_alpha() {
        for source in [0, 1] {
            let file = frame_file(2, 3, source);
            for format in [PixelFormat::Bgra, PixelFormat::Rgba] {
                let frame = read_frame(&file, 7, 0, format).unwrap().unwrap();
                let same = (source == 0) == (format == PixelFormat::Bgra);
                let pixel = if same {
                    [11, 37, 203, 149]
                } else {
                    [203, 37, 11, 149]
                };
                assert_eq!(frame.pixels, pixel.repeat(6));
                assert_eq!(frame.format, format);
                assert_eq!((frame.width, frame.height, frame.sequence), (2, 3, 1));
            }
        }
    }
    #[test]
    fn frame_reader_rejects_stale_invalid_and_truncated_slots() {
        let file = frame_file(2, 3, 0);
        assert!(
            read_frame(&file, 7, 1, PixelFormat::Bgra)
                .unwrap()
                .is_none()
        );
        assert!(read_frame(&file, 8, 0, PixelFormat::Bgra).is_err());
        file.write_all_at(&2u32.to_le_bytes(), 40).unwrap();
        assert!(
            read_frame(&file, 7, 0, PixelFormat::Bgra)
                .unwrap()
                .is_none()
        );
        file.write_all_at(&1u32.to_le_bytes(), 40).unwrap();
        file.set_len(4096 + 24 + 23).unwrap();
        assert!(read_frame(&file, 7, 0, PixelFormat::Bgra).is_err());
    }
    /// Synthetic host transport only: no guest rendering, GPUI upload or display timing.
    #[test]
    #[ignore = "run alone with --ignored --nocapture to measure copied frame costs"]
    fn copied_frame_benchmark() {
        use std::hint::black_box;
        for (width, height) in [(720, 1080), (1080, 1920), (2048, 2048)] {
            let file = frame_file(width, height, 0);
            let mut timings = [Vec::new(), Vec::new()];
            for iteration in 0..330 {
                // Alternate which path runs first to limit cache/order bias.
                for path in [iteration % 2, 1 - iteration % 2] {
                    let start = Instant::now();
                    let mut frame = read_frame(
                        &file,
                        7,
                        0,
                        if path == 0 {
                            PixelFormat::Rgba
                        } else {
                            PixelFormat::Bgra
                        },
                    )
                    .unwrap()
                    .unwrap();
                    if path == 0 {
                        swap_red_blue(&mut frame.pixels);
                    }
                    black_box(&frame.pixels);
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    assert_eq!(&frame.pixels[..4], &[11, 37, 203, 149]);
                    if iteration >= 30 {
                        timings[path].push(elapsed);
                    }
                }
            }
            for (path, values) in timings.iter_mut().enumerate() {
                values.sort_by(f64::total_cmp);
                println!(
                    "{}",
                    serde_json::json!({
                        "scope": "synthetic_copied_bgra_frame",
                        "path": if path == 0 { "previous_double_swizzle" } else { "direct_bgra" },
                        "width": width, "height": height, "samples": values.len(),
                        "p50_ms": values[values.len() / 2],
                        "p95_ms": values[values.len() * 95 / 100],
                        "pixel_bytes": width as u64 * height as u64 * 4,
                    })
                );
            }
        }
    }
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
        assert!(read_frame(&f, 1, 0, PixelFormat::Rgba).is_err());
    }
}
