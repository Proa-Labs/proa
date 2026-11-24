// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(not(wpe_stub), not(feature = "wpe")))]
use std::ptr;
#[cfg(all(not(wpe_stub), not(feature = "wpe")))]
use std::sync::Once;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OgImageError {
    #[error("native snapshots are unavailable for the selected WebKit backend")]
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
    /// Maximum time for the page load and snapshot to complete, in seconds.
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
    pub(crate) fn effective_timeout_secs(&self) -> u32 {
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

#[cfg(all(not(wpe_stub), not(feature = "wpe")))]
static GTK_INIT: Once = Once::new();
// Replaces a former `static mut bool`: an atomic removes the deprecated
// `static mut` (a hard error under edition 2024) while keeping the
// `Once`-guarded one-time initialization.
#[cfg(all(not(wpe_stub), not(feature = "wpe")))]
static GTK_INIT_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// WebKit headless renderer for OG image generation.
///
/// Keeps a persistent WebView and offscreen window alive between renders.
/// Reuse a single `OgRenderer` for best performance — subsequent renders
/// skip WebView/window creation and just swap HTML content.
#[cfg(not(feature = "wpe"))]
pub struct OgRenderer {
    #[cfg(not(wpe_stub))]
    window: *mut crate::ffi::GtkWidget,
    #[cfg(not(wpe_stub))]
    web_view: *mut crate::ffi::WebKitWebView,
}

#[cfg(not(feature = "wpe"))]
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
        let width = options.clamped_width();
        let height = options.clamped_height();

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
            crate::render_state::capture(self.web_view, html, options.effective_timeout_secs())
        }
    }
}

#[cfg(not(feature = "wpe"))]
impl Drop for OgRenderer {
    fn drop(&mut self) {
        #[cfg(not(wpe_stub))]
        unsafe {
            crate::ffi::gtk_widget_destroy(self.window);
        }
    }
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
}
