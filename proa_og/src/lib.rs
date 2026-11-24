// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! # proa_og
//!
//! Open Graph image generation with WebKitGTK and Cairo.
//!
//! WebKit lays out HTML and CSS in an offscreen view. The renderer captures
//! the configured viewport and returns its pixels and a PNG.
//!
//! ## Architecture
//!
//! ```text
//! HTML + CSS string
//!   → GTK offscreen window
//!   → WebKitWebView (full WebCore layout engine)
//!   → snapshot → BGRA8 pixel buffer
//!   → PNG encoding
//! ```
//!
//! ## Platform
//!
//! Native snapshots require Linux with WebKitGTK and Cairo. Stock WPE SDKs
//! lack the Cairo snapshot API. On those SDKs and on hosts without WebKitGTK,
//! renderer construction returns [`OgImageError::PlatformNotAvailable`].
//!
//! ## Security
//!
//! Rendering runs the real WebKit engine. JavaScript is disabled (no scripting,
//! no `XMLHttpRequest`/`fetch`), but `webkit_web_view_load_html` still resolves
//! subresources referenced in the markup: `<img src>`, CSS `@import` and
//! `url(...)`, and web fonts. Those loads can reach `file://` paths and internal
//! `http(s)://` endpoints, including the cloud metadata service at
//! `169.254.169.254`. Interpolating attacker-controlled text or URLs into the OG
//! HTML is therefore an SSRF and local-file-disclosure surface.
//!
//! Ideally the renderer would install a resource-load filter that denies every
//! subresource except `data:` URIs (OG templates should inline their assets).
//! The WebKit versions this crate targets do not expose a subresource-load veto
//! that the current bindings can drive (`resource-load-started` reports loads
//! but does not reliably cancel them across the supported 2.x line, and
//! `decide-policy` does not fire for every subresource), so that filter is not
//! installed here. This is a known limitation; see the work tracked for binding
//! a network-process filter once a stable hook is available.
//!
//! Until then, the mitigations in place are:
//!
//! - JavaScript is disabled.
//! - HTML is loaded with an empty base URI, so relative subresource references
//!   cannot resolve (absolute `file://`/`http(s)://`/`data:` still can).
//! - The viewport is clamped before rendering. Snapshot dimensions are also
//!   checked before allocating the output pixel buffer.
//! - Each render is bounded by a configurable timeout
//!   ([`RenderOptions::timeout_secs`]); a hung or slow subresource yields
//!   [`OgImageError::Timeout`] rather than wedging the render thread.
//!
//! Callers MUST sanitize/escape any interpolated values and MUST NOT pass
//! attacker-controlled URLs. See [`OgRenderer::render_html`] for the per-call
//! contract.

mod ffi;
#[cfg(not(wpe_stub))]
mod render_state;
mod renderer;

#[cfg(feature = "wpe")]
mod wpe_renderer;

#[cfg(not(feature = "wpe"))]
pub use renderer::OgRenderer;
pub use renderer::{OgImage, OgImageError, RenderOptions};
#[cfg(feature = "wpe")]
pub use wpe_renderer::WpeRenderer as OgRenderer;

#[cfg(feature = "wpe")]
pub use wpe_renderer::WpeRenderer;
