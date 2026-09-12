// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Card component - container for grouped content
//!
//! Cards organize content into sections with header, body, and footer.

mod card;
pub mod md;
mod recipe;

/// Stable scope/part contract shared by every Card renderer.
pub const CARD_ANATOMY: crate::components::_proa::Anatomy = crate::components::_proa::Anatomy::new(
    "card",
    &[
        "root",
        "header",
        "title",
        "description",
        "content",
        "footer",
    ],
);

// Re-export card types
pub use card::{Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle};

// Re-export recipe types
pub use recipe::{
    card_classes, card_content_classes, card_description_classes, card_footer_classes,
    card_header_classes, card_recipe, card_title_classes, CardRecipe, CardRecipeProps, CardStyle,
    CARD_RECIPE,
};
