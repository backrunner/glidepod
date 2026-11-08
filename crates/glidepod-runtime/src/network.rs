//! Desired network attachment. Runtime status reports the configuration of this boot.
use crate::{Store, experimental::atomic_json, guest::Guest};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{sync::mpsc, thread, time::Duration};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Shared,
    Bridged,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub mode: Mode,
    pub interface: Option<String>,
}
impl Configuration {
    pub fn validate(&self) -> Result<()> {
        if let Some(name) = &self.interface {
            ensure!(
                !name.is_empty()
                    && name.len() <= 15
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "Invalid bridge interface name"
            );
        }
        ensure!(
            self.mode != Mode::Bridged || self.interface.is_some(),
            "Choose a bridge interface first"
        );
        Ok(())
    }
    pub fn prepare(&self, command: &mut std::process::Command) -> Result<()> {
        self.validate()?;
        if self.mode == Mode::Bridged {
            let interface = self.interface.as_ref().unwrap();
            ensure!(
                glidepod_macos::network::bridge_interfaces()?.contains(interface),
                "Bridge interface {interface} is unavailable. Choose an available interface or Shared network."
            );
            // An old engine rejects this argument. It cannot silently run shared networking.
            command.arg("--bridge-interface").arg(interface);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupStatus {
    pub epoch: u64,
    pub message: String,
}
impl Store {
    pub fn network_startup_status(&self, epoch: u64) -> Option<StartupStatus> {
        let bytes = std::fs::read(self.root().join("network-startup.json")).ok()?;
        if bytes.len() > 8192 {
            return None;
        }
        serde_json::from_slice::<StartupStatus>(&bytes)
            .ok()
            .filter(|s| s.epoch == epoch)
    }
}

/// Android must join its synthetic Wi-Fi network before host sockets are usable.
/// This bounded boot task runs outside the supervisor's request loop.
pub(crate) struct Worker {
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn start(guest: Guest, mode: Mode) -> Self {
        let (stop, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let publish = |message: String| {
                let _ = atomic_json(
                    &guest.store.root().join("network-startup.json"),
                    &StartupStatus {
                        epoch: guest.endpoints.epoch,
                        message,
                    },
                );
            };
            publish("Preparing Android network connection…".into());
            let result = (|| -> Result<()> {
                let wifi = if mode == Mode::Shared {
                    "enabled"
                } else {
                    "disabled"
                };
                guest.shell_timeout(
                    &format!("cmd wifi set-wifi-enabled {wifi}"),
                    Duration::from_secs(5),
                )?;
                let interface = if mode == Mode::Shared {
                    "wlan0"
                } else {
                    "eth1"
                };
                let mut requested = false;
                for _ in 0..15 {
                    let address = guest.shell_timeout(
                        &format!("ip -o -4 addr show dev {interface}"),
                        Duration::from_secs(3),
                    )?;
                    if address.contains(" inet ") {
                        publish("Android obtained an IPv4 address at startup. Internet access is not checked.".into());
                        return Ok(());
                    }
                    if mode == Mode::Shared && !requested {
                        let scan = guest
                            .shell_timeout("cmd wifi list-scan-results", Duration::from_secs(3))?;
                        if scan.split_whitespace().any(|word| word == "VirtWifi") {
                            guest.shell_timeout(
                                "cmd wifi connect-network VirtWifi open",
                                Duration::from_secs(5),
                            )?;
                            requested = true;
                        }
                    }
                    if receiver.recv_timeout(Duration::from_secs(2))
                        != Err(mpsc::RecvTimeoutError::Timeout)
                    {
                        return Ok(());
                    }
                }
                anyhow::bail!(
                    "Android did not obtain an IPv4 address. Check its network settings and the selected connection."
                )
            })();
            if let Err(error) = result {
                publish(format!("Network setup incomplete: {error}"));
            }
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
    fn bridged_mode_requires_a_specific_safe_interface() {
        assert!(Configuration::default().validate().is_ok());
        for interface in [
            None,
            Some("".into()),
            Some("en0;id".into()),
            Some("../en0".into()),
        ] {
            assert!(
                Configuration {
                    mode: Mode::Bridged,
                    interface
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            Configuration {
                mode: Mode::Bridged,
                interface: Some("en0".into())
            }
            .validate()
            .is_ok()
        );
    }
}
