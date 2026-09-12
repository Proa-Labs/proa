// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Email rendering target (stub for Phase 2)
//!
//! This module will provide functionality for rendering components to email-compatible HTML.
//! Email rendering requires special considerations:
//! - Inline styles instead of CSS classes
//! - Table-based layouts for compatibility
//! - Limited CSS support
//! - VML fallbacks for Outlook
//!
//! **Status**: Stub for Phase 2 implementation

pub mod context;

pub use context::EmailContext;
