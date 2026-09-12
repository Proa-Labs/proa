// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Literal values that can appear in initial state or attribute
//! defaults.
//!
//! [`Value`] is the v0.1 surface: the small set of primitives the
//! emitters know how to render into either Rust (for the server
//! initial-state literal) or JavaScript (for the client-side runtime
//! initial value). It deliberately does not cover collections,
//! structs, or enums; richer initial state lands when a real use case
//! demands it (likely M9+).

/// A literal value carried by the IR.
///
/// Used in two places:
///
/// 1. As the `init` field of a [`crate::reactive::SignalDecl`], i.e.
///    the value the signal holds at construction time.
/// 2. As the payload of an [`crate::client::ClientExpr::Literal`] node,
///    i.e. a constant appearing inside a reactive expression.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A boolean literal.
    Bool(bool),
    /// A 64-bit signed integer literal. Wider than i32 so the v0.1
    /// surface covers both `i32` and `i64` source types without an
    /// extra widening pass.
    ///
    /// emit_js lowers `Value::Int(n)` to a bare JS number literal. Supported
    /// `i64`/`u64` source domains never use this variant: they are represented
    /// by [`Value::BigInt`] even when the value is small. Callers constructing
    /// IR directly should keep `Value::Int`s within the JS-safe-integer range.
    Int(i64),
    /// A wide signed integer literal that must keep exact integer
    /// semantics on the client. Lowers to a JS BigInt literal
    /// (`9007199254740993n`) and, as a signal seed, to the same
    /// JSON-string wire format `rsjs::client_codec::ClientEncode` uses for the
    /// supported wide domains (`i64`, `u64`). On the server it behaves exactly
    /// like [`Value::Int`]; the split only matters to the JS emitters and
    /// seed-blob writers.
    ///
    /// The analyzer produces this variant for every explicit `i64`/`u64`
    /// literal and promotes contextual unsuffixed literals when a proven wide
    /// domain requires it. Unsupported widths (`i128`, `u128`, `isize`,
    /// `usize`) do not become supported merely by fitting this carrier; the
    /// macro rejects them with a domain diagnostic before client arithmetic.
    BigInt(i128),
    /// A 64-bit floating-point literal.
    Float(f64),
    /// An owned string literal. Owned (rather than `&'static str`)
    /// so the IR can be constructed at runtime from arbitrary
    /// authoring sources, not just compile-time string literals.
    String(String),
    /// The absence of a value. Maps to JS `null` on the client.
    Null,
}
