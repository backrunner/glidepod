//! VM-scoped location forwarding. Mac fixes are never persisted in configuration or status.
use crate::{Store, experimental::atomic_json, guest::Guest, memory::control};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{sync::mpsc, thread, time::Duration};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Off,
    Host,
    Virtual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub accuracy: f64,
}
impl Point {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (-90.0..=90.0).contains(&self.latitude)
                && (-180.0..=180.0).contains(&self.longitude)
                && (-12000.0..=100000.0).contains(&self.altitude)
                && (1.0..=100000.0).contains(&self.accuracy),
            "Location needs finite latitude (-90…90), longitude (-180…180), altitude (-12000…100000 m) and accuracy (1…100000 m)"
        );
        Ok(())
    }
}
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub source: Source,
    pub point: Option<Point>,
}
impl Configuration {
    pub fn validate(&self) -> Result<()> {
        if let Some(point) = &self.point {
            point.validate()?;
        }
        ensure!(
            self.source != Source::Virtual || self.point.is_some(),
            "Enter a virtual location first"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub epoch: u64,
    pub source: Source,
    pub delivering: bool,
    pub message: String,
}
impl Store {
    pub fn location_status(&self) -> Result<Status> {
        let config = self.load()?.location;
        if let glidepod_core::VmState::Running { epoch } = self.runtime_status()?.vm
            && let Ok(bytes) = std::fs::read(self.root().join("location-status.json"))
            && bytes.len() < 8192
            && let Ok(status) = serde_json::from_slice::<Status>(&bytes)
            && status.epoch == epoch
            && status.source == config.source
        {
            return Ok(status);
        }
        Ok(Status {
            epoch: 0,
            source: config.source,
            delivering: false,
            message: if config.source == Source::Off {
                "Location is off."
            } else {
                "Location will apply when Android is running."
            }
            .into(),
        })
    }
}
pub(crate) struct Worker {
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn start(guest: Guest) -> Self {
        let (stop, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut native = None;
            let mut enabled = None;
            let mut previous = None;
            loop {
                let config = guest.store.load().map(|c| c.location);
                let source = config.as_ref().map(|c| c.source).unwrap_or(Source::Off);
                let mut state = Status {
                    epoch: guest.endpoints.epoch,
                    source,
                    delivering: false,
                    message: String::new(),
                };
                let result = (|| -> Result<()> {
                    let config = config?;
                    let point = match config.source {
                        Source::Off => {
                            native = None;
                            state.message = "Location is off.".into();
                            None
                        }
                        Source::Virtual => {
                            native = None;
                            state.message = "Virtual location is active.".into();
                            config.point
                        }
                        Source::Host => {
                            if native.is_none() {
                                native = Some(glidepod_macos::location::Session::new()?);
                            }
                            let fix = native.as_mut().unwrap().poll(true);
                            state.message = match fix.state {
                                1 => "Allow GlidePod to use location in the macOS permission prompt.",
                                2 => "Mac location permission is denied. Enable it in System Settings → Privacy & Security → Location Services.",
                                3 => "Mac location is restricted by system policy.",
                                4 => "Waiting for a fresh Mac location.",
                                5 => "Following the Mac’s location.",
                                _ => "Mac location is unavailable. Use the packaged GlidePod app and enable macOS Location Services.",
                            }.into();
                            (fix.state == 5).then_some(Point {
                                latitude: fix.latitude,
                                longitude: fix.longitude,
                                altitude: fix.altitude,
                                accuracy: fix.accuracy.max(1.0),
                            })
                        }
                    };
                    if let Some(point) = &point {
                        point.validate()?;
                        ensure!(
                            control(
                                &guest,
                                &format!(
                                    "location {} {} {} {}",
                                    point.latitude, point.longitude, point.altitude, point.accuracy
                                )
                            )?
                            .trim()
                                == "ok",
                            "This runtime does not support location forwarding"
                        );
                    } else if enabled != Some(false) {
                        ensure!(
                            control(&guest, "location off")?.trim() == "ok",
                            "This runtime does not support location control"
                        );
                    }
                    let wanted = point.is_some();
                    if enabled != Some(wanted) {
                        // Disabling Android location also prevents the image's synthetic fallback fix.
                        guest.shell_timeout(
                            &format!("cmd location set-location-enabled {wanted} --user 0"),
                            Duration::from_secs(5),
                        )?;
                        enabled = Some(wanted);
                    }
                    state.delivering = wanted;
                    Ok(())
                })();
                if let Err(error) = result {
                    native = None;
                    let _ = control(&guest, "location off");
                    let _ = guest.shell_timeout(
                        "cmd location set-location-enabled false --user 0",
                        Duration::from_secs(5),
                    );
                    enabled = None;
                    state.message = format!("Location unavailable: {error}");
                }
                if previous.as_ref() != Some(&state) {
                    let _ = atomic_json(&guest.store.root().join("location-status.json"), &state);
                    previous = Some(state);
                }
                if receiver.recv_timeout(Duration::from_secs(2))
                    != Err(mpsc::RecvTimeoutError::Timeout)
                {
                    break;
                }
            }
            drop(native);
            let _ = control(&guest, "location off");
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_are_bounded_and_virtual_mode_requires_a_point() {
        let good = Point {
            latitude: 1.3,
            longitude: 103.8,
            altitude: 0.0,
            accuracy: 10.0,
        };
        assert!(good.validate().is_ok());
        for latitude in [f64::NAN, f64::INFINITY, -91.0, 91.0] {
            assert!(
                Point {
                    latitude,
                    ..good.clone()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            Point {
                accuracy: f64::INFINITY,
                ..good.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            Point {
                longitude: 181.0,
                ..good
            }
            .validate()
            .is_err()
        );
        assert!(
            Configuration {
                source: Source::Virtual,
                point: None
            }
            .validate()
            .is_err()
        );
        assert!(Configuration::default().validate().is_ok());
    }
}
