// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Variant trait for CSS class generation
//!
//! Provides a common interface for variant enums used in component styling.
//! All implementations should use const fn for compile-time class resolution.

/// Trait for variant enums that provide CSS classes
///
/// Implementations should be const fn where possible, enabling compile-time
/// class resolution for known variant combinations.
///
/// # Example
///
/// ```ignore
/// use proa_core::Variant;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// pub enum ButtonSize {
///     Sm,
///     #[default]
///     Md,
///     Lg,
/// }
///
/// impl Variant for ButtonSize {
///     fn classes(self) -> &'static str {
///         match self {
///             Self::Sm => "h-8 px-3",
///             Self::Md => "h-9 px-4",
///             Self::Lg => "h-10 px-6",
///         }
///     }
/// }
/// ```
pub trait Variant: Copy + Default {
    /// Returns the CSS classes for this variant as a static string slice
    fn classes(self) -> &'static str;
}
