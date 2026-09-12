// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

#![allow(clippy::module_inception)]
//! Table component - responsive data table
//!
//! Renders a styled table with header, body, footer, and caption sections.

pub mod md;
mod recipe;
mod table;

/// Stable scope/part contract shared by every Table renderer.
pub const TABLE_ANATOMY: crate::components::_proa::Anatomy = crate::components::_proa::Anatomy::new(
    "table",
    &[
        "root",
        "container",
        "header",
        "body",
        "footer",
        "row",
        "head",
        "cell",
        "caption",
    ],
);

// Re-export table types
pub use table::{
    Table, TableBody, TableCaption, TableCell, TableFooter, TableHead, TableHeader, TableRow,
};

// Re-export recipe types
pub use recipe::{
    table_body_classes, table_caption_classes, table_cell_classes, table_classes,
    table_footer_classes, table_head_classes, table_header_classes, table_recipe,
    table_row_classes, TableRecipe, TableRecipeProps, TableStyle, TABLE_RECIPE,
};
