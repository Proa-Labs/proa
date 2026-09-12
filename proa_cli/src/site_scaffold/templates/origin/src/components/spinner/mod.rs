// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND (ISC AND MIT)

#![allow(clippy::module_inception)]
//! Spinner component - animated loading indicator
//!
//! The Spinner component provides a rotating loader icon for indicating loading states.
//! Uses the Lucide loader-circle icon with CSS animation.

mod md;
mod recipe;
mod spinner;

// Re-export spinner types
pub use spinner::Spinner;

// Re-export recipe types
pub use recipe::{
    spinner_classes, spinner_recipe, SpinnerRecipe, SpinnerRecipeProps, SpinnerSize, SpinnerStyle,
    SPINNER_RECIPE,
};

// Re-export metadata export function (only when feature is enabled)
