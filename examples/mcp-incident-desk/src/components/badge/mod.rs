// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Badge component - displays a small status indicator or label
//!
//! The Badge component is used for highlighting status, categories, or counts.
//! It supports multiple variants for different visual styles.

mod badge;
mod md;
mod recipe;

// Re-export badge types
pub use badge::Badge;

// Re-export recipe types
pub use recipe::{
    badge_classes, badge_recipe, BadgeRecipe, BadgeRecipeProps, BadgeStyle, BadgeVariant,
    BADGE_RECIPE,
};
