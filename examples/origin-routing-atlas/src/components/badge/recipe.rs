// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Badge recipe - type-safe styling system for Badge component
//!
//! Provides variant styles and base styling for the Badge component.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Base styles for the badge
mod base {
    use super::*;

    pub const BADGE_BASE: &str = tw_join!(
        // Layout
        "inline-flex",
        "items-center",
        // Sizing
        "rounded-md",
        "px-2.5",
        "py-0.5",
        // Typography
        "text-xs",
        "font-medium",
        // Transitions
        "transition-colors",
        // Focus
        "focus:outline-hidden",
        "focus:ring-2",
        "focus:ring-ring",
        "focus:ring-offset-2"
    );
}

/// Variant styles for the badge
mod variants {
    use super::*;

    pub const BADGE_DEFAULT: &str = tw_join!(
        "bg-primary",
        "text-primary-foreground",
        "shadow",
        "hover:bg-primary/80"
    );

    pub const BADGE_SECONDARY: &str = tw_join!(
        "bg-secondary",
        "text-secondary-foreground",
        "hover:bg-secondary/80"
    );

    pub const BADGE_DESTRUCTIVE: &str = tw_join!(
        "bg-destructive",
        "text-destructive-foreground",
        "shadow",
        "hover:bg-destructive/80"
    );

    pub const BADGE_OUTLINE: &str =
        tw_join!("border", "border-input", "text-foreground", "bg-background");

    pub const BADGE_SUCCESS: &str = tw_join!(
        "bg-green-600",
        "text-white",
        "shadow",
        "hover:bg-green-600/80"
    );

    pub const BADGE_WARNING: &str = tw_join!(
        "bg-yellow-500",
        "text-white",
        "shadow",
        "hover:bg-yellow-500/80"
    );
}

/// Variant options for the badge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "variant", path = "crate::components::_proa::variant_spec")]
pub enum BadgeVariant {
    /// Primary badge style (default)
    #[default]
    Default,
    /// Secondary/neutral style
    Secondary,
    /// Destructive/error style
    Destructive,
    /// Outline/bordered style
    Outline,
    /// Success/positive style
    Success,
    /// Warning style
    Warning,
}

impl BadgeVariant {
    /// Get the Tailwind classes for this variant
    pub const fn classes(self) -> &'static str {
        match self {
            BadgeVariant::Default => variants::BADGE_DEFAULT,
            BadgeVariant::Secondary => variants::BADGE_SECONDARY,
            BadgeVariant::Destructive => variants::BADGE_DESTRUCTIVE,
            BadgeVariant::Outline => variants::BADGE_OUTLINE,
            BadgeVariant::Success => variants::BADGE_SUCCESS,
            BadgeVariant::Warning => variants::BADGE_WARNING,
        }
    }

    /// Get the data attribute value
    pub const fn as_str(self) -> &'static str {
        match self {
            BadgeVariant::Default => "default",
            BadgeVariant::Secondary => "secondary",
            BadgeVariant::Destructive => "destructive",
            BadgeVariant::Outline => "outline",
            BadgeVariant::Success => "success",
            BadgeVariant::Warning => "warning",
        }
    }
}

/// Optional inputs accepted by [`BadgeRecipe::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BadgeRecipeProps {
    pub variant: Option<BadgeVariant>,
}

/// Fully typed badge recipe with compile-time defaults.
#[derive(Debug, Clone, Copy)]
pub struct BadgeRecipe {
    base: &'static str,
    variant_default: BadgeVariant,
}

/// Global badge recipe instance
pub const BADGE_RECIPE: BadgeRecipe = BadgeRecipe {
    base: base::BADGE_BASE,
    variant_default: BadgeVariant::Default,
};

impl BadgeRecipe {
    pub const fn resolve(&'static self, props: BadgeRecipeProps) -> BadgeStyle {
        let variant = match props.variant {
            Some(variant) => variant,
            None => self.variant_default,
        };

        BadgeStyle {
            recipe: self,
            variant,
        }
    }
}

/// Fully resolved, zero-allocation badge style.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct BadgeStyle {
    recipe: &'static BadgeRecipe,
    variant: BadgeVariant,
}

impl BadgeStyle {
    pub const fn variant(self) -> BadgeVariant {
        self.variant
    }
}

impl AttrValue for BadgeStyle {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.recipe.base.as_bytes())?;
        cx.out.extend_static(b" ")?;
        cx.out.extend_static(self.variant.classes().as_bytes())
    }
}

/// Get the global badge recipe
pub const fn badge_recipe() -> &'static BadgeRecipe {
    &BADGE_RECIPE
}

/// Compatibility resolver for callers of the former helper.
pub const fn badge_classes(variant: BadgeVariant) -> BadgeStyle {
    BADGE_RECIPE.resolve(BadgeRecipeProps {
        variant: Some(variant),
    })
}

impl crate::components::_proa::variant_spec::ComponentSpec for BadgeRecipe {
    const NAME: &'static str = "Badge";

    fn base_classes(&self) -> &'static str {
        self.base
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![crate::components::_proa::variant_spec::variant_metadata::<
            BadgeVariant,
        >()]
    }
}
