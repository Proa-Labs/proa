// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[cfg(not(wpe_stub))]
use std::sync::Once;

use crate::renderer::{OgImage, OgImageError, RenderOptions};

#[cfg(not(wpe_stub))]
static WPE_INIT: Once = Once::new();

/// WPE WebKit headless renderer for OG image generation.
///
/// Requires a WPE SDK that exposes Cairo snapshots. Stock WPE SDKs omit
/// that API, so construction returns [`OgImageError::PlatformNotAvailable`].
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
        unsafe {
            crate::render_state::capture(self.web_view, html, options.effective_timeout_secs())
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
