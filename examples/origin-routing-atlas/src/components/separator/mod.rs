// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Separator component - a visual divider between content sections
//!
//! The Separator component renders a horizontal or vertical line to visually
//! divide content. It supports decorative mode (default) for purely visual
//! separators and semantic mode for accessible content division.

mod md;
mod recipe;
mod separator;

// Re-export separator types
pub use separator::Separator;

// Re-export recipe types
pub use recipe::{
    separator_classes, separator_recipe, SeparatorOrientation, SeparatorRecipe,
    SeparatorRecipeProps, SeparatorStyle, SEPARATOR_RECIPE,
};
