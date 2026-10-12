// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compiled HTML and Markdown components for people and agents.
//!
//! ```
//! use proa::{html_sync, WebContext, WebRenderSync};
//!
//! let name = "Rust & agents";
//! let mut cx = WebContext::new();
//! html_sync! { <p>{name}</p> }.render(&mut cx).unwrap();
//! assert_eq!(cx.into_output().into_string(), "<p>Rust &amp; agents</p>");
//! ```

pub use proa_actions as actions;
pub use proa_core::*;
pub use proa_ir as ir;

#[cfg(feature = "macros")]
pub use proa_macros::*;

#[cfg(feature = "axum")]
pub use proa_framework_axum as axum;
