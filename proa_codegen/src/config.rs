// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Configuration for TypeScript code generation

use serde::{Deserialize, Serialize};

/// Configuration for code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodegenConfig {
    /// Target React version
    pub react_version: ReactVersion,

    /// Whether to use forwardRef (default: true for React 18 compatibility)
    pub use_forward_ref: bool,

    /// Whether to add data-slot attributes (default: true)
    pub add_data_slot: bool,

    /// Whether to use named functions instead of const with displayName (default: false)
    pub use_named_functions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReactVersion {
    /// React 18 (stable) - requires forwardRef
    React18,
    /// React 19+ (beta/future) - ref as prop
    React19,
}

impl Default for CodegenConfig {
    fn default() -> Self {
        Self {
            react_version: ReactVersion::React18,
            use_forward_ref: true,
            add_data_slot: true,
            use_named_functions: false,
        }
    }
}

// Config presets - available for users to choose in main.rs
#[allow(dead_code)]
impl CodegenConfig {
    /// Create config optimized for React 18 (current stable)
    pub fn react_18() -> Self {
        Self {
            react_version: ReactVersion::React18,
            use_forward_ref: true,
            add_data_slot: true,
            use_named_functions: false,
        }
    }

    /// Create config optimized for React 19+ (future)
    /// Major changes are that it does NOT use forward ref
    pub fn react_19() -> Self {
        Self {
            react_version: ReactVersion::React19,
            use_forward_ref: false,
            add_data_slot: true,
            use_named_functions: true,
        }
    }

    /// Create config matching shadcn/ui v4 patterns
    pub fn shadcn_v4() -> Self {
        Self {
            react_version: ReactVersion::React19,
            use_forward_ref: false,
            add_data_slot: true,
            use_named_functions: true,
        }
    }
}

impl ReactVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            ReactVersion::React18 => "18",
            ReactVersion::React19 => "19",
        }
    }
}
