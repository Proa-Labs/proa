// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Traits and conventions for Proa UI components
//!
//! ## Component Styling Convention
//!
//! Components with recipe-based styling should implement a `get_classes` method
//! that returns their CSS classes from a recipe.
//!
//! Since Rust 1.90.0 doesn't support generic const expressions in trait methods,
//! we use a convention where each component defines their own `get_classes` method
//! with a concrete array size.
//!
//! # Example
//!
//! ```ignore
//! impl Button {
//!     /// Get Tailwind CSS classes from the ButtonRecipe
//!     pub fn get_classes(&self) -> ([&'static str; ButtonRecipe::CLASS_CAPACITY], usize) {
//!         button_recipe().classes(self.size, self.color, self.variant, self.radius)
//!     }
//! }
//! ```
//!
//! The `ProaComponent` derive macro expects a `get_classes` method on the component.
