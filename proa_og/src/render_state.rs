// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The GLib main context dispatches the load, watchdog, and snapshot callbacks
//! on the rendering thread. Each pending snapshot owns a reference to its state.

use crate::{ffi, renderer::MAX_DIMENSION, OgImage, OgImageError};
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString},
    ptr,
    rc::Rc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitingLoad,
    Loading,
    Snapshot,
    Complete,
    TimedOut,
}

struct Capture {
    main_loop: *mut ffi::GMainLoop,
    cancellable: *mut ffi::GCancellable,
    phase: Cell<Phase>,
    result: RefCell<Option<Result<OgImage, OgImageError>>>,
}

impl Capture {
    fn new() -> Self {
        unsafe {
            Self {
                main_loop: ffi::g_main_loop_new(ptr::null_mut(), 0),
                cancellable: ffi::g_cancellable_new(),
                phase: Cell::new(Phase::AwaitingLoad),
                result: RefCell::new(None),
            }
        }
    }

    fn timeout(&self) {
        if !matches!(self.phase.get(), Phase::Complete | Phase::TimedOut) {
            self.phase.set(Phase::TimedOut);
            unsafe { ffi::g_main_loop_quit(self.main_loop) };
        }
    }

    fn finish_snapshot(&self, result: Result<OgImage, OgImageError>) {
        if self.phase.get() == Phase::Snapshot {
            self.result.replace(Some(result));
            self.phase.set(Phase::Complete);
            unsafe { ffi::g_main_loop_quit(self.main_loop) };
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe {
            ffi::g_object_unref(self.cancellable.cast());
            ffi::g_main_loop_unref(self.main_loop);
        }
    }
}

/// These two registrations borrow the driver's Rc until the loop exits.
/// The snapshot completion callback takes its own strong reference.
struct Registrations {
    view: *mut ffi::WebKitWebView,
    signal: ffi::gulong,
    timer: ffi::guint,
}

impl Drop for Registrations {
    fn drop(&mut self) {
        unsafe {
            ffi::g_signal_handler_disconnect(self.view.cast(), self.signal);
            ffi::g_source_remove(self.timer);
        }
    }
}

/// `view` belongs to the calling renderer and its GLib main context.
pub(crate) unsafe fn capture(
    view: *mut ffi::WebKitWebView,
    html: &str,
    timeout_secs: u32,
) -> Result<OgImage, OgImageError> {
    let html = CString::new(html).map_err(|e| OgImageError::LoadFailed(e.to_string()))?;
    let state = Rc::new(Capture::new());
    {
        let data = ptr::from_ref(&state).cast_mut().cast();
        let signal = ffi::g_signal_connect_data(
            view.cast(),
            c"load-changed".as_ptr(),
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut ffi::WebKitWebView, u32, ffi::gpointer),
                unsafe extern "C" fn(),
            >(on_load_changed)),
            data,
            None,
            0,
        );
        let timer = ffi::g_timeout_add_seconds(timeout_secs, Some(on_timeout), data);
        let _registrations = Registrations {
            view,
            signal,
            timer,
        };
        ffi::webkit_web_view_load_html(view, html.as_ptr(), c"".as_ptr());
        ffi::g_main_loop_run(state.main_loop);
    }
    if state.phase.get() == Phase::TimedOut {
        ffi::g_cancellable_cancel(state.cancellable);
        ffi::webkit_web_view_stop_loading(view);
        return Err(OgImageError::Timeout(timeout_secs));
    }
    let result = state.result.take();
    result.unwrap_or_else(|| Err(OgImageError::SnapshotFailed("no snapshot".into())))
}

unsafe extern "C" fn on_timeout(data: ffi::gpointer) -> ffi::gboolean {
    let state = &*data.cast::<Rc<Capture>>();
    state.timeout();
    // The registration guard removes the source immediately after the loop returns.
    1
}

unsafe extern "C" fn on_load_changed(
    view: *mut ffi::WebKitWebView,
    event: u32,
    data: ffi::gpointer,
) {
    let state = &*data.cast::<Rc<Capture>>();
    match (event, state.phase.get()) {
        (ffi::WebKitLoadEvent_WEBKIT_LOAD_STARTED, Phase::AwaitingLoad) => {
            state.phase.set(Phase::Loading);
        }
        (ffi::WebKitLoadEvent_WEBKIT_LOAD_FINISHED, Phase::Loading) => {
            state.phase.set(Phase::Snapshot);
            ffi::webkit_web_view_get_snapshot(
                view,
                ffi::WebKitSnapshotRegion_WEBKIT_SNAPSHOT_REGION_VISIBLE,
                ffi::WebKitSnapshotOptions_WEBKIT_SNAPSHOT_OPTIONS_NONE,
                state.cancellable,
                Some(on_snapshot_ready),
                Rc::into_raw(Rc::clone(state)).cast_mut().cast(),
            );
        }
        _ => {}
    }
}

struct Surface(*mut ffi::cairo_surface_t);
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe { ffi::cairo_surface_destroy(self.0) }
    }
}

unsafe extern "C" fn on_snapshot_ready(
    source: *mut ffi::GObject,
    result: *mut ffi::GAsyncResult,
    data: ffi::gpointer,
) {
    // WebKit calls completion once, including after cancellation.
    let state = Rc::from_raw(data.cast::<Capture>());
    let mut error = ptr::null_mut();
    let raw = ffi::webkit_web_view_get_snapshot_finish(source.cast(), result, &mut error);
    let surface = (!raw.is_null()).then(|| Surface(raw));
    let result = if !error.is_null() {
        let message = CStr::from_ptr((*error).message)
            .to_string_lossy()
            .into_owned();
        ffi::g_error_free(error);
        Err(OgImageError::SnapshotFailed(message))
    } else if state.phase.get() != Phase::Snapshot {
        return;
    } else if let Some(surface) = surface {
        read_image(surface)
    } else {
        Err(OgImageError::SnapshotFailed("no surface".into()))
    };
    state.finish_snapshot(result);
}

unsafe fn read_image(source: Surface) -> Result<OgImage, OgImageError> {
    let width = ffi::cairo_image_surface_get_width(source.0);
    let height = ffi::cairo_image_surface_get_height(source.0);
    let dimensions = 1..=MAX_DIMENSION as i32;
    if ffi::cairo_surface_status(source.0) != 0
        || !dimensions.contains(&width)
        || !dimensions.contains(&height)
    {
        return Err(OgImageError::SnapshotFailed(format!(
            "invalid snapshot dimensions {width}x{height}"
        )));
    }
    let copy = Surface(ffi::cairo_image_surface_create(
        ffi::_cairo_format_CAIRO_FORMAT_ARGB32,
        width,
        height,
    ));
    let cr = ffi::cairo_create(copy.0);
    ffi::cairo_set_source_surface(cr, source.0, 0.0, 0.0);
    ffi::cairo_paint(cr);
    let status = ffi::cairo_status(cr);
    ffi::cairo_destroy(cr);
    ffi::cairo_surface_flush(copy.0);
    let stride = ffi::cairo_image_surface_get_stride(copy.0);
    let data = ffi::cairo_image_surface_get_data(copy.0);
    // Cairo's fresh ARGB32 image has four bytes per pixel, already aligned.
    if status != 0
        || ffi::cairo_surface_status(copy.0) != 0
        || stride != width * 4
        || data.is_null()
    {
        return Err(OgImageError::SnapshotFailed(
            "invalid ARGB32 surface".into(),
        ));
    }
    let pixels = std::slice::from_raw_parts(data, stride as usize * height as usize).to_vec();
    let mut png_bytes = Vec::new();
    let status = ffi::cairo_surface_write_to_png_stream(
        copy.0,
        Some(write_png),
        ptr::from_mut(&mut png_bytes).cast(),
    );
    if status != 0 {
        return Err(OgImageError::PngEncode(format!("Cairo status {status}")));
    }
    Ok(OgImage {
        pixels,
        png_bytes,
        width: width as u32,
        height: height as u32,
    })
}

unsafe extern "C" fn write_png(output: ffi::gpointer, data: *const u8, length: u32) -> u32 {
    if length != 0 {
        (*output.cast::<Vec<u8>>())
            .extend_from_slice(std::slice::from_raw_parts(data, length as usize));
    }
    0 // CAIRO_STATUS_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_terminal_callback_owns_the_result() {
        let done = Capture::new();
        done.phase.set(Phase::Snapshot);
        done.finish_snapshot(Err(OgImageError::SnapshotFailed("test".into())));
        done.timeout();
        assert!(done.phase.get() == Phase::Complete);
        assert!(matches!(
            done.result.take(),
            Some(Err(OgImageError::SnapshotFailed(_)))
        ));

        let driver = Rc::new(Capture::new());
        driver.phase.set(Phase::Snapshot);
        let pending = Rc::clone(&driver);
        let weak = Rc::downgrade(&driver);
        driver.timeout();
        drop(driver);
        pending.finish_snapshot(Err(OgImageError::SnapshotFailed("test".into())));
        assert!(pending.phase.get() == Phase::TimedOut);
        assert!(pending.result.take().is_none());
        drop(pending);
        assert!(weak.upgrade().is_none());
    }
}
