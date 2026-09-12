// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Builder APIs for constructing IR in an ergonomic way
//!
//! These builders provide a fluent API for constructing FormIR structures.

pub mod expr;
pub mod form;

pub use expr::ExprBuilder;
pub use form::{block, form, BlockBuilder, FieldBuilder, FormBuilder};
