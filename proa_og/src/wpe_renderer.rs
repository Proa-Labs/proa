// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[cfg(not(wpe_stub))]
use std::ffi::CString;
#[cfg(not(wpe_stub))]
use std::ptr;
#[cfg(not(wpe_stub))]
use std::sync::Once;

use crate::renderer::{OgImage, OgImageError, RenderOptions};

#[cfg(not(wpe_stub))]
static WPE_INIT: Once = Once::new();

/// WPE WebKit headless renderer for OG image generation.
///
/// Uses WPE WebKit instead of WebKitGTK — no GTK dependency, no display
/// server (Xvfb) required. WPE manages its own headless surfaces.
///
/// Keeps a persistent WebView alive between renders. Reuse a single
/// `WpeRenderer` for best performance — subsequent renders skip WebView
/// creation and just swap HTML content.
pub struct WpeRenderer {
    #[cfg(not(wpe_stub))]
    web_view: *mut crate::ffi::WebKitWebView,
}

impl WpeRenderer {
    /// Create a new WPE renderer with a persistent WebView.
    ///
    /// Does not require a display server or Xvfb — WPE runs fully headless.
    pub fn new() -> Result<Self, OgImageError> {
        #[cfg(wpe_stub)]
        {
            Err(OgImageError::PlatformNotAvailable)
        }

        #[cfg(not(wpe_stub))]
        {
            use crate::ffi;

            // Initialize the WPE backend once
            WPE_INIT.call_once(|| unsafe {
                // Ensure the default web context is created — this initializes
                // the WPE backend in headless mode
                let _ctx = ffi::webkit_web_context_get_default();
            });

            unsafe {
                let settings = ffi::webkit_settings_new();
                ffi::webkit_settings_set_hardware_acceleration_policy(
                    settings, 2, // WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER
                );
                ffi::webkit_settings_set_enable_javascript(settings, 0);

                // WPE creates views without windows — no GTK widget hierarchy
                let web_view = ffi::webkit_web_view_new_with_settings(settings);

                Ok(Self {
                    web_view: web_view as *mut ffi::WebKitWebView,
                })
            }
        }
    }

    /// Render an HTML string to an OG image.
    ///
    /// Reuses the persistent WebView — just loads new HTML and snapshots.
    /// Much faster than creating a new WebView per render.
    ///
    /// # Security
    ///
    /// This renders with the real WebKit engine. JavaScript is disabled, but
    /// `webkit_web_view_load_html` still fetches external subresources declared
    /// in the markup (`<img src>`, CSS `@import`/`url(...)`, web fonts),
    /// including `file://` paths and internal `http(s)://` endpoints such as the
    /// cloud metadata service at `169.254.169.254`. The available WebKit
    /// version does not expose a subresource-load filter that this crate can
    /// bind (see the module-level docs), so callers MUST treat this as an SSRF
    /// and local-file-disclosure surface:
    ///
    /// - You MUST escape/sanitize any attacker-controlled text interpolated into
    ///   the HTML so it cannot inject new elements or URLs.
    /// - You MUST NOT pass attacker-controlled URLs in `src`/`href`/`url(...)`.
    ///   OG templates should inline assets (for example `data:` URIs) rather
    ///   than referencing remote or local resources.
    ///
    /// Defense in depth applied here: the load is bounded by
    /// [`RenderOptions::timeout_secs`] so a hung or slow subresource returns
    /// [`OgImageError::Timeout`] instead of wedging the render thread.
    pub fn render_html(
        &self,
        html: &str,
        options: &RenderOptions,
    ) -> Result<OgImage, OgImageError> {
        #[cfg(wpe_stub)]
        {
            let _ = (html, options);
            Err(OgImageError::PlatformNotAvailable)
        }

        #[cfg(not(wpe_stub))]
        self.render_impl(html, options)
    }

    #[cfg(not(wpe_stub))]
    fn render_impl(&self, html: &str, options: &RenderOptions) -> Result<OgImage, OgImageError> {
        use crate::ffi;
        use crate::render_state::{on_timeout, SignalGuard, State, TimeoutGuard};
        use std::os::raw::c_void;

        // WPE snapshot captures the full document; size is set by CSS. The
        // timeout still applies as a render watchdog.
        let timeout_secs = options.effective_timeout_secs();

        // Build every fallible input BEFORE connecting the signal handler.
        //
        // The handler's user_data points at the `state` stack local below. If a
        // `?` returned between connect and disconnect, the stale handler would
        // remain installed on the persistent WebView and fire against a freed
        // stack frame on the next render (a use-after-free). Constructing the
        // CStrings here means no `?` can run between connect and disconnect, and
        // the SignalGuard makes disconnect unconditional regardless.
        let html_c = CString::new(html)
            .map_err(|e| OgImageError::LoadFailed(format!("null byte in HTML: {e}")))?;
        // Empty base URI: relative subresource references cannot resolve. This
        // does not stop absolute file://, http(s)://, or data: URLs; see the
        // SSRF note on `render_html`.
        let base_uri = CString::new("").expect("empty string has no null byte");
        let signal = CString::new("load-changed").expect("literal has no null byte");

        unsafe {
            let main_loop = ffi::g_main_loop_new(ptr::null_mut(), 0);
            let mut state = State::new(main_loop);
            let state_ptr = &mut state as *mut State as *mut c_void;

            // Connect load-changed, then immediately take the disconnect + unref
            // responsibility into an RAII guard so no later early return or panic
            // can leak the handler.
            let handler_id = ffi::g_signal_connect_data(
                self.web_view as *mut c_void,
                signal.as_ptr(),
                Some(std::mem::transmute::<
                    unsafe extern "C" fn(*mut ffi::WebKitWebView, u32, *mut c_void),
                    unsafe extern "C" fn(),
                >(on_load_changed)),
                state_ptr,
                None,
                0,
            );
            let _signal_guard =
                SignalGuard::new(self.web_view as *mut c_void, handler_id, main_loop);

            // Arm the render-timeout watchdog and guard its removal so it can
            // never fire into a freed loop after normal completion.
            let timeout_id = ffi::g_timeout_add_seconds(
                timeout_secs,
                Some(on_timeout),
                state_ptr as ffi::gpointer,
            );
            let _timeout_guard = TimeoutGuard::new(timeout_id);

            // Load HTML into the existing WebView (infallible: html_c is built).
            ffi::webkit_web_view_load_html(self.web_view, html_c.as_ptr(), base_uri.as_ptr());

            // Run until a snapshot, error, or the timeout quits the loop.
            ffi::g_main_loop_run(main_loop);

            // _signal_guard / _timeout_guard drop at end of scope: disconnect the
            // handler, unref the loop, and remove the timeout source.

            if state.timed_out {
                if !state.surface.is_null() {
                    ffi::cairo_surface_destroy(state.surface);
                }
                tracing::warn!(timeout_secs, "OG render timed out");
                return Err(OgImageError::Timeout(timeout_secs));
            }

            if let Some(err) = state.error {
                if !state.surface.is_null() {
                    ffi::cairo_surface_destroy(state.surface);
                }
                return Err(OgImageError::SnapshotFailed(err));
            }

            if state.surface.is_null() {
                return Err(OgImageError::SnapshotFailed("no surface".into()));
            }

            let surface = state.surface;
            ffi::cairo_surface_flush(surface);

            let width = ffi::cairo_image_surface_get_width(surface) as u32;
            let height = ffi::cairo_image_surface_get_height(surface) as u32;
            let stride = ffi::cairo_image_surface_get_stride(surface) as usize;
            let data = ffi::cairo_image_surface_get_data(surface);

            let pixel_bytes = if !data.is_null() && width > 0 && height > 0 {
                std::slice::from_raw_parts(data, stride * height as usize).to_vec()
            } else {
                Vec::new()
            };

            let png_bytes = cairo_surface_to_png(surface);
            ffi::cairo_surface_destroy(surface);

            Ok(OgImage {
                png_bytes,
                pixels: pixel_bytes,
                width,
                height,
            })
        }
    }
}

impl Drop for WpeRenderer {
    fn drop(&mut self) {
        #[cfg(not(wpe_stub))]
        unsafe {
            // WPE views are GObjects — unref to release
            crate::ffi::g_object_unref(self.web_view as *mut std::os::raw::c_void);
        }
    }
}

// -- GLib callbacks ----------------------------------------------------------

#[cfg(not(wpe_stub))]
unsafe extern "C" fn on_load_changed(
    web_view: *mut crate::ffi::WebKitWebView,
    load_event: u32,
    user_data: *mut std::os::raw::c_void,
) {
    use crate::ffi;

    if load_event != 3 {
        // WEBKIT_LOAD_FINISHED
        return;
    }

    let callback: ffi::GAsyncReadyCallback = Some(on_snapshot_ready_raw);
    ffi::webkit_web_view_get_snapshot(
        web_view,
        1, // WEBKIT_SNAPSHOT_REGION_FULL_DOCUMENT
        0, // WEBKIT_SNAPSHOT_OPTIONS_NONE
        ptr::null_mut(),
        callback,
        user_data,
    );
}

#[cfg(not(wpe_stub))]
unsafe extern "C" fn on_snapshot_ready_raw(
    source: *mut crate::ffi::GObject,
    result: *mut crate::ffi::GAsyncResult,
    user_data: crate::ffi::gpointer,
) {
    on_snapshot_ready(
        source as *mut std::os::raw::c_void,
        result as *mut std::os::raw::c_void,
        user_data as *mut std::os::raw::c_void,
    );
}

#[cfg(not(wpe_stub))]
unsafe fn on_snapshot_ready(
    source: *mut std::os::raw::c_void,
    result: *mut std::os::raw::c_void,
    user_data: *mut std::os::raw::c_void,
) {
    use crate::ffi;
    use crate::render_state::State;
    use std::ffi::CStr;

    let state = &mut *(user_data as *mut State);
    let web_view = source as *mut ffi::WebKitWebView;

    let mut error: *mut ffi::GError = ptr::null_mut();
    let surface = ffi::webkit_web_view_get_snapshot_finish(
        web_view,
        result as *mut ffi::GAsyncResult,
        &mut error,
    );

    if !error.is_null() {
        let msg = CStr::from_ptr((*error).message)
            .to_string_lossy()
            .into_owned();
        ffi::g_error_free(error);
        state.error = Some(msg);
    } else if surface.is_null() {
        state.error = Some("snapshot returned null surface".into());
    } else {
        // Copy the snapshot into our own surface, then destroy the original.
        //
        // webkit_web_view_get_snapshot_finish is annotated (transfer full) in
        // the WebKit GIR: ownership of the returned cairo_surface_t passes to
        // the caller, which must cairo_surface_destroy it. The previous code
        // copied into `copy` but never destroyed `surface`, leaking the full
        // snapshot (~3 MB at 1200x630) on every render.
        let w = ffi::cairo_image_surface_get_width(surface);
        let h = ffi::cairo_image_surface_get_height(surface);
        let copy = ffi::cairo_image_surface_create(0, w, h); // CAIRO_FORMAT_ARGB32
        let cr = ffi::cairo_create(copy);
        ffi::cairo_set_source_surface(cr, surface, 0.0, 0.0);
        ffi::cairo_paint(cr);
        ffi::cairo_destroy(cr);
        ffi::cairo_surface_destroy(surface);
        state.surface = copy;
    }

    ffi::g_main_loop_quit(state.main_loop);
}

// -- PNG encoding ------------------------------------------------------------

#[cfg(not(wpe_stub))]
unsafe fn cairo_surface_to_png(surface: *mut crate::ffi::cairo_surface_t) -> Vec<u8> {
    use crate::ffi;
    use std::os::raw::c_void;

    struct PngWriter {
        buf: Vec<u8>,
    }

    unsafe extern "C" fn write_fn(closure: *mut c_void, data: *const u8, length: u32) -> u32 {
        let writer = &mut *(closure as *mut PngWriter);
        writer
            .buf
            .extend_from_slice(std::slice::from_raw_parts(data, length as usize));
        0 // CAIRO_STATUS_SUCCESS
    }

    let mut writer = PngWriter {
        buf: Vec::with_capacity(64 * 1024),
    };

    ffi::cairo_surface_write_to_png_stream(
        surface,
        Some(write_fn),
        &mut writer as *mut PngWriter as *mut c_void,
    );

    writer.buf
}
