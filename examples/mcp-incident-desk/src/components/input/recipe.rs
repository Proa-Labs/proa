// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Input recipe - type-safe styling system for Input component
//!
//! Provides size variants and base styling for the Input component.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Base styles for the input
mod base {
    use super::*;

    pub const INPUT_BASE: &str = tw_join!(
        // Layout
        "flex w-full",
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
        // Focus visible (shadcn style)
        "focus-visible:outline-hidden",
        "focus-visible:border-ring",
        "focus-visible:ring-ring/50",
        "focus-visible:ring-[3px]",
        // Disabled
        "disabled:cursor-not-allowed",
        "disabled:opacity-50",
        // File input styling
        "file:border-0",
        "file:bg-transparent",
        "file:text-sm",
        "file:font-medium",
        "file:text-foreground",
        // Selection
        "selection:bg-primary",
        "selection:text-primary-foreground",
        // Aria invalid (error state)
        "aria-invalid:border-destructive",
        "aria-invalid:ring-destructive/20",
        "dark:aria-invalid:ring-destructive/40"
    );
}

/// Size variants for the input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "size", path = "crate::components::_proa::variant_spec")]
pub enum InputSize {
    Sm,
    #[default]
    Default,
    Lg,
}

impl InputSize {
    /// Get the Tailwind classes for this size
    pub const fn classes(self) -> &'static str {
        match self {
            InputSize::Sm => "h-8 px-2.5 py-1.5 text-xs",
            InputSize::Default => "h-9 px-3 py-1",
            InputSize::Lg => "h-10 px-4 py-2 text-base",
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

/// Input type variants (HTML input types)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InputType {
    #[default]
    Text,
    Email,
    Password,
    Number,
    Tel,
    Url,
    Search,
    Date,
    Time,
    DatetimeLocal,
    File,
    Hidden,
}

impl InputType {
    /// Get the HTML type attribute value
    pub const fn as_str(self) -> &'static str {
        match self {
            InputType::Text => "text",
            InputType::Email => "email",
            InputType::Password => "password",
            InputType::Number => "number",
            InputType::Tel => "tel",
            InputType::Url => "url",
            InputType::Search => "search",
            InputType::Date => "date",
            InputType::Time => "time",
            InputType::DatetimeLocal => "datetime-local",
            InputType::File => "file",
            InputType::Hidden => "hidden",
        }
    }
}

/// Optional inputs accepted by [`InputRecipe::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputRecipeProps {
    pub size: Option<InputSize>,
}

/// Fully typed input recipe with compile-time defaults.
#[derive(Debug, Clone, Copy)]
pub struct InputRecipe {
    base: &'static str,
    /// Wrapper used by the rsjs-controlled input surface.
    pub controlled_root: &'static str,
    /// Label text used by the rsjs-controlled input surface.
    pub controlled_label: &'static str,
    size_default: InputSize,
}

pub const INPUT_CONTROLLED_ROOT: &str = "grid gap-1.5";
pub const INPUT_CONTROLLED_LABEL: &str = "text-sm font-medium";

/// Global input recipe instance
pub const INPUT_RECIPE: InputRecipe = InputRecipe {
    base: base::INPUT_BASE,
    controlled_root: INPUT_CONTROLLED_ROOT,
    controlled_label: INPUT_CONTROLLED_LABEL,
    size_default: InputSize::Default,
};

impl InputRecipe {
    pub const fn resolve(&'static self, props: InputRecipeProps) -> InputStyle {
        let size = match props.size {
            Some(size) => size,
            None => self.size_default,
        };
        InputStyle { recipe: self, size }
    }
}

/// Fully resolved, streamable Input style.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct InputStyle {
    recipe: &'static InputRecipe,
    size: InputSize,
}

impl InputStyle {
    pub const fn size(self) -> InputSize {
        self.size
    }
}

impl AttrValue for InputStyle {
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

/// Get the global input recipe
pub const fn input_recipe() -> &'static InputRecipe {
    &INPUT_RECIPE
}

/// Compatibility resolver for callers of the former helper.
pub const fn input_classes(size: InputSize) -> InputStyle {
    INPUT_RECIPE.resolve(InputRecipeProps { size: Some(size) })
}

impl crate::components::_proa::variant_spec::ComponentSpec for InputRecipe {
    const NAME: &'static str = "Input";

    fn base_classes(&self) -> &'static str {
        self.base
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![crate::components::_proa::variant_spec::variant_metadata::<
            InputSize,
        >()]
    }
}
