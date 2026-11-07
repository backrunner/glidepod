//! Window-scoped AppKit gestures. The monitor and queue stay on the main thread.
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GestureEvent {
    pub kind: u32,
    pub phase: u32,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
}
unsafe extern "C" {
    fn glidepod_gestures_new(view: *mut c_void) -> *mut c_void;
    fn glidepod_gestures_configure(
        handle: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        enabled: bool,
        touch_scroll: bool,
    );
    fn glidepod_gestures_next(handle: *mut c_void, event: *mut GestureEvent) -> bool;
    fn glidepod_gestures_free(handle: *mut c_void);
}
pub struct Gestures {
    handle: NonNull<c_void>,
    _main_thread: PhantomData<Rc<()>>,
}
impl Gestures {
    /// # Safety
    /// `view` must be a live NSView. Create, use and drop this object on the main thread.
    pub unsafe fn new(view: NonNull<c_void>) -> Self {
        Self {
            handle: NonNull::new(unsafe { glidepod_gestures_new(view.as_ptr()) }).unwrap(),
            _main_thread: PhantomData,
        }
    }
    pub fn configure(&mut self, bounds: [f64; 4], enabled: bool, touch_scroll: bool) {
        let [x, y, w, h] = bounds;
        unsafe {
            glidepod_gestures_configure(self.handle.as_ptr(), x, y, w, h, enabled, touch_scroll)
        }
    }
    pub fn drain(&mut self) -> Vec<GestureEvent> {
        let mut result = Vec::new();
        let mut event = GestureEvent::default();
        while unsafe { glidepod_gestures_next(self.handle.as_ptr(), &mut event) } {
            result.push(event);
        }
        result
    }
}
impl Drop for Gestures {
    fn drop(&mut self) {
        unsafe { glidepod_gestures_free(self.handle.as_ptr()) }
    }
}
