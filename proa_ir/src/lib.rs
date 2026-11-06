// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa IR - Intermediate Representation for Client-Side Interactivity
//!
//! This crate provides an IR (Intermediate Representation) that describes
//! client-side behavior for interactive components. The IR is
//! designed to be:
//!
//! - **Complete**: Express interaction logic without escape hatches
//! - **Type-safe**: Catch errors at compile time in Rust
//! - **Optimizable**: Enable dead code elimination and other optimizations
//! - **Portable**: Compile to JavaScript, WASM, or other targets
//!
//! # Architecture
//!
//! The IR consists of several layers:
//!
//! - **Types**: Basic identifiers (`SignalId`, `FieldId`, `Selector`, etc.)
//! - **Values**: Literal values that can appear in the IR
//! - **Expressions**: Pure computations that produce values
//! - **Statements**: Side effects (state updates, DOM operations, fetches)
//! - **Triggers**: Events that cause effects to run
//!
//! Forms use native browser controls and application validation (for example,
//! `wellformed`); this crate does not provide a form framework.
//!
//! # Server evaluation
//!
//! Expressions also run server-side: [`eval()`] interprets the server
//! dialect of [`Expr`] against a [`ScopeStack`] (the template data scope,
//! rooted at [`Expr::Scope`]), with semantics pinned to the JavaScript
//! emitters (see `SEMANTICS.md`). [`validate_server_dialect`] rejects
//! client-only nodes at compile-to-program time, and evaluation is
//! budgeted ([`EvalBudget`]) so pathological inputs error instead of
//! hanging. `Expr` and [`Value`] are `Serialize + Deserialize`, so
//! downstream runtime-template compilers can store programs as data.

#![forbid(unsafe_code)]

pub mod dev_hot;
pub mod dialect;
pub mod eval;
pub mod expr;
pub mod reactive;
pub mod ser;
pub mod stmt;
pub mod trigger;
pub mod types;
pub mod value;

pub mod builder;
pub mod codegen;
pub mod component_tree;

// Re-export main types for convenience
pub use dialect::{
    validate_server_dialect, validate_server_dialect_bounded, DialectError, DialectErrorReason,
};
pub use eval::{eval, eval_with_budget, EvalBudget, EvalError, ScopeStack};
pub use expr::Expr;
pub use reactive::{Binding, ReactiveAttr, ReactiveClass};
pub use ser::{to_value, ToValueError};
pub use stmt::{Block, FetchOptions, Stmt};
pub use trigger::{Effect, KeyFilter, Trigger};
pub use types::{FieldId, IrStr, Method, Selector, SignalId, ToastLevel, VarId};
pub use value::Value;
