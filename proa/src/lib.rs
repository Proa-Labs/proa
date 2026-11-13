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
//! `axum` for Axum integration, `actions` for action data and derives,
//! or `react` for React interoperability. The default `stream` feature exposes
//! loaders and streaming components without selecting a runtime; `rt-tokio`
//! enables eager Tokio loads. Markdown, streaming, framework, and RSJS are enabled
//! by default; RSJS is available through `proa::rsjs`. Build-time font tools remain
//! separate dependencies.

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
#[cfg(feature = "react")]
pub use proa_react as react;
#[cfg(feature = "stream")]
pub use proa_stream as stream;

#[doc(hidden)]
pub mod __private {
    #[cfg(any(feature = "actions", feature = "react"))]
    pub use serde;
    #[cfg(any(feature = "actions", feature = "react"))]
    pub use serde_json;
}
