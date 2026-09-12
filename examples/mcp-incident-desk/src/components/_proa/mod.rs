// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared support for source-installed Proa UI components.
#![allow(ambiguous_glob_reexports, dead_code)]

pub mod anatomy;
pub mod collection;
pub mod component_macros;
pub mod component_tree;
pub mod form;
pub(crate) mod free_web;
pub mod md_support;
pub mod overlay;
pub mod recipe;
pub mod slots;
pub mod state;
pub mod theme;
pub mod tokens;
pub mod traits;
pub mod variant_spec;

pub use crate::{render_html, tw, tw_join};
pub use anatomy::*;
pub use collection::*;
pub use component_tree::*;
pub use form::*;
pub use md_support::*;
pub use overlay::*;
pub use recipe::*;
pub use slots::*;
pub use state::*;
pub use theme::*;
pub use tokens::*;
pub use variant_spec::*;
