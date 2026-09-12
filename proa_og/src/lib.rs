// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! # proa_og
//!
//! OG image generation powered by WPE WebKit's headless rendering engine.
//!
//! Uses the real WebKit CSS/layout engine for pixel-perfect HTML/CSS rendering,
//! then captures the result as PNG. No Yoga, no Taffy, no CSS subset — the same
//! engine that powers Safari and GNOME Web.
//!
//! ## Architecture
//!
//! ```text
//! HTML + CSS string
//!   → WPE headless display (no GPU, no window server)
//!   → WebKitWebView (full WebCore layout engine)
//!   → snapshot → BGRA8 pixel buffer
//!   → PNG encoding
//! ```
//!
//! ## Platform
//!
//! Requires Linux with WPE WebKit development libraries installed.
//! On macOS, the crate compiles with stub bindings for IDE support,
//! but panics at runtime. Use the provided Dockerfile for local development.
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
//! - Render dimensions are clamped to the renderer's fixed dimension limit
//!   before any surface is allocated, bounding memory use.
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

pub use renderer::{OgImage, OgImageError, OgRenderer, RenderOptions};

#[cfg(feature = "wpe")]
pub use wpe_renderer::WpeRenderer;
