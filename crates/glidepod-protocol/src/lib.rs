//! Typed host/guest contracts. No network listener or authorization is implied by these types.
use serde::{Deserialize, Serialize};
use thiserror::Error;
pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 0;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub apk_install: bool,
    pub task_displays: bool,
    pub guest_root: bool,
    pub live_widgets: bool,
    pub gpu_rendering: bool,
    pub shared_surfaces: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub major: u16,
    pub minor: u16,
    pub image_id: String,
    pub capabilities: Capabilities,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowLease {
    pub vm_epoch: u64,
    pub task_id: u32,
    pub display_id: u32,
    pub display_generation: u64,
    pub resize_serial: u64,
    pub focus_epoch: u64,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("The runtime protocol requires a compatible major version.")]
    MajorVersion,
    #[error("The guest image does not match the selected runtime.")]
    ImageMismatch,
    #[error("A stale window, display, resize or focus generation was rejected.")]
    StaleLease,
    #[error("The control payload exceeds the limit or has invalid framing.")]
    InvalidFrame,
}
impl Hello {
    pub fn negotiate(&self, expected_image: &str) -> Result<u16, ProtocolError> {
        if self.major != PROTOCOL_MAJOR {
            return Err(ProtocolError::MajorVersion);
        }
        if self.image_id != expected_image {
            return Err(ProtocolError::ImageMismatch);
        }
        Ok(PROTOCOL_MINOR)
    }
}
impl WindowLease {
    pub fn validate_input(&self, current: &Self) -> Result<(), ProtocolError> {
        if self != current {
            Err(ProtocolError::StaleLease)
        } else {
            Ok(())
        }
    }
}
/// Bounded framing primitive for the future vsock transport. Payload encoding is negotiated separately.
pub fn frame(payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if payload.is_empty() || payload.len() > MAX_CONTROL_BYTES {
        return Err(ProtocolError::InvalidFrame);
    }
    let mut bytes = Vec::with_capacity(4 + payload.len());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}
pub fn payload(frame: &[u8]) -> Result<&[u8], ProtocolError> {
    if frame.len() < 4 {
        return Err(ProtocolError::InvalidFrame);
    }
    let size = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    if size == 0 || size > MAX_CONTROL_BYTES || frame.len() != size + 4 {
        return Err(ProtocolError::InvalidFrame);
    }
    Ok(&frame[4..])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_truncated_and_oversized_frame() {
        assert!(payload(&[0, 0, 0, 8, 1]).is_err());
        assert!(payload(&[255, 255, 255, 255]).is_err());
        assert_eq!(payload(&frame(b"hello").unwrap()).unwrap(), b"hello");
    }
    #[test]
    fn stale_focus_and_resize_never_receive_input() {
        let l = WindowLease {
            vm_epoch: 1,
            task_id: 7,
            display_id: 2,
            display_generation: 3,
            resize_serial: 9,
            focus_epoch: 4,
        };
        let mut n = l;
        n.focus_epoch += 1;
        assert!(l.validate_input(&n).is_err());
        n = l;
        n.resize_serial += 1;
        assert!(l.validate_input(&n).is_err());
    }
    #[test]
    fn handshake_requires_image_and_major_match() {
        let mut h = Hello {
            major: 1,
            minor: 4,
            image_id: "image-a".into(),
            capabilities: Capabilities::default(),
        };
        assert_eq!(h.negotiate("image-a"), Ok(0));
        assert_eq!(h.negotiate("image-b"), Err(ProtocolError::ImageMismatch));
        h.major = 2;
        assert_eq!(h.negotiate("image-a"), Err(ProtocolError::MajorVersion));
    }
}
