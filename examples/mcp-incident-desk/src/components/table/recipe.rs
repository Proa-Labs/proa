// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Table recipe — static named slots for responsive table parts.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

mod base {
    use super::*;

    pub const CONTAINER: &str = tw_join!("relative", "w-full", "overflow-auto");
    pub const ROOT: &str = tw_join!("w-full", "caption-bottom", "text-sm");
    pub const HEADER: &str = "[&_tr]:border-b";
    pub const BODY: &str = "[&_tr:last-child]:border-0";
    pub const FOOTER: &str = tw_join!(
        "bg-muted/50",
        "border-t",
        "font-medium",
        "[&>tr]:last:border-b-0"
    );
    pub const ROW: &str = tw_join!(
        "border-b",
        "transition-colors",
        "hover:bg-muted/50",
        "data-[state=selected]:bg-muted"
    );
    pub const HEAD: &str = tw_join!(
        "h-10",
        "px-2",
        "text-left",
        "align-middle",
        "font-medium",
        "text-muted-foreground",
        "[&:has([role=checkbox])]:pr-0",
        "[&>[role=checkbox]]:translate-y-[2px]"
    );
    pub const CELL: &str = tw_join!(
        "p-2",
        "align-middle",
        "[&:has([role=checkbox])]:pr-0",
        "[&>[role=checkbox]]:translate-y-[2px]"
    );
    pub const CAPTION: &str = tw_join!("mt-4", "text-sm", "text-muted-foreground");
}

/// Optional inputs accepted by [`TableRecipe::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableRecipeProps;

/// Static named classes for every Table part.
#[derive(Debug, Clone, Copy)]
pub struct TableRecipe {
    pub container: &'static str,
    pub root: &'static str,
    pub header: &'static str,
    pub body: &'static str,
    pub footer: &'static str,
    pub row: &'static str,
    pub head: &'static str,
    pub cell: &'static str,
    pub caption: &'static str,
}

impl TableRecipe {
    pub const fn resolve(&'static self, _props: TableRecipeProps) -> TableStyle {
        TableStyle { recipe: self }
    }
}

/// Fully resolved, streamable Table element style.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct TableStyle {
    recipe: &'static TableRecipe,
}

impl AttrValue for TableStyle {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.recipe.root.as_bytes())
    }
}

pub const TABLE_RECIPE: TableRecipe = TableRecipe {
    container: base::CONTAINER,
    root: base::ROOT,
    header: base::HEADER,
    body: base::BODY,
    footer: base::FOOTER,
    row: base::ROW,
    head: base::HEAD,
    cell: base::CELL,
    caption: base::CAPTION,
};

pub const fn table_recipe() -> &'static TableRecipe {
    &TABLE_RECIPE
}

impl crate::components::_proa::variant_spec::ComponentSpec for TableRecipe {
    const NAME: &'static str = "Table";

    fn base_classes(&self) -> &'static str {
        self.root
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        Vec::new()
    }

    fn anatomy(&self) -> Option<crate::components::_proa::Anatomy> {
        Some(super::TABLE_ANATOMY)
    }
}

/// Compatibility accessors for the former standalone helpers.
pub const fn table_classes() -> &'static str {
    TABLE_RECIPE.root
}

pub const fn table_header_classes() -> &'static str {
    TABLE_RECIPE.header
}

pub const fn table_body_classes() -> &'static str {
    TABLE_RECIPE.body
}

pub const fn table_footer_classes() -> &'static str {
    TABLE_RECIPE.footer
}

pub const fn table_row_classes() -> &'static str {
    TABLE_RECIPE.row
}

pub const fn table_head_classes() -> &'static str {
    TABLE_RECIPE.head
}

pub const fn table_cell_classes() -> &'static str {
    TABLE_RECIPE.cell
}

pub const fn table_caption_classes() -> &'static str {
    TABLE_RECIPE.caption
}
