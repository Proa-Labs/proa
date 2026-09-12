// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Static action data types for compile-time known actions.
//!
//! These types use `&'static str` for all fields, allowing them to be:
//! - Fully stack-allocated (no heap allocations)
//! - Used in const contexts
//! - Zero runtime overhead
//!
//! This enables compile-time optimization of entire component subtrees.

use serde_json::Value;

/// Static action data - fully stack-allocated and const-friendly.
///
/// All fields are `&'static str`, making this type:
/// - Copy-able
/// - Usable in const contexts
/// - Zero heap allocations
/// - Perfect for compile-time known action handlers
///
/// # Example
///
/// ```ignore
/// const BUTTON_ACTION: ActionDataStatic = ActionDataStatic {
///     action: "handleClick",
///     params: "{}",  // JSON string
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDataStatic {
    /// The action identifier (e.g., "handleClick")
    pub action: &'static str,

    /// JSON-encoded parameters as a static string
    pub params: &'static str,
}

impl ActionDataStatic {
    /// Create a new static action with no parameters.
    pub const fn new(action: &'static str) -> Self {
        Self {
            action,
            params: "{}",
        }
    }

    /// Create a new static action with JSON parameters.
    pub const fn with_params(action: &'static str, params: &'static str) -> Self {
        Self { action, params }
    }

    /// Parse the params string as JSON Value (runtime operation).
    pub fn params_value(&self) -> Value {
        serde_json::from_str(self.params).unwrap_or(Value::Object(Default::default()))
    }
}
