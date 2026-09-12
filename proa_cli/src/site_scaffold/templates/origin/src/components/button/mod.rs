// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Button component - interactive element for triggering actions
//!
//! The Button component provides a type-safe builder API for creating buttons
//! with various styles, sizes, and states. Styling is handled by the ButtonRecipe
//! system which maps semantic variants to Tailwind CSS classes.

mod button;
mod md;
mod recipe;

// Re-export button types
pub use button::{Button, ButtonType, BUTTON_ANATOMY};

// Re-export recipe types (color + variant separation)
pub use recipe::{
    button_recipe, ButtonColor, ButtonRadius, ButtonRecipe, ButtonRecipeProps, ButtonSize,
    ButtonStyle, ButtonVariant, BUTTON_RECIPE,
};

// Re-export metadata export function (only when feature is enabled)
