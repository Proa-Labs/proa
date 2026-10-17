// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared wire types for explicitly enabled development template updates.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One generation of pre-escaped, compiler-validated static template runs.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StaticManifest {
    /// Exact development protocol version.
    pub schema: u16,
    /// Templates keyed by an explicitly authored, project-unique ID.
    pub templates: BTreeMap<String, StaticTemplate>,
}

/// A template's fixed Rust/markup shape and replaceable static runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StaticTemplate {
    /// Shape excludes only eligible static string literal contents.
    pub shape: String,
    /// Already escaped bytes, in consolidation order.
    pub slots: Vec<Vec<u8>>,
}
