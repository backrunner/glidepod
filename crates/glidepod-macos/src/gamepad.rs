//! GameController.framework polling. Only the active app window consumes these snapshots.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GamepadSnapshot {
    pub identity: u64,
    pub buttons: u32,
    pub axes: [f32; 8],
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn glidepod_gamepad_poll(out: *mut GamepadSnapshot) -> i32;
    #[cfg(test)]
    fn glidepod_gamepad_mapping_probe(out: *mut GamepadSnapshot) -> i32;
}
/// Must be called on the native main thread; returns None while disconnected.
pub fn poll() -> Option<GamepadSnapshot> {
    #[cfg(target_os = "macos")]
    {
        let mut out = GamepadSnapshot::default();
        // SAFETY: the shim bounds every write to this C-compatible structure and checks the thread.
        if unsafe { glidepod_gamepad_poll(&mut out) } == 1 {
            return Some(out);
        }
    }
    None
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn apple_snapshot_maps_axes_and_buttons() {
        let mut out = GamepadSnapshot::default();
        // SAFETY: framework snapshot owned locally; no connected controller or global input is used.
        assert_eq!(unsafe { glidepod_gamepad_mapping_probe(&mut out) }, 1);
        assert_eq!(out.buttons & 17, 17);
        assert_eq!(&out.axes[..2], &[0.5, -0.75]);
        assert!((out.axes[5] - 0.6).abs() < 0.001);
        assert_eq!(&out.axes[6..], &[-1., -1.]);
    }
}
