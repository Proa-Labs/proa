// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Theme Toggle component - switches the document between light and dark.
//!
//! A single icon button: moon while the page is light, sun while it is dark.
//! Clicking flips the `dark` class on `<html>`, mirrors `color-scheme`, and
//! persists the choice under the `theme` localStorage key; with nothing
//! stored, the runtime follows `prefers-color-scheme`.

use crate::components::_proa::Anatomy;

mod md;
mod recipe;
mod theme_toggle;

/// Static anatomy contract shared by SSR and the delegated runtime.
pub const THEME_TOGGLE_ANATOMY: Anatomy =
    Anatomy::new("theme-toggle", &["root", "moon-icon", "sun-icon"]);

pub use recipe::{
    theme_toggle_recipe, ThemeToggleRecipe, ThemeToggleRecipeProps, ThemeToggleStyle,
    THEME_TOGGLE_RECIPE,
};
pub use theme_toggle::ThemeToggle;
