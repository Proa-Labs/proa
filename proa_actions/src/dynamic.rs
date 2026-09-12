// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Dynamic (heap-allocated) action types
//!
//! These types own their data and use heap allocation for flexibility.
//!
//! ## Use cases:
//! - Creating actions from enums with dynamic data
//! - Serialization/deserialization (JSON, etc.)
//! - Storage in databases
//! - API request/response payloads
//! - Long-lived data structures
//!
//! ## Types:
//! - `ActionData` - Owns String + serde_json::Value
//! - `ActionHandler` - Trait for action conversion
//! - `ActionMetadata` - Metadata for codegen

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Core action data structure that gets serialized to JSON and embedded in HTML
///
/// This is the heap-allocated version that owns its data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionData {
    pub action: String,
    pub params: Value,
}

impl ActionData {
    /// Serialize parameters to a JSON string
    pub fn params_json(&self) -> String {
        serde_json::to_string(&self.params).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Trait implemented by action enums via #[derive(Action)]
pub trait ActionHandler {
    /// Convert action to data that can be embedded in HTML
    fn to_action_data(&self) -> ActionData;

    /// Parse action from data (for server-side handling)
    fn from_action_data(data: ActionData) -> Result<Self, serde_json::Error>
    where
        Self: Sized;
}

/// Metadata about actions extracted at compile time for codegen
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionMetadata {
    pub scope: String,
    pub name: String,
    pub variants: Vec<ActionVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionVariant {
    pub name: String,
    pub constant: String,
    pub params: Vec<ActionParam>,
    #[serde(default)]
    pub handler: Option<ActionHandlerMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionParam {
    pub name: String,
    pub type_name: String,
}

/// Additional metadata describing the client-side handler signature.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ActionHandlerMetadata {
    /// The DOM event name (e.g. `"click"`, `"blur"`).
    pub event: Option<String>,
    /// Optional DOM element type (e.g. `"HTMLButtonElement"`).
    pub element: Option<String>,
    /// Override for the generated TypeScript event type, if desired.
    pub ts_event_type: Option<String>,
    /// Whether generated stubs should call `event.preventDefault()`.
    pub prevent_default: bool,
}
