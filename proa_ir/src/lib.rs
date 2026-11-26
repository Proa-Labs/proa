// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Expressions, values, reactive attribute bindings, and component trees.
//!
//! Component libraries use the shared tree and binding data for rendering and
//! code generation. Downstream runtime-template compilers use the expression
//! evaluator and its bounded server dialect. Browser effects and event handling
//! belong to the compiled RSJS pipeline.
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
pub mod types;
pub mod value;

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
pub use types::{FieldId, IrStr, SignalId, VarId};
pub use value::Value;
