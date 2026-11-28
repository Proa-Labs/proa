// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa's application-facing API.
//!
//! ```
//! use proa::{html_sync, render_to_string};
//! let name = "<Ada>";
//! let html = render_to_string(html_sync! { <p>{name}</p> });
//! assert_eq!(html, "<p>&lt;Ada&gt;</p>");
//! ```
//!
//! Rendering types and template macros are available at the root. Enable
//! `axum` for Axum integration or `actions` for action data and derives. The default `stream` feature exposes
//! loaders and streaming components without selecting a runtime; `rt-tokio`
//! enables eager Tokio loads. Markdown, streaming, framework, and RSJS are enabled
//! by default; RSJS is available through `proa::rsjs`. Build-time font tools remain
//! separate dependencies.
//!
//! The two streaming types a route or page names sit at the root beside
//! `DataLoader`: `DataResolver`, the request-scoped loader wrapper, and
//! `Suspense`, the fallback boundary. Drivers and adapter plumbing stay under
//! `proa::stream`, where both types remain available as well.

#![deny(unsafe_code)]

extern crate self as proa;

pub use proa_core::*;
pub use proa_ir as ir;
pub use proa_macros::*;
#[cfg(feature = "rsjs")]
pub use rsjs;

#[cfg(feature = "actions")]
pub use proa_actions as actions;
#[cfg(feature = "axum")]
pub use proa_framework_axum as axum;
#[cfg(feature = "framework")]
pub use proa_framework_core as framework;
#[cfg(feature = "stream")]
pub use proa_stream as stream;
#[cfg(feature = "stream")]
pub use proa_stream::{DataResolver, Suspense};

#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "actions")]
    pub use serde;
    #[cfg(feature = "actions")]
    pub use serde_json;
}
