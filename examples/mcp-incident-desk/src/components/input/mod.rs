// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Input component - form text input field
//!
//! The Input component provides a styled text input with support for
//! various input types, sizes, and validation states.

mod input;
mod md;
mod recipe;

// Re-export input types

pub use input::Input;

// Re-export recipe types
pub use recipe::{
    input_classes, input_recipe, InputRecipe, InputRecipeProps, InputSize, InputStyle, InputType,
    INPUT_RECIPE,
};
