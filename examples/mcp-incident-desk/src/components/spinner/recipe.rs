// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT AND (ISC AND MIT)

//! Spinner recipe - type-safe styling system for Spinner component
//!
//! Inspired by Panda CSS and Ark UI patterns, this module provides a fully
//! typed recipe system for generating Tailwind classes with compile-time safety.

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Base styles module - core spinner styling
mod base {
    use crate::components::_proa::theme::{animate, shadcn_utils};
    use crate::tw_join;

    pub const SPINNER_BASE: &str = tw_join!(
        "lucide",
        "lucide-loader-circle",
        animate::SPIN,
        shadcn_utils::SHRINK_0
    );
}

/// Size styles module - spinner size variations
mod sizes {
    /// Spinner size variants
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
    #[variant(name = "size", path = "crate::components::_proa::variant_spec")]
    pub enum SpinnerSize {
        Sm, // size-3
        #[default]
        Default, // size-4
        Md, // size-5
        Lg, // size-6
        Xl, // size-8
    }

    impl SpinnerSize {
        /// Get the Tailwind classes for this size
        pub const fn classes(self) -> &'static str {
            match self {
                SpinnerSize::Sm => "size-3",
                SpinnerSize::Default => "size-4",
                SpinnerSize::Md => "size-5",
                SpinnerSize::Lg => "size-6",
                SpinnerSize::Xl => "size-8",
            }
        }

        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Sm => "sm",
                Self::Default => "default",
                Self::Md => "md",
                Self::Lg => "lg",
                Self::Xl => "xl",
            }
        }
    }
}

// Re-export all public types for convenience
pub use sizes::SpinnerSize;

/// Fully typed spinner recipe with compile-time safety
///
/// This recipe provides size control for the spinner icon.
///
/// This is a const struct that uses match-based lookups instead of hashmaps,
/// making it zero-cost at runtime with no initialization overhead.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpinnerRecipeProps {
    pub size: Option<SpinnerSize>,
}

#[derive(Debug, Clone, Copy)]
pub struct SpinnerRecipe {
    base: &'static str,
    size_default: SpinnerSize,
}

/// Global spinner recipe instance - fully const, zero runtime initialization!
pub const SPINNER_RECIPE: SpinnerRecipe = SpinnerRecipe {
    base: base::SPINNER_BASE,
    size_default: SpinnerSize::Default,
};

impl SpinnerRecipe {
    pub const fn resolve(&'static self, props: SpinnerRecipeProps) -> SpinnerStyle {
        let size = match props.size {
            Some(size) => size,
            None => self.size_default,
        };
        SpinnerStyle { recipe: self, size }
    }
}

#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct SpinnerStyle {
    recipe: &'static SpinnerRecipe,
    size: SpinnerSize,
}

impl SpinnerStyle {
    pub const fn size(self) -> SpinnerSize {
        self.size
    }
}

impl AttrValue for SpinnerStyle {
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
        cx.out.extend_static(self.size.classes().as_bytes())
    }
}

/// Get the global spinner recipe with default Tailwind styling
///
/// This is now just a convenience function that returns a reference to the const recipe.
/// No initialization overhead, no synchronization, just a simple reference!
pub const fn spinner_recipe() -> &'static SpinnerRecipe {
    &SPINNER_RECIPE
}

/// Compatibility resolver for callers of the former helper.
pub const fn spinner_classes(size: SpinnerSize) -> SpinnerStyle {
    SPINNER_RECIPE.resolve(SpinnerRecipeProps { size: Some(size) })
}

impl crate::components::_proa::variant_spec::ComponentSpec for SpinnerRecipe {
    const NAME: &'static str = "Spinner";

    fn base_classes(&self) -> &'static str {
        self.base
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![crate::components::_proa::variant_spec::variant_metadata::<
            SpinnerSize,
        >()]
    }
}

/// Lucide loader-circle SVG path
#[allow(dead_code)]
const LOADER_CIRCLE_PATH: &str = r#"<path d="M21 12a9 9 0 1 1-6.219-8.56" />"#;
