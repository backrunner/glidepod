//! Native file panels and typed pasteboard access. Panel operations run on the UI thread.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char},
    path::{Path, PathBuf},
};
unsafe extern "C" {
    fn glidepod_files(request: *const c_char) -> *mut c_char;
    fn glidepod_files_free(response: *mut c_char);
}
pub fn command(request: Value) -> Result<Value> {
    let json = CString::new(serde_json::to_string(&request)?)?;
    let ptr = unsafe { glidepod_files(json.as_ptr()) };
    ensure!(!ptr.is_null(), "Mac file integration unavailable");
    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec();
    unsafe { glidepod_files_free(ptr) };
    let value: Value = serde_json::from_slice(&bytes)?;
    ensure!(value.get("error").is_none(), "{}", value["error"]);
    Ok(value)
}
pub fn downloads_directory() -> Result<PathBuf> {
    let v = command(json!({"op":"downloads"}))?;
    Ok(PathBuf::from(
        v["path"]
            .as_str()
            .context("Downloads folder is unavailable")?,
    ))
}
pub fn mime_type(path: &Path) -> Result<String> {
    let v = command(
        json!({"op":"mime","extension":path.extension().and_then(|s|s.to_str()).unwrap_or("")}),
    )?;
    Ok(v["mime"]
        .as_str()
        .context("Cannot determine file type")?
        .into())
}
