// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Separator recipe - type-safe styling system for Separator component
//!
//! Provides orientation-based styling for the Separator component.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Base styles for the separator
mod base {
    use super::*;

    pub const SEPARATOR_BASE: &str = tw_join!("shrink-0", "bg-border");
}

/// Orientation styles for the separator
mod orientations {
    use super::*;

    pub const SEPARATOR_HORIZONTAL: &str = tw_join!("h-px", "w-full");

    pub const SEPARATOR_VERTICAL: &str = tw_join!("h-full", "w-px");
}

/// Orientation options for the separator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "orientation", path = "crate::components::_proa::variant_spec")]
pub enum SeparatorOrientation {
    /// Horizontal separator (default)
    #[default]
    Horizontal,
    /// Vertical separator
    Vertical,
}

impl SeparatorOrientation {
    /// Get the Tailwind classes for this orientation
    pub const fn classes(self) -> &'static str {
        match self {
            SeparatorOrientation::Horizontal => orientations::SEPARATOR_HORIZONTAL,
            SeparatorOrientation::Vertical => orientations::SEPARATOR_VERTICAL,
        }
    }

    /// Get the data attribute value
    pub const fn as_str(self) -> &'static str {
        match self {
            SeparatorOrientation::Horizontal => "horizontal",
            SeparatorOrientation::Vertical => "vertical",
        }
    }
}

/// Optional inputs accepted by [`SeparatorRecipe::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeparatorRecipeProps {
    pub orientation: Option<SeparatorOrientation>,
}

/// Compile-time Separator recipe.
#[derive(Debug, Clone, Copy)]
pub struct SeparatorRecipe {
    root: &'static str,
    orientation_default: SeparatorOrientation,
}

impl SeparatorRecipe {
    pub const fn resolve(&'static self, props: SeparatorRecipeProps) -> SeparatorStyle {
        let orientation = match props.orientation {
            Some(orientation) => orientation,
            None => self.orientation_default,
        };

        SeparatorStyle {
            recipe: self,
            orientation,
        }
    }
}

/// Fully resolved, streamable Separator style.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct SeparatorStyle {
    recipe: &'static SeparatorRecipe,
    orientation: SeparatorOrientation,
}

impl SeparatorStyle {
    pub const fn orientation(self) -> SeparatorOrientation {
        self.orientation
    }
}

impl AttrValue for SeparatorStyle {
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
        cx.out.extend_static(self.orientation.classes().as_bytes())
    }
}

pub const SEPARATOR_RECIPE: SeparatorRecipe = SeparatorRecipe {
    root: base::SEPARATOR_BASE,
    orientation_default: SeparatorOrientation::Horizontal,
};

pub const fn separator_recipe() -> &'static SeparatorRecipe {
    &SEPARATOR_RECIPE
}

/// Compatibility resolver for callers of the former helper.
pub const fn separator_classes(orientation: SeparatorOrientation) -> SeparatorStyle {
    SEPARATOR_RECIPE.resolve(SeparatorRecipeProps {
        orientation: Some(orientation),
    })
}

impl crate::components::_proa::variant_spec::ComponentSpec for SeparatorRecipe {
    const NAME: &'static str = "Separator";

    fn base_classes(&self) -> &'static str {
        self.root
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![crate::components::_proa::variant_spec::variant_metadata::<
            SeparatorOrientation,
        >()]
    }
}
