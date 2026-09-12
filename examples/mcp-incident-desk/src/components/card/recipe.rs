// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Card recipe — static named slots for a composable card surface.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

mod base {
    use super::*;

    pub const ROOT: &str = tw_join!(
        "rounded-xl",
        "border",
        "bg-card",
        "text-card-foreground",
        "shadow-sm"
    );
    pub const HEADER: &str = tw_join!("flex", "flex-col", "gap-1.5", "p-6");
    pub const TITLE: &str = tw_join!("leading-none", "font-semibold", "tracking-tight");
    pub const DESCRIPTION: &str = tw_join!("text-sm", "text-muted-foreground");
    pub const CONTENT: &str = tw_join!("p-6", "pt-0");
    pub const FOOTER: &str = tw_join!("flex", "items-center", "p-6", "pt-0");
}

/// Optional inputs accepted by [`CardRecipe::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CardRecipeProps;

/// Static named classes for every Card part.
#[derive(Debug, Clone, Copy)]
pub struct CardRecipe {
    pub root: &'static str,
    pub header: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub content: &'static str,
    pub footer: &'static str,
}

impl CardRecipe {
    pub const fn resolve(&'static self, _props: CardRecipeProps) -> CardStyle {
        CardStyle { recipe: self }
    }
}

/// Fully resolved, streamable Card root style.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct CardStyle {
    recipe: &'static CardRecipe,
}

impl AttrValue for CardStyle {
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

pub const CARD_RECIPE: CardRecipe = CardRecipe {
    root: base::ROOT,
    header: base::HEADER,
    title: base::TITLE,
    description: base::DESCRIPTION,
    content: base::CONTENT,
    footer: base::FOOTER,
};

pub const fn card_recipe() -> &'static CardRecipe {
    &CARD_RECIPE
}

impl crate::components::_proa::variant_spec::ComponentSpec for CardRecipe {
    const NAME: &'static str = "Card";

    fn base_classes(&self) -> &'static str {
        self.root
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        Vec::new()
    }

    fn anatomy(&self) -> Option<crate::components::_proa::Anatomy> {
        Some(super::CARD_ANATOMY)
    }
}

/// Compatibility accessors for the former standalone helpers.
pub const fn card_classes() -> &'static str {
    CARD_RECIPE.root
}

pub const fn card_header_classes() -> &'static str {
    CARD_RECIPE.header
}

pub const fn card_title_classes() -> &'static str {
    CARD_RECIPE.title
}

pub const fn card_description_classes() -> &'static str {
    CARD_RECIPE.description
}

pub const fn card_content_classes() -> &'static str {
    CARD_RECIPE.content
}

pub const fn card_footer_classes() -> &'static str {
    CARD_RECIPE.footer
}
