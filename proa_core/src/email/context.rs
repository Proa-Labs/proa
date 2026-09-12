// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Email rendering context (stub for Phase 2)

/// Context for rendering email-compatible HTML (stub).
///
/// This will be fully implemented in Phase 2 with support for:
/// - Inline styles instead of CSS classes
/// - Table-based layouts for email client compatibility
/// - VML fallbacks for Outlook
/// - Limited CSS support matching email client capabilities
///
/// **Status**: Stub for Phase 2 implementation
pub struct EmailContext<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}
