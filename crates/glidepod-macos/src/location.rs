//! Thread-owned Core Location session. Real coordinates are returned only in memory.
use anyhow::{Result, bail};

#[repr(C)]
#[derive(Default)]
pub struct Sample {
    pub state: i32,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub accuracy: f64,
    pub timestamp: f64,
}

pub struct Session {
    #[cfg(target_os = "macos")]
    handle: *mut std::ffi::c_void,
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn gp_location_create() -> *mut std::ffi::c_void;
    fn gp_location_poll(handle: *mut std::ffi::c_void, active: i32, sample: *mut Sample) -> i32;
    fn gp_location_destroy(handle: *mut std::ffi::c_void);
}
impl Session {
    pub fn new() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let handle = unsafe { gp_location_create() };
            if handle.is_null() {
                bail!("Core Location is unavailable");
            }
            Ok(Self { handle })
        }
        #[cfg(not(target_os = "macos"))]
        bail!("Mac location requires macOS")
    }
    pub fn poll(&mut self, active: bool) -> Sample {
        let mut sample = Sample::default();
        #[cfg(target_os = "macos")]
        unsafe {
            gp_location_poll(self.handle, i32::from(active), &mut sample);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = active;
            sample.state = 6;
        }
        sample
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            gp_location_destroy(self.handle);
        }
    }
}
