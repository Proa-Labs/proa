// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[cfg(not(wpe_stub))]
use std::ffi::CString;
#[cfg(not(wpe_stub))]
use std::ptr;
#[cfg(not(wpe_stub))]
use std::sync::Once;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OgImageError {
    #[error("WebKitGTK is not available on this platform")]
    PlatformNotAvailable,
    #[error("GTK initialization failed")]
    GtkInit,
    #[error("failed to load HTML: {0}")]
    LoadFailed(String),
    #[error("snapshot failed: {0}")]
    SnapshotFailed(String),
    #[error("PNG encoding failed: {0}")]
    PngEncode(String),
    #[error("render timed out after {0} seconds")]
    Timeout(u32),
}

/// Hard upper bound on render dimensions.
///
/// A snapshot surface is allocated at `width * height * 4` bytes, so an
/// unbounded caller-controlled size is a memory-exhaustion vector. Both axes
/// are clamped to this maximum before any allocation happens.
pub const MAX_DIMENSION: u32 = 4096;

/// Default per-render timeout in seconds.
///
/// `g_main_loop_run` blocks until the page reaches `LOAD_FINISHED`. A hung or
/// slow remote subresource would otherwise wedge the render thread forever, so
/// a timeout source quits the loop and surfaces an [`OgImageError::Timeout`].
pub const DEFAULT_TIMEOUT_SECS: u32 = 10;

/// Options for rendering an OG image.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Width in pixels (default: 1200, clamped to the renderer's dimension limit)
    pub width: u32,
    /// Height in pixels (default: 630, clamped to the renderer's dimension limit)
    pub height: u32,
    /// Whether to use a transparent background
    pub transparent: bool,
    /// Maximum time to wait for the page to finish loading, in seconds.
    ///
    /// When the deadline elapses the render is aborted and
    /// [`OgImageError::Timeout`] is returned instead of hanging. Defaults to
    /// 10 seconds.
    pub timeout_secs: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 630,
            transparent: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

impl RenderOptions {
    /// Width clamped to at least 1 and at most [`MAX_DIMENSION`].
    #[cfg_attr(wpe_stub, allow(dead_code))]
    #[inline]
    fn clamped_width(&self) -> u32 {
        self.width.clamp(1, MAX_DIMENSION)
    }

    /// Height clamped to at least 1 and at most [`MAX_DIMENSION`].
    #[cfg_attr(wpe_stub, allow(dead_code))]
    #[inline]
    fn clamped_height(&self) -> u32 {
        self.height.clamp(1, MAX_DIMENSION)
    }

    /// Timeout in seconds, falling back to [`DEFAULT_TIMEOUT_SECS`] when zero so
    /// a misconfigured `0` can never disable the watchdog.
    #[cfg_attr(wpe_stub, allow(dead_code))]
    #[inline]
    fn effective_timeout_secs(&self) -> u32 {
        if self.timeout_secs == 0 {
            DEFAULT_TIMEOUT_SECS
        } else {
            self.timeout_secs
        }
    }
}

/// The rendered OG image.
pub struct OgImage {
    /// PNG-encoded bytes
    pub png_bytes: Vec<u8>,
    /// Raw BGRA8 pixel data (cairo native format)
    pub pixels: Vec<u8>,
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
}

#[cfg(not(wpe_stub))]
static GTK_INIT: Once = Once::new();
// Replaces a former `static mut bool`: an atomic removes the deprecated
// `static mut` (a hard error under edition 2024) while keeping the
// `Once`-guarded one-time initialization.
#[cfg(not(wpe_stub))]
static GTK_INIT_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// WebKit headless renderer for OG image generation.
///
/// Keeps a persistent WebView and offscreen window alive between renders.
/// Reuse a single `OgRenderer` for best performance — subsequent renders
/// skip WebView/window creation and just swap HTML content.
pub struct OgRenderer {
    #[cfg(not(wpe_stub))]
    window: *mut crate::ffi::GtkWidget,
    #[cfg(not(wpe_stub))]
    web_view: *mut crate::ffi::WebKitWebView,
}

impl OgRenderer {
    /// Create a new renderer with a persistent WebView.
    ///
    /// Must be called from the main thread. Requires a display server
    /// or Xvfb virtual framebuffer (use `xvfb-run` in Docker).
    pub fn new() -> Result<Self, OgImageError> {
        #[cfg(wpe_stub)]
        {
            Err(OgImageError::PlatformNotAvailable)
        }

        #[cfg(not(wpe_stub))]
        {
            use crate::ffi;

            use std::sync::atomic::Ordering;

            GTK_INIT.call_once(|| {
                let ok = unsafe { ffi::gtk_init_check(ptr::null_mut(), ptr::null_mut()) };
                GTK_INIT_OK.store(ok != 0, Ordering::SeqCst);
            });

            if !GTK_INIT_OK.load(Ordering::SeqCst) {
                return Err(OgImageError::GtkInit);
            }

            unsafe {
                let window = ffi::gtk_offscreen_window_new();
                ffi::gtk_window_set_default_size(window as *mut ffi::GtkWindow, 1200, 630);

                let settings = ffi::webkit_settings_new();
                ffi::webkit_settings_set_hardware_acceleration_policy(
                    settings, 2, // WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER
                );
                ffi::webkit_settings_set_enable_javascript(settings, 0);

                let web_view = ffi::webkit_web_view_new_with_settings(settings);
                ffi::gtk_widget_set_size_request(web_view as *mut ffi::GtkWidget, 1200, 630);
                ffi::gtk_container_add(
                    window as *mut ffi::GtkContainer,
                    web_view as *mut ffi::GtkWidget,
                );
                ffi::gtk_widget_show_all(window);

                Ok(Self {
                    window,
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
    /// Defense in depth applied here: width and height are clamped to
    /// the renderer's fixed dimension limit before any surface is allocated,
    /// and the load is bounded by [`RenderOptions::timeout_secs`] so a hung or
    /// slow subresource returns [`OgImageError::Timeout`] instead of wedging the
    /// render thread.
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

        // Clamp caller-controlled dimensions before any allocation: the snapshot
        // surface is width * height * 4 bytes, so an unbounded size is a
        // memory-exhaustion vector.
        let width = options.clamped_width();
        let height = options.clamped_height();
        let timeout_secs = options.effective_timeout_secs();

        // Build every fallible input BEFORE connecting the signal handler.
        //
        // The handler's user_data points at the `state` stack local below. If a
        // `?` returned between connect and disconnect, the stale handler would
        // remain installed on the persistent WebView and fire against a freed
        // stack frame on the next render (a use-after-free). Constructing the
        // CStrings here means the only fallible step left after connect is loop
        // setup, and the SignalGuard makes disconnect unconditional regardless.
        let html_c = CString::new(html)
            .map_err(|e| OgImageError::LoadFailed(format!("null byte in HTML: {e}")))?;
        // Empty base URI: relative subresource references cannot resolve. This
        // does not stop absolute file://, http(s)://, or data: URLs; see the
        // SSRF note on `render_html`.
        let base_uri = CString::new("").expect("empty string has no null byte");
        let signal = CString::new("load-changed").expect("literal has no null byte");

        // Resize if options differ from current size.
        unsafe {
            ffi::gtk_window_set_default_size(
                self.window as *mut ffi::GtkWindow,
                width as i32,
                height as i32,
            );
            ffi::gtk_widget_set_size_request(
                self.web_view as *mut ffi::GtkWidget,
                width as i32,
                height as i32,
            );
        }

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

            let out_width = ffi::cairo_image_surface_get_width(surface) as u32;
            let out_height = ffi::cairo_image_surface_get_height(surface) as u32;
            let stride = ffi::cairo_image_surface_get_stride(surface) as usize;
            let data = ffi::cairo_image_surface_get_data(surface);

            let pixel_bytes = if !data.is_null() && out_width > 0 && out_height > 0 {
                std::slice::from_raw_parts(data, stride * out_height as usize).to_vec()
            } else {
                Vec::new()
            };

            let png_bytes = cairo_surface_to_png(surface);
            ffi::cairo_surface_destroy(surface);

            Ok(OgImage {
                png_bytes,
                pixels: pixel_bytes,
                width: out_width,
                height: out_height,
            })
        }
    }
}

impl Drop for OgRenderer {
    fn drop(&mut self) {
        #[cfg(not(wpe_stub))]
        unsafe {
            crate::ffi::gtk_widget_destroy(self.window);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_clamp_to_max() {
        let opts = RenderOptions {
            width: 100_000,
            height: 0,
            ..RenderOptions::default()
        };
        assert_eq!(opts.clamped_width(), MAX_DIMENSION);
        // Zero clamps up to at least 1 so cairo never sees a zero-area surface.
        assert_eq!(opts.clamped_height(), 1);
    }

    #[test]
    fn zero_timeout_falls_back_to_default() {
        let opts = RenderOptions {
            timeout_secs: 0,
            ..RenderOptions::default()
        };
        assert_eq!(opts.effective_timeout_secs(), DEFAULT_TIMEOUT_SECS);

        let opts = RenderOptions {
            timeout_secs: 3,
            ..RenderOptions::default()
        };
        assert_eq!(opts.effective_timeout_secs(), 3);
    }

    /// Under stub bindings (no WebKit present), every render reports the
    /// platform as unavailable. The embedded-NUL use-after-free regression test
    /// below requires real bindings, so it is gated out of stub builds.
    #[cfg(wpe_stub)]
    #[test]
    fn stub_reports_platform_unavailable() {
        // OgRenderer does not implement Debug, so inspect the error directly
        // rather than matching on the whole Result.
        match OgRenderer::new() {
            Ok(_) => panic!("expected PlatformNotAvailable under stubs, got Ok"),
            Err(OgImageError::PlatformNotAvailable) => {}
            Err(other) => panic!("expected PlatformNotAvailable under stubs, got {other:?}"),
        }
    }

    /// Regression test for the CString-error-path use-after-free (finding 1).
    ///
    /// An HTML string containing an embedded NUL must be rejected cleanly with
    /// an error, and a subsequent render on the SAME persistent renderer must
    /// still succeed. Before the fix, the NUL `?` returned after the signal
    /// handler was connected but before it was disconnected, leaving a stale
    /// handler whose user_data pointed at the freed stack frame; the next render
    /// fired it and corrupted memory.
    ///
    /// Real bindings only: needs a live WebKit WebView. Skips gracefully when no
    /// display/Xvfb is available so it is a no-op in headless CI without a
    /// framebuffer rather than a failure.
    #[cfg(not(wpe_stub))]
    #[test]
    fn embedded_nul_is_rejected_then_renderer_still_works() {
        let renderer = match OgRenderer::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skipping embedded-NUL regression test: {e}");
                return;
            }
        };

        let opts = RenderOptions::default();

        // First render: HTML with an embedded NUL must be rejected cleanly.
        let bad_html = "<html><body>before\0after</body></html>";
        let first = renderer.render_html(bad_html, &opts);
        assert!(
            matches!(first, Err(OgImageError::LoadFailed(_))),
            "embedded NUL should be rejected with LoadFailed, got {first:?}"
        );

        // Second render on the same renderer must still work: proves the stale
        // handler was not left installed (no use-after-free / crash).
        let good_html = "<html><body style=\"width:64px;height:64px\">ok</body></html>";
        let second = renderer.render_html(good_html, &opts);
        assert!(
            second.is_ok(),
            "render after a rejected NUL input should still succeed, got {second:?}"
        );
    }
}
