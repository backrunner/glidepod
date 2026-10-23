//! Local notification center only; does not register for APNs or make network requests.
use anyhow::{Result, ensure};
use serde_json::Value;
use std::ffi::{CStr, CString, c_char};
unsafe extern "C" {
    fn glidepod_notifications(request: *const c_char) -> *mut c_char;
    fn glidepod_notifications_free(response: *mut c_char);
}
pub fn command(request: Value) -> Result<Value> {
    let json = CString::new(serde_json::to_string(&request)?)?;
    let p = unsafe { glidepod_notifications(json.as_ptr()) };
    ensure!(!p.is_null(), "Native notification bridge unavailable");
    let data = unsafe { CStr::from_ptr(p) }.to_bytes().to_vec();
    unsafe { glidepod_notifications_free(p) };
    let value: Value = serde_json::from_slice(&data)?;
    ensure!(value.get("error").is_none(), "{}", value["error"]);
    Ok(value)
}
