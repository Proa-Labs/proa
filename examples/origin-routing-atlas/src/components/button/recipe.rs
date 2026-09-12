// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Button recipe - type-safe styling system for Button component
//!
//! Inspired by Panda CSS and Ark UI patterns, this module provides a fully
//! typed recipe system for generating Tailwind classes with compile-time safety.
//!
//! # Architecture
//!
//! This module defines:
//! 1. **Variant enums** (ButtonSize, ButtonColor, ButtonVariant, ButtonRadius)
//! 2. **ButtonRecipeProps** - Optional author inputs
//! 3. **ButtonRecipe** - Defaults plus canonical compound resolution
//! 4. **ButtonStyle** - A resolved, streamable attribute value
//!
//! # Theme Integration
//!
//! This recipe composes primitive design tokens from `theme.rs` into component-specific styles.
//! Following Panda CSS principles:
//! - **Theme contains primitives only**: `Colors::BLUE_600`, `Spacing::SPACING_4`, etc.
//! - **Theme provides utility modules**: `theme::bg`, `theme::text`, `theme::hover_bg`, etc.
//! - **Recipes compose utilities**: Uses `const_format::concatcp!()` to compose at compile-time
//!
//! ## Using tw_join! for Composition
//!
//! We use the `tw_join!()` macro to compose theme constants with automatic spacing:
//!
//! ```ignore
//! use crate::components::_proa::theme::{bg, text, hover_bg, focus_ring};
//! use crate::tw_join;
//!
//! const BUTTON_PRIMARY: &str = tw_join!(
//!     bg::BLUE_600,
//!     text::WHITE,
//!     hover_bg::BLUE_700,
//!     focus_ring::BLUE_500
//! );
//! ```
//!
//! The `tw_join!` macro wraps `const_format::concatcp!()` and automatically adds spaces
//! between classes. Changing `Colors::BLUE_600` in theme.rs automatically propagates
//! through `bg::BLUE_600` to `BUTTON_PRIMARY`.
//!
//! # Usage Pattern
//!
//! Resolve the global recipe once, then render the returned style directly as
//! an attribute value.
//!
//! ```ignore
//! let style = BUTTON_RECIPE.resolve(ButtonRecipeProps {
//!     size: Some(ButtonSize::Lg),
//!     color: Some(ButtonColor::Primary),
//!     ..ButtonRecipeProps::default()
//! });
//! ```

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Base styles module - core button styling (shadcn style)
mod base {
    use crate::components::_proa::theme::{
        aria_invalid, focus_visible, font_size, font_weight, shadcn_utils, state, transition,
    };
    use crate::tw_join;

    pub const BUTTON_BASE: &str = tw_join!(
        shadcn_utils::INLINE_FLEX,
        shadcn_utils::ITEMS_CENTER,
        shadcn_utils::JUSTIFY_CENTER,
        shadcn_utils::GAP_2,
        shadcn_utils::WHITESPACE_NOWRAP,
        font_size::TEXT_SM,
        font_weight::FONT_MEDIUM,
        transition::TRANSITION_ALL,
        state::CURSOR_POINTER,
        state::DISABLED_POINTER_EVENTS_NONE,
        state::DISABLED_OPACITY_50,
        shadcn_utils::SVG_POINTER_EVENTS_NONE,
        shadcn_utils::SVG_SIZE_4,
        shadcn_utils::SHRINK_0,
        shadcn_utils::SVG_SHRINK_0,
        focus_visible::OUTLINE_NONE,
        focus_visible::BORDER_RING,
        focus_visible::RING_RING,
        focus_visible::RING_3,
        aria_invalid::RING_DESTRUCTIVE,
        aria_invalid::RING_DESTRUCTIVE_DARK,
        aria_invalid::BORDER_DESTRUCTIVE
    );
}

/// Size styles module - shadcn height-based sizing with gap
mod sizes {
    use crate::components::_proa::theme::{height, padding, shadcn_utils};
    use crate::tw_join;

    // shadcn button sizes: md (h-9), sm (h-8), lg (h-10), icon (size-9)
    const BUTTON_SIZE_MD: &str =
        tw_join!(height::H_9, padding::PX_4, padding::PY_2, "has-[>svg]:px-3");
    const BUTTON_SIZE_SM: &str = tw_join!(
        height::H_8,
        shadcn_utils::GAP_1_5,
        padding::PX_3,
        "has-[>svg]:px-2.5"
    );
    const BUTTON_SIZE_LG: &str = tw_join!(height::H_10, padding::PX_6, "has-[>svg]:px-4");
    // Icon sizes - square buttons for icon-only use
    const BUTTON_SIZE_ICON: &str = "size-9";
    const BUTTON_SIZE_ICON_SM: &str = "size-8";
    const BUTTON_SIZE_ICON_LG: &str = "size-10";

    /// Button size variants (shadcn style)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
    #[variant(name = "size", path = "crate::components::_proa::variant_spec")]
    pub enum ButtonSize {
        Sm,
        #[default]
        Md,
        Lg,
        /// Square icon button (default size, 36px)
        Icon,
        /// Small square icon button (32px)
        IconSm,
        /// Large square icon button (40px)
        IconLg,
    }

    impl ButtonSize {
        /// Get the Tailwind classes for this size
        pub const fn classes(self) -> &'static str {
            match self {
                ButtonSize::Sm => BUTTON_SIZE_SM,
                ButtonSize::Md => BUTTON_SIZE_MD,
                ButtonSize::Lg => BUTTON_SIZE_LG,
                ButtonSize::Icon => BUTTON_SIZE_ICON,
                ButtonSize::IconSm => BUTTON_SIZE_ICON_SM,
                ButtonSize::IconLg => BUTTON_SIZE_ICON_LG,
            }
        }

        /// Get the stable value used by `data-size` and generated APIs.
        pub const fn as_str(self) -> &'static str {
            match self {
                ButtonSize::Sm => "sm",
                ButtonSize::Md => "md",
                ButtonSize::Lg => "lg",
                ButtonSize::Icon => "icon",
                ButtonSize::IconSm => "icon-sm",
                ButtonSize::IconLg => "icon-lg",
            }
        }
    }
}

/// Color styles module - semantic button colors
/// Colors provide different classes based on the variant (solid, outline, ghost)
mod colors {
    use crate::components::_proa::theme::{bg, hover_bg, text};
    use crate::tw_join;

    // ===== SOLID VARIANT COLORS =====
    const SOLID_PRIMARY: &str =
        tw_join!(bg::PRIMARY, text::PRIMARY_FOREGROUND, hover_bg::PRIMARY_90);

    const SOLID_SECONDARY: &str = tw_join!(
        bg::SECONDARY,
        text::SECONDARY_FOREGROUND,
        hover_bg::SECONDARY_80
    );

    const SOLID_DESTRUCTIVE: &str = tw_join!(
        bg::DESTRUCTIVE,
        text::WHITE,
        "shadow-xs",
        hover_bg::DESTRUCTIVE_80,
        "focus-visible:ring-destructive/20",
        "dark:focus-visible:ring-destructive/40"
    );

    // ===== OUTLINE VARIANT COLORS =====
    // Outline: transparent bg, colored border + text, light colored hover
    const OUTLINE_PRIMARY: &str = tw_join!(
        "border",
        "border-primary",
        "bg-background",
        "text-primary",
        "shadow-xs",
        "hover:bg-primary/10",
        "dark:bg-transparent",
        "dark:hover:bg-primary/20"
    );

    const OUTLINE_SECONDARY: &str = tw_join!(
        "border",
        "border-input",
        "bg-background",
        "text-foreground",
        "shadow-xs",
        "hover:bg-accent",
        "hover:text-accent-foreground",
        "dark:bg-input/30",
        "dark:border-input",
        "dark:hover:bg-input/50"
    );

    const OUTLINE_DESTRUCTIVE: &str = tw_join!(
        "border",
        "border-destructive",
        "bg-background",
        "text-destructive",
        "shadow-xs",
        "hover:bg-destructive/10",
        "dark:bg-transparent",
        "dark:hover:bg-destructive/20"
    );

    // ===== GHOST VARIANT COLORS =====
    // Ghost: no border, no bg, colored text, light colored hover
    const GHOST_PRIMARY: &str = tw_join!(
        "text-primary",
        "hover:bg-primary/10",
        "hover:text-primary",
        "dark:hover:bg-primary/20"
    );

    const GHOST_SECONDARY: &str = tw_join!(
        "text-foreground",
        "hover:bg-accent",
        "hover:text-accent-foreground",
        "dark:hover:bg-accent/50"
    );

    const GHOST_DESTRUCTIVE: &str = tw_join!(
        "text-destructive",
        "hover:bg-destructive/10",
        "hover:text-destructive",
        "dark:hover:bg-destructive/20"
    );

    // ===== LINK VARIANT COLORS =====
    // Link: looks like a hyperlink - no bg, underline on hover
    const LINK_PRIMARY: &str = tw_join!("text-primary", "underline-offset-4", "hover:underline");

    const LINK_SECONDARY: &str =
        tw_join!("text-foreground", "underline-offset-4", "hover:underline");

    const LINK_DESTRUCTIVE: &str =
        tw_join!("text-destructive", "underline-offset-4", "hover:underline");

    /// Button color variants - semantic color choices
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
    #[variant(name = "color", path = "crate::components::_proa::variant_spec")]
    pub enum ButtonColor {
        #[default]
        Primary,
        Secondary,
        Destructive,
    }

    impl ButtonColor {
        /// Get the Tailwind classes for this color with solid variant
        pub const fn solid_classes(self) -> &'static str {
            match self {
                ButtonColor::Primary => SOLID_PRIMARY,
                ButtonColor::Secondary => SOLID_SECONDARY,
                ButtonColor::Destructive => SOLID_DESTRUCTIVE,
            }
        }

        /// Get the Tailwind classes for this color with outline variant
        pub const fn outline_classes(self) -> &'static str {
            match self {
                ButtonColor::Primary => OUTLINE_PRIMARY,
                ButtonColor::Secondary => OUTLINE_SECONDARY,
                ButtonColor::Destructive => OUTLINE_DESTRUCTIVE,
            }
        }

        /// Get the Tailwind classes for this color with ghost variant
        pub const fn ghost_classes(self) -> &'static str {
            match self {
                ButtonColor::Primary => GHOST_PRIMARY,
                ButtonColor::Secondary => GHOST_SECONDARY,
                ButtonColor::Destructive => GHOST_DESTRUCTIVE,
            }
        }

        /// Get the Tailwind classes for this color with link variant
        pub const fn link_classes(self) -> &'static str {
            match self {
                ButtonColor::Primary => LINK_PRIMARY,
                ButtonColor::Secondary => LINK_SECONDARY,
                ButtonColor::Destructive => LINK_DESTRUCTIVE,
            }
        }

        /// Resolve the compound `color x variant` appearance in one place.
        pub const fn classes_for(self, variant: super::ButtonVariant) -> &'static str {
            match variant {
                super::ButtonVariant::Solid => self.solid_classes(),
                super::ButtonVariant::Outline => self.outline_classes(),
                super::ButtonVariant::Ghost => self.ghost_classes(),
                super::ButtonVariant::Link => self.link_classes(),
            }
        }

        /// Get the stable value used by `data-color` and generated APIs.
        pub const fn as_str(self) -> &'static str {
            match self {
                ButtonColor::Primary => "primary",
                ButtonColor::Secondary => "secondary",
                ButtonColor::Destructive => "destructive",
            }
        }

        /// Legacy standalone color lookup.
        ///
        /// Compound-aware consumers should use [`Self::classes_for`]. Component
        /// metadata deliberately omits these classes from the independent color
        /// axis and exports the complete `color x variant` matrix instead.
        pub const fn classes(self) -> &'static str {
            self.solid_classes()
        }
    }
}

/// Variant styles module - visual styling (solid, outline, ghost, link)
mod variants {
    /// Button variant - visual style (independent of color)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
    #[variant(name = "variant", path = "crate::components::_proa::variant_spec")]
    pub enum ButtonVariant {
        #[default]
        Solid,
        Outline,
        Ghost,
        /// Link variant - styled like a hyperlink with underline on hover
        Link,
    }

    impl ButtonVariant {
        /// Independent-axis metadata projection.
        ///
        /// Appearance classes are resolved by `ButtonColor::classes_for` because
        /// they depend on both the color and variant.
        pub const fn classes(self) -> &'static str {
            ""
        }

        pub const fn as_str(self) -> &'static str {
            match self {
                ButtonVariant::Solid => "solid",
                ButtonVariant::Outline => "outline",
                ButtonVariant::Ghost => "ghost",
                ButtonVariant::Link => "link",
            }
        }
    }
}

/// Radius styles module - border radius variations
mod radius {
    use crate::components::_proa::theme::rounded;

    /// Button radius variants
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
    #[variant(name = "radius", path = "crate::components::_proa::variant_spec")]
    pub enum ButtonRadius {
        None,
        Sm,
        #[default]
        Md,
        Lg,
        Full,
    }

    impl ButtonRadius {
        /// Get the Tailwind classes for this radius
        pub const fn classes(self) -> &'static str {
            match self {
                ButtonRadius::None => rounded::NONE,
                ButtonRadius::Sm => rounded::SM,
                ButtonRadius::Md => rounded::MD,
                ButtonRadius::Lg => rounded::LG,
                ButtonRadius::Full => rounded::FULL,
            }
        }

        /// Get the stable value used by `data-radius` and generated APIs.
        pub const fn as_str(self) -> &'static str {
            match self {
                ButtonRadius::None => "none",
                ButtonRadius::Sm => "sm",
                ButtonRadius::Md => "md",
                ButtonRadius::Lg => "lg",
                ButtonRadius::Full => "full",
            }
        }
    }
}

// Re-export all public types for convenience
pub use colors::ButtonColor;
pub use radius::ButtonRadius;
pub use sizes::ButtonSize;
pub use variants::ButtonVariant;

/// Inputs accepted by [`ButtonRecipe::resolve`].
///
/// Optional fields preserve the ergonomic component API while the resolved
/// [`ButtonStyle`] always contains concrete effective values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ButtonRecipeProps {
    pub size: Option<ButtonSize>,
    pub color: Option<ButtonColor>,
    pub variant: Option<ButtonVariant>,
    pub radius: Option<ButtonRadius>,
}

/// Fully resolved, zero-allocation Button style.
///
/// This value is `Copy` and implements [`AttrValue`], so it streams its static
/// class fragments directly into the response buffer when used in an HTML
/// attribute. It is the sole runtime class-resolution path for Button.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonStyle {
    base: &'static str,
    size: ButtonSize,
    color: ButtonColor,
    variant: ButtonVariant,
    radius: ButtonRadius,
    appearance: &'static str,
}

impl ButtonStyle {
    pub const fn size(self) -> ButtonSize {
        self.size
    }

    pub const fn color(self) -> ButtonColor {
        self.color
    }

    pub const fn variant(self) -> ButtonVariant {
        self.variant
    }

    pub const fn radius(self) -> ButtonRadius {
        self.radius
    }

    /// Compatibility representation for callers of the pre-resolver recipe API.
    const fn class_segments(self) -> ([&'static str; ButtonRecipe::CLASS_CAPACITY], usize) {
        (
            [
                self.base,
                self.size.classes(),
                self.radius.classes(),
                self.appearance,
                "",
            ],
            ButtonRecipe::RESOLVED_CLASS_COUNT,
        )
    }
}

impl AttrValue for ButtonStyle {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.base.as_bytes())?;
        cx.out.extend_static(b" ")?;
        cx.out.extend_static(self.size.classes().as_bytes())?;
        cx.out.extend_static(b" ")?;
        cx.out.extend_static(self.radius.classes().as_bytes())?;
        cx.out.extend_static(b" ")?;
        cx.out.extend_static(self.appearance.as_bytes())
    }
}

/// Fully typed button recipe with compile-time defaults.
///
/// This recipe separates color and variant as independent dimensions:
/// - **Color**: Primary, Secondary, Destructive (semantic meaning)
/// - **Variant**: Solid, Outline, Ghost (visual style)
/// - **Size**: Sm, Default, Lg, Icon (button dimensions)
/// - **Radius**: None, Sm, Md, Lg, Full (border radius)
///
/// This is a const struct that uses match-based lookups instead of hashmaps,
/// making it zero-cost at runtime with no initialization overhead.
///
/// This implementation is handwritten because the current `ProaRecipe` derive
/// only combines independent variant axes and cannot model `color x variant`
/// compound classes. The legacy `classes()` API remains available, but now
/// delegates to the same canonical resolver used by the component renderer.
pub struct ButtonRecipe {
    base: &'static str,
    size_default: Option<ButtonSize>,
    color_default: Option<ButtonColor>,
    variant_default: Option<ButtonVariant>,
    radius_default: Option<ButtonRadius>,
}

/// Global button recipe instance - fully const, zero runtime initialization!
pub const BUTTON_RECIPE: ButtonRecipe = ButtonRecipe {
    base: base::BUTTON_BASE,
    size_default: Some(ButtonSize::Md),
    color_default: Some(ButtonColor::Primary),
    variant_default: Some(ButtonVariant::Solid),
    radius_default: Some(ButtonRadius::Md),
};

impl ButtonRecipe {
    /// Capacity of the legacy array representation. This remains five for
    /// source compatibility with the former derived recipe; compound
    /// resolution now needs only four populated fragments.
    pub const CLASS_CAPACITY: usize = 5;
    const RESOLVED_CLASS_COUNT: usize = 4;

    /// Resolve optional author inputs into an effective, streamable style.
    pub const fn resolve(&self, props: ButtonRecipeProps) -> ButtonStyle {
        let size = match props.size {
            Some(value) => value,
            None => match self.size_default {
                Some(value) => value,
                None => ButtonSize::Md,
            },
        };
        let color = match props.color {
            Some(value) => value,
            None => match self.color_default {
                Some(value) => value,
                None => ButtonColor::Primary,
            },
        };
        let variant = match props.variant {
            Some(value) => value,
            None => match self.variant_default {
                Some(value) => value,
                None => ButtonVariant::Solid,
            },
        };
        let radius = match props.radius {
            Some(value) => value,
            None => match self.radius_default {
                Some(value) => value,
                None => ButtonRadius::Md,
            },
        };

        ButtonStyle {
            base: self.base,
            size,
            color,
            variant,
            radius,
            appearance: color.classes_for(variant),
        }
    }

    /// Compatibility API for existing callers that consume recipe segments.
    ///
    /// Unlike the old derived implementation, this includes the canonical
    /// compound appearance selected by [`Self::resolve`]. New render code should
    /// use `resolve()` directly to avoid the intermediate array.
    pub const fn classes(
        &self,
        size: Option<ButtonSize>,
        color: Option<ButtonColor>,
        variant: Option<ButtonVariant>,
        radius: Option<ButtonRadius>,
    ) -> ([&'static str; Self::CLASS_CAPACITY], usize) {
        self.resolve(ButtonRecipeProps {
            size,
            color,
            variant,
            radius,
        })
        .class_segments()
    }
}

impl crate::components::_proa::variant_spec::ComponentSpec for ButtonRecipe {
    const NAME: &'static str = "Button";

    fn base_classes(&self) -> &'static str {
        self.base
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        use crate::components::_proa::variant_spec::{compound_axis_metadata, variant_metadata};

        vec![
            variant_metadata::<ButtonSize>(),
            compound_axis_metadata::<ButtonColor>(),
            compound_axis_metadata::<ButtonVariant>(),
            variant_metadata::<ButtonRadius>(),
        ]
    }

    fn compound_variant_metadata(
        &self,
    ) -> Vec<crate::components::_proa::variant_spec::CompoundVariantMetadata> {
        use crate::components::_proa::variant_spec::{
            CompoundVariantConditionMetadata, CompoundVariantMetadata,
        };

        let colors = [
            ButtonColor::Primary,
            ButtonColor::Secondary,
            ButtonColor::Destructive,
        ];
        let variants = [
            ButtonVariant::Solid,
            ButtonVariant::Outline,
            ButtonVariant::Ghost,
            ButtonVariant::Link,
        ];
        let mut compounds = Vec::with_capacity(colors.len() * variants.len());

        for color in colors {
            for variant in variants {
                compounds.push(CompoundVariantMetadata::new(
                    vec![
                        CompoundVariantConditionMetadata::new("color", color.as_str()),
                        CompoundVariantConditionMetadata::new("variant", variant.as_str()),
                    ],
                    color.classes_for(variant),
                ));
            }
        }

        compounds
    }

    fn anatomy(&self) -> Option<crate::components::_proa::Anatomy> {
        Some(super::BUTTON_ANATOMY)
    }
}

/// Get the global button recipe with default Tailwind styling
///
/// This is now just a convenience function that returns a reference to the const recipe.
/// No initialization overhead, no synchronization, just a simple reference!
pub const fn button_recipe() -> &'static ButtonRecipe {
    &BUTTON_RECIPE
}
