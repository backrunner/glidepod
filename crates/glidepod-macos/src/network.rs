//! System proxy discovery only. Raw host sockets do not inherit these preferences.
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyEndpoint {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Syntax only: no DNS lookup, authentication or reachability check is performed.
    pub endpoint_valid: bool,
}

impl ProxyEndpoint {
    pub fn summary(&self) -> String {
        if !self.enabled {
            "Disabled".into()
        } else if let (true, Some(host), Some(port)) = (self.endpoint_valid, &self.host, self.port)
        {
            if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            }
        } else {
            "Enabled, but the endpoint is invalid or incomplete".into()
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemProxySnapshot {
    pub http: ProxyEndpoint,
    pub https: ProxyEndpoint,
    pub socks: ProxyEndpoint,
    pub pac_enabled: bool,
    pub pac_url_present: bool,
    pub auto_discovery_enabled: bool,
    pub exclude_simple_hostnames: bool,
    pub bypass_rule_count: usize,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Deserialize)]
struct RawEndpoint {
    enabled: bool,
    host: Option<String>,
    port: Option<serde_json::Value>,
}

#[cfg(any(target_os = "macos", test))]
impl From<RawEndpoint> for ProxyEndpoint {
    fn from(raw: RawEndpoint) -> Self {
        let host = raw.host.filter(|host| {
            !host.is_empty()
                && host.len() <= 255
                && !host
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || "/\\@?#".contains(c))
        });
        let port = raw
            .port
            .and_then(|port| port.as_u64())
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0);
        let endpoint_valid = host.is_some() && port.is_some();
        Self {
            enabled: raw.enabled,
            host,
            port,
            endpoint_valid,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Deserialize)]
struct RawSnapshot {
    http: RawEndpoint,
    https: RawEndpoint,
    socks: RawEndpoint,
    pac_enabled: bool,
    pac_url_present: bool,
    auto_discovery_enabled: bool,
    exclude_simple_hostnames: bool,
    bypass_rule_count: usize,
}

#[cfg(any(target_os = "macos", test))]
fn parse_snapshot(bytes: &[u8]) -> Result<SystemProxySnapshot> {
    let raw: RawSnapshot = serde_json::from_slice(bytes)?;
    Ok(SystemProxySnapshot {
        http: raw.http.into(),
        https: raw.https.into(),
        socks: raw.socks.into(),
        pac_enabled: raw.pac_enabled,
        pac_url_present: raw.pac_url_present,
        auto_discovery_enabled: raw.auto_discovery_enabled,
        exclude_simple_hostnames: raw.exclude_simple_hostnames,
        bypass_rule_count: raw.bypass_rule_count,
    })
}

/// Snapshot the default macOS proxy dictionary. This does not evaluate a PAC script,
/// select a proxy for a destination, inspect VPN routes or connect to the proxy.
#[cfg(target_os = "macos")]
pub fn system_proxy_snapshot() -> Result<SystemProxySnapshot> {
    use std::ffi::{CStr, c_char};
    unsafe extern "C" {
        fn glidepod_system_proxy_snapshot() -> *mut c_char;
        fn glidepod_system_proxy_snapshot_free(snapshot: *mut c_char);
    }
    // SAFETY: the bridge returns a NUL-terminated allocation or null. Parse while it
    // is owned, then release it with the matching function even if decoding fails.
    let pointer = unsafe { glidepod_system_proxy_snapshot() };
    anyhow::ensure!(
        !pointer.is_null(),
        "macOS system proxy settings unavailable"
    );
    let result = parse_snapshot(unsafe { CStr::from_ptr(pointer) }.to_bytes());
    unsafe { glidepod_system_proxy_snapshot_free(pointer) };
    result
}

#[cfg(not(target_os = "macos"))]
pub fn system_proxy_snapshot() -> Result<SystemProxySnapshot> {
    anyhow::bail!("System proxy discovery requires macOS")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(target_os = "macos")]
    #[test]
    fn native_snapshot_decodes_without_modifying_system_preferences() {
        // Exercises Objective-C/JSON boolean representation as well as allocation/free.
        let snapshot = system_proxy_snapshot().unwrap();
        assert!(serde_json::to_value(snapshot).unwrap()["pac_enabled"].is_boolean());
    }

    #[test]
    fn enabled_invalid_proxy_is_not_reported_as_disabled() {
        for (host, port) in [
            ("127.0.0.1", json!(0)),
            ("127.0.0.1", json!(65536)),
            ("127.0.0.1", json!(-1)),
            ("127.0.0.1", json!(8080.5)),
            ("user:password@proxy.test", json!(8080)),
            ("proxy.test\nInjected", json!(8080)),
        ] {
            let endpoint = ProxyEndpoint::from(RawEndpoint {
                enabled: true,
                host: Some(host.into()),
                port: Some(port),
            });
            assert!(endpoint.enabled);
            assert!(!endpoint.endpoint_valid);
            assert!(endpoint.summary().starts_with("Enabled, but"));
        }
    }

    #[test]
    fn inventory_keeps_protocols_and_pac_separate_without_copying_secrets() {
        let raw = json!({
            "http": {"enabled":true,"host":"127.0.0.1","port":7890},
            "https": {"enabled":false,"host":"old.proxy.test","port":8080},
            "socks": {"enabled":true,"host":"::1","port":1080},
            "pac_enabled":true,"pac_url_present":true,"auto_discovery_enabled":false,
            "exclude_simple_hostnames":true,"bypass_rule_count":2,
            "pac_url":"https://proxy.test/private-secret", "password":"private-secret"
        });
        let snapshot = parse_snapshot(&serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(snapshot.http.summary(), "127.0.0.1:7890");
        assert_eq!(snapshot.https.summary(), "Disabled");
        assert_eq!(snapshot.socks.summary(), "[::1]:1080");
        assert!(snapshot.pac_enabled && snapshot.pac_url_present);
        assert_eq!(snapshot.bypass_rule_count, 2);
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("private-secret")
        );
    }
}
