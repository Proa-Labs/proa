// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Static consolidation for the `html!` / `html_sync!` / `xml!` macros.
//!
//! The semantic core (validation pass, compile-time escaping, static
//! rendering, and the chunking walk with its [`ConsolidationSink`] seam)
//! lives in [`proa_dsl::consolidate`], because this crate is
//! `proc-macro = true` and cannot export types to a runtime template
//! compiler that shares the core.
//! This module re-exports the core under its original path and keeps the
//! one macro-specific piece: the `TokenStream` emitter in
//! [`emit_tokens`], the sink implementation that materializes the walk's
//! event sequence as render-closure statements.

pub(crate) use proa_dsl::consolidate::{
    escape_attr_compile_time, escape_text_compile_time, MarkupDialect,
};
pub use proa_dsl::consolidate::{validate_security, validate_xml};

#[allow(unused_imports)] // seam documentation anchor for emit_tokens
pub(crate) use proa_dsl::consolidate::sink::ConsolidationSink;

mod emit_tokens;
#[cfg(test)]
mod golden_tests;

pub use emit_tokens::{
    generate_optimized, generate_optimized_async, generate_optimized_async_xml,
    generate_optimized_send_async, generate_optimized_xml,
};
