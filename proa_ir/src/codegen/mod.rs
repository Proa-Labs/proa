// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Code generation from IR to JavaScript
//!
//! This module compiles FormIR to optimized JavaScript.
//!
//! ## Architecture
//!
//! The codegen is split into two parts:
//!
//! 1. **Runtime** (`runtime.rs`) - The shared library (~2KB) that provides:
//!    - Form class with validation, state management, effects
//!    - Helper functions (debounce, toast, etc.)
//!    - This should be loaded once from a CDN and cached
//!
//! 2. **Per-form config** (`form.rs`) - Tiny per-form code (~300-500 bytes):
//!    - Just the form configuration (fields, rules, handlers)
//!    - Imports from the runtime
//!
//! ## Usage
//!
//! ```rust,ignore
//! use proa_ir::codegen::{FormCodegen, FormCodegenConfig, generate_runtime};
//!
//! // Generate the runtime once (host on CDN)
//! let runtime_js = generate_runtime();
//!
//! // Generate per-form scripts
//! let form_script = FormCodegen::generate_script(&form_ir, FormCodegenConfig::default());
//! ```

mod form;
mod js;
mod runtime;

// Legacy full codegen (includes everything inline)
pub use js::{expr_to_js, JsCodegen};

// New split codegen
pub use form::{FormCodegen, FormCodegenConfig};
pub use runtime::{
    generate_runtime, generate_runtime_minified, runtime_url, DEFAULT_CDN_URL, RUNTIME_VERSION,
};
