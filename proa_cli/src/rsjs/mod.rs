// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The linked RSJS build pipeline and its inspection tools.
//!
//! Compilation flows descriptor-first: [`build`] compiles descriptor rlibs
//! behind the rustc wrapper, [`semantic_link`] and [`link`] assemble the
//! content-addressed plan, and [`final_compile`] drives the plan-sensitive
//! final Cargo compilation. [`pipeline`] orchestrates the cached end-to-end
//! run, [`cache`] owns the persistent descriptor caches, and [`abi_adapter`]
//! bridges compiler ABI revisions. The `proa rsjs` inspection surface
//! (strategy explanations and crossing reports) lives in [`inspect`] and is
//! re-exported at this module's root.

pub mod abi_adapter;
pub mod build;
pub mod cache;
pub mod final_compile;
mod inspect;
pub(crate) mod legacy_graph;
pub mod link;
pub mod pipeline;
pub mod semantic_link;

pub use inspect::*;
