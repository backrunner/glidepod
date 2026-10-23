//! Persistent, display-bound input. The guest releases held state on EOF or a missed heartbeat.
use crate::{
    adb::{Adb, quote},
    display::AppWindow,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct GamepadState {
    /// A, B, X, Y, L1, R1, L2, R2, Select, Start, Mode, L3, R3.
    pub buttons: u32,
    /// Left X/Y, right X/Y, left/right trigger, hat X/Y. Y increases downwards.
    pub axes: [f32; 8],
}
impl GamepadState {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.buttons < (1 << 13), "Invalid controller buttons");
        for (i, value) in self.axes.iter().enumerate() {
            let min = if i == 4 || i == 5 { 0. } else { -1. };
            ensure!(
                value.is_finite() && (min..=1.).contains(value),
                "Invalid controller axis"
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum InputEvent {
    Focus,
    Reset,
    Ping,
    Key { code: u32, down: bool, meta: u32 },
    Touch { points: Vec<(u32, f32, f32)> },
    Mouse { state: (f32, f32, u32, f32, f32) },
    Pad { buttons: u32, axes: [f32; 8] },
}
impl From<GamepadState> for InputEvent {
    fn from(p: GamepadState) -> Self {
        Self::Pad {
            buttons: p.buttons,
            axes: p.axes,
        }
    }
}
impl InputEvent {
    pub fn validate(&self, width: u32, height: u32) -> Result<()> {
        let position = |x: f32, y: f32| {
            x.is_finite()
                && y.is_finite()
                && (0. ..=(width - 1) as f32).contains(&x)
                && (0. ..=(height - 1) as f32).contains(&y)
        };
        match self {
            Self::Key { code, meta, .. } => ensure!(
                (1..=304).contains(code) && *meta <= 0x7fffff,
                "Invalid keyboard event"
            ),
            Self::Touch { points } => {
                ensure!(points.len() <= 10, "Too many touch contacts");
                let mut ids = std::collections::HashSet::new();
                for &(id, x, y) in points {
                    ensure!(
                        id < 32 && ids.insert(id) && position(x, y),
                        "Invalid touch contact"
                    );
                }
            }
            Self::Mouse {
                state: (x, y, b, h, v),
            } => ensure!(
                position(*x, *y)
                    && *b <= 7
                    && h.is_finite()
                    && v.is_finite()
                    && h.abs() <= 100.
                    && v.abs() <= 100.,
                "Invalid mouse event"
            ),
            Self::Pad { buttons, axes } => GamepadState {
                buttons: *buttons,
                axes: *axes,
            }
            .validate()?,
            _ => {}
        }
        Ok(())
    }
}

pub struct InputSession {
    window: Arc<AppWindow>,
    stream: Adb,
    pending: Vec<u8>,
    pub gamepad_device: i32,
    failed: bool,
}
impl AppWindow {
    pub fn open_input(self: &Arc<Self>) -> Result<InputSession> {
        self.guest.verify_epoch()?;
        ensure!(
            !self.input_open.swap(true, Ordering::AcqRel),
            "This app window already owns an input channel"
        );
        let result = self.connect_input();
        if result.is_err() {
            self.input_open.store(false, Ordering::Release);
        }
        result
    }
    fn connect_input(self: &Arc<Self>) -> Result<InputSession> {
        let i = &self.info;
        let command = format!(
            "exec:CLASSPATH=/data/local/tmp/glidepod/input-bridge.jar app_process / dev.glidepod.bridge.Input {} {} {} {}",
            i.display_id,
            i.width,
            i.height,
            quote(&i.package_id)
        );
        let mut stream = Adb::open(
            &self.guest.endpoints.vsock,
            &command,
            Duration::from_secs(8),
        )?;
        stream.coalesce_acks();
        let mut session = InputSession {
            window: self.clone(),
            stream,
            pending: Vec::new(),
            gamepad_device: -1,
            failed: false,
        };
        let ready = session.reply()?;
        ensure!(
            ready["ready"] == 1 && ready["display"] == i.display_id,
            "Input bridge did not identify the owned display: {ready}"
        );
        session.gamepad_device = ready["gamepad"].as_i64().unwrap_or(-1) as i32;
        Ok(session)
    }
}
impl InputSession {
    fn reply(&mut self) -> Result<serde_json::Value> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line = self.pending.drain(..=end).collect::<Vec<_>>();
                return serde_json::from_slice(&line).with_context(|| {
                    format!(
                        "Invalid input bridge response: {}",
                        String::from_utf8_lossy(&line)
                    )
                });
            }
            let data = self
                .stream
                .read_chunk()?
                .context("Android closed the input channel")?;
            ensure!(
                self.pending.len() + data.len() <= 65536,
                "Input bridge response exceeds limit"
            );
            self.pending.extend(data);
        }
    }
    pub fn send(&mut self, event: InputEvent) -> Result<()> {
        ensure!(!self.failed, "Input channel failed; reopen the app window");
        event.validate(self.window.info.width, self.window.info.height)?;
        let result = (|| {
            // A path from an earlier VM boot can never authorize input in a new boot.
            self.window.guest.verify_epoch()?;
            self.stream.renew_timeout(Duration::from_secs(2));
            let mut data = serde_json::to_vec(&event)?;
            data.push(b'\n');
            self.stream.write_all(&data)?;
            let reply = self.reply()?;
            ensure!(reply["ok"] == true, "Android input failed: {reply}");
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result.with_context(|| format!("Send Android input {event:?}"))
    }
}
impl Drop for InputSession {
    fn drop(&mut self) {
        if !self.failed {
            let _ = self.send(InputEvent::Reset);
        }
        self.window.input_open.store(false, Ordering::Release);
        // Closing the transport also runs the guest's finally block, including on timeout.
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_invalid_contacts_before_transport() {
        for points in [
            vec![(0, 1., 1.), (0, 2., 2.)],
            vec![(0, f32::NAN, 1.)],
            vec![(0, 720., 1.)],
            vec![(32, 1., 1.)],
        ] {
            assert!(InputEvent::Touch { points }.validate(720, 1080).is_err());
        }
        assert!(
            InputEvent::Touch {
                points: vec![(0, 0., 0.), (1, 719., 1079.)]
            }
            .validate(720, 1080)
            .is_ok()
        );
    }
    #[test]
    fn controller_ranges_are_not_silently_truncated() {
        assert!(GamepadState::default().validate().is_ok());
        let mut state = GamepadState::default();
        state.axes[4] = -0.1;
        assert!(state.validate().is_err());
        state.axes[4] = 0.;
        state.buttons = 1 << 13;
        assert!(state.validate().is_err());
    }
}
