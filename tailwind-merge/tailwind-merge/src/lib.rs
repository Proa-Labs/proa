// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Tailwind merge engines for both runtime and compile-time usage.
//!
//! - [`runtime`] contains the original, allocation-friendly merger with caching support.
//! - [`compile_time`] hosts the `const fn`-capable merger backed by generated tables.
//!
//! The most common re-exports are available at the crate root for convenience.

pub mod compile_time;
mod crabtime;
pub mod runtime;

pub use compile_time::{tw_merge_const, tw_merge_const_str, ConstMergeResult, DEFAULT_MAX_CLASSES};
pub use runtime::{
    tw_merge, tw_merge_iter, ClassDefinition, ClassDefinitionEntry, Config, ConfigError,
    ParsedClassName, TailwindMerge,
};
