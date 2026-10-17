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
//! (strategy explanations and crossing reports) lives in [`proa_source_tools::rsjs`] and is
//! re-exported at this module's root.

pub use proa_source_tools::rsjs::*;

pub use proa_build::{abi_adapter, build, cache, final_compile, link, pipeline, semantic_link};

#[doc(hidden)]
pub use proa_build::timing;
