// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Core type definitions for the IR
//!
//! These types represent identifiers and selectors used throughout the IR.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// String type used for identifiers, property keys, and string literals in
/// the IR.
///
/// Compile-time construction (proc macros, the builder API) uses
/// `Cow::Borrowed` through const-friendly constructors, so the existing
/// `&'static str` path stays allocation-free. Deserialized runtime-template
/// programs use `Cow::Owned`.
pub type IrStr = Cow<'static, str>;

/// Identifier for a signal (reactive state)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignalId(pub IrStr);

impl SignalId {
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Construct from a runtime-owned string (for example, deserialization).
    pub fn owned(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifier for a form field
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldId(pub IrStr);

impl FieldId {
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Construct from a runtime-owned string (for example, deserialization).
    pub fn owned(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifier for a local variable within a block
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VarId(pub IrStr);

impl VarId {
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Construct from a runtime-owned string (for example, deserialization).
    pub fn owned(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// CSS selector for DOM elements
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Selector(pub IrStr);

impl Selector {
    pub const fn new(selector: &'static str) -> Self {
        Self(Cow::Borrowed(selector))
    }

    /// Create a selector from a field name (e.g., `[name="email"]`)
    pub fn from_field(field: &FieldId) -> Self {
        Self(field.0.clone())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// HTTP method for fetch requests
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

/// Toast notification level
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

/// Content type for fetch requests
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    #[default]
    Json,
    FormData,
    UrlEncoded,
    Text,
}

/// Response type for fetch requests
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseType {
    #[default]
    Json,
    Text,
    Blob,
}
