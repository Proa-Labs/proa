// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Parser for the `html!` / `html_sync!` macro syntax.
//!
//! The parsing types and `syn::parse::Parse` impls live in the
//! [`proa_dsl`] crate so they can be shared with library crates that cannot
//! depend on this proc-macro crate directly. This module simply re-exports
//! them under their original path so the rest of `proa_macros` keeps working
//! unchanged (`use crate::parse::{Attribute, Element, HtmlRoot, Node}`).

pub use proa_dsl::parse::{Attribute, Element, HtmlRoot, Node, XmlRoot};
