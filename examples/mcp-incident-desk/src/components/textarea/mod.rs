// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Textarea component - form multi-line text input field
//!
//! The Textarea component provides a styled multi-line text input with support for
//! various sizes, validation states, and form integration via proa_ir.

mod md;
mod recipe;
mod textarea;

// Re-export textarea types
pub use textarea::{Textarea, TextareaWrap};

// Re-export recipe types
pub use recipe::{
    textarea_base_classes, textarea_classes, textarea_recipe, TextareaRecipe, TextareaRecipeProps,
    TextareaSize, TextareaStyle, TEXTAREA_RECIPE,
};
