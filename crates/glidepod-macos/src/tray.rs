//! Main-thread SwiftUI status panel, with thread-safe publication of monitoring data.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{marker::PhantomData, rc::Rc};

#[derive(Debug, Clone, Serialize)]
pub struct AppEntry {
    pub package_id: String,
    pub title: String,
    pub icon_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub state: String,
    pub detail: String,
    pub cpu: String,
    pub memory: String,
    pub allocation: String,
    pub app_count: Option<usize>,
    pub apps: Vec<AppEntry>,
    pub valid_for_seconds: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    OpenCenter,
    FocusApp { package_id: String },
    Quit,
}

pub struct Tray {
    handle: *mut std::ffi::c_void,
    _main_thread: PhantomData<Rc<()>>,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn glidepod_tray_new(icon_data: *const u8, icon_len: usize) -> *mut std::ffi::c_void;
    fn glidepod_tray_free(handle: *mut std::ffi::c_void);
    fn glidepod_tray_drain(handle: *mut std::ffi::c_void) -> *mut std::ffi::c_char;
    fn glidepod_tray_show(handle: *mut std::ffi::c_void);
    fn glidepod_tray_publish(data: *const u8, len: usize);
}

impl Tray {
    pub fn new() -> Result<Self> {
        #[cfg(target_os = "macos")]
        let handle = {
            let icon = include_bytes!("../../../assets/brand/glidepod-icon-light.png");
            unsafe { glidepod_tray_new(icon.as_ptr(), icon.len()) }
        };
        #[cfg(not(target_os = "macos"))]
        let handle: *mut std::ffi::c_void = std::ptr::null_mut();
        ensure!(
            !handle.is_null(),
            "Could not create the macOS menu bar item"
        );
        Ok(Self {
            handle,
            _main_thread: PhantomData,
        })
    }

    pub fn drain(&self) -> Vec<Action> {
        #[cfg(target_os = "macos")]
        unsafe {
            let data = glidepod_tray_drain(self.handle);
            if data.is_null() {
                return Vec::new();
            }
            let actions = serde_json::from_slice(std::ffi::CStr::from_ptr(data).to_bytes());
            libc::free(data.cast());
            actions.unwrap_or_default()
        }
        #[cfg(not(target_os = "macos"))]
        Vec::new()
    }

    pub fn show(&self) {
        #[cfg(target_os = "macos")]
        unsafe {
            glidepod_tray_show(self.handle);
        }
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            glidepod_tray_free(self.handle);
        }
    }
}

/// Publishes immutable data only; AppKit is refreshed on its main run loop.
pub fn publish(snapshot: &Snapshot) {
    #[cfg(target_os = "macos")]
    if let Ok(data) = serde_json::to_vec(snapshot) {
        unsafe {
            glidepod_tray_publish(data.as_ptr(), data.len());
        }
    }
}
