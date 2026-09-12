// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Textarea recipe - type-safe styling system for Textarea component
//!
//! Provides size variants and base styling for the Textarea component.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Base styles for the textarea - similar to Input but with textarea-specific adjustments
mod base {
    use super::*;

    pub const TEXTAREA_BASE: &str = tw_join!(
        // Layout
        "flex w-full",
        "min-h-16",
        "field-sizing-content",
        // Typography
        "text-base md:text-sm",
        // Colors
        "bg-transparent",
        "text-foreground",
        "placeholder:text-muted-foreground",
        // Border (shadcn input style)
        "border border-input",
        "rounded-md",
        // Shadow
        "shadow-xs",
        // Padding
        "px-3 py-2",
        // Focus visible (shadcn style)
        "focus-visible:outline-hidden",
        "focus-visible:border-ring",
        "focus-visible:ring-ring/50",
        "focus-visible:ring-[3px]",
        // Disabled
        "disabled:cursor-not-allowed",
        "disabled:opacity-50",
        // Selection
        "selection:bg-primary",
        "selection:text-primary-foreground",
        // Aria invalid (error state)
        "aria-invalid:border-destructive",
        "aria-invalid:ring-destructive/20",
        "dark:aria-invalid:ring-destructive/40"
    );
}

/// Size variants for the textarea
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "size", path = "crate::components::_proa::variant_spec")]
pub enum TextareaSize {
    Sm,
    #[default]
    Default,
    Lg,
}

impl TextareaSize {
    /// Get the Tailwind classes for this size
    pub const fn classes(self) -> &'static str {
        match self {
            TextareaSize::Sm => "min-h-16 px-2.5 py-1.5 text-xs",
            TextareaSize::Default => "min-h-20 px-3 py-2",
            TextareaSize::Lg => "min-h-32 px-4 py-3 text-base",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Default => "default",
            Self::Lg => "lg",
        }
    }
}

/// Get textarea base classes
pub const fn textarea_base_classes() -> &'static str {
    base::TEXTAREA_BASE
}

/// Optional inputs accepted by [`TextareaRecipe::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextareaRecipeProps {
    pub size: Option<TextareaSize>,
}

/// Compile-time Textarea recipe.
#[derive(Debug, Clone, Copy)]
pub struct TextareaRecipe {
    root: &'static str,
    size_default: TextareaSize,
}

impl TextareaRecipe {
    pub const fn resolve(&'static self, props: TextareaRecipeProps) -> TextareaStyle {
        let size = match props.size {
            Some(size) => size,
            None => self.size_default,
        };

        TextareaStyle { recipe: self, size }
    }
}

/// Fully resolved, streamable Textarea style.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct TextareaStyle {
    recipe: &'static TextareaRecipe,
    size: TextareaSize,
}

impl TextareaStyle {
    pub const fn size(self) -> TextareaSize {
        self.size
    }
}

impl AttrValue for TextareaStyle {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.recipe.root.as_bytes())?;
        cx.out.extend_static(b" ")?;
        cx.out.extend_static(self.size.classes().as_bytes())
    }
}

pub const TEXTAREA_RECIPE: TextareaRecipe = TextareaRecipe {
    root: base::TEXTAREA_BASE,
    size_default: TextareaSize::Default,
};

pub const fn textarea_recipe() -> &'static TextareaRecipe {
    &TEXTAREA_RECIPE
}

/// Compatibility resolver for callers of the former helper.
pub const fn textarea_classes(size: TextareaSize) -> TextareaStyle {
    TEXTAREA_RECIPE.resolve(TextareaRecipeProps { size: Some(size) })
}

impl crate::components::_proa::variant_spec::ComponentSpec for TextareaRecipe {
    const NAME: &'static str = "Textarea";

    fn base_classes(&self) -> &'static str {
        self.root
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![crate::components::_proa::variant_spec::variant_metadata::<
            TextareaSize,
        >()]
    }
}
