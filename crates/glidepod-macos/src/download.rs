//! Disk-streamed NSURLSession downloads use the host's system networking settings.
use anyhow::{Result, ensure};
use std::{
    ffi::{CString, c_char, c_void},
    path::Path,
};
unsafe extern "C" {
    fn glidepod_download(
        url: *const c_char,
        path: *const c_char,
        bytes: u64,
        callback: extern "C" fn(u64, *mut c_void) -> bool,
        context: *mut c_void,
    ) -> i32;
}
pub fn file(
    url: &str,
    path: &Path,
    bytes: u64,
    progress: &mut dyn FnMut(u64) -> bool,
) -> Result<()> {
    extern "C" fn update(bytes: u64, context: *mut c_void) -> bool {
        // Native code invokes this synchronously on the calling worker only.
        let callback = unsafe { &mut *(context as *mut &mut dyn FnMut(u64) -> bool) };
        callback(bytes)
    }
    let url = CString::new(url)?;
    let path = CString::new(path.as_os_str().as_encoded_bytes())?;
    let mut callback = progress;
    let result = unsafe {
        glidepod_download(
            url.as_ptr(),
            path.as_ptr(),
            bytes,
            update,
            &mut callback as *mut _ as *mut c_void,
        )
    };
    ensure!(result != 2, "Download cancelled");
    ensure!(
        result == 0,
        "Download failed. Check the Mac's network or system proxy and retry."
    );
    Ok(())
}
