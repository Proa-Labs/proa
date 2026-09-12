// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tooltip recipe — semantic root placement and named slot classes.

use crate::components::_proa::theme::rounded;
use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Tooltip placement options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "side", path = "crate::components::_proa::variant_spec")]
pub enum TooltipSide {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl TooltipSide {
    pub const fn classes(self) -> &'static str {
        ""
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Tooltip alignment relative to its trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "align", path = "crate::components::_proa::variant_spec")]
pub enum TooltipAlign {
    #[default]
    Center,
    Start,
    End,
}

impl TooltipAlign {
    pub const fn classes(self) -> &'static str {
        ""
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TooltipRecipeProps {
    pub side: Option<TooltipSide>,
    pub align: Option<TooltipAlign>,
}

/// Static named classes for every Tooltip part.
#[derive(Debug, Clone, Copy)]
pub struct TooltipRecipe {
    pub root: &'static str,
    pub trigger: &'static str,
    pub content: &'static str,
    side_default: TooltipSide,
    align_default: TooltipAlign,
}

impl TooltipRecipe {
    pub const fn resolve(&'static self, props: TooltipRecipeProps) -> TooltipStyle {
        let side = match props.side {
            Some(side) => side,
            None => self.side_default,
        };
        let align = match props.align {
            Some(align) => align,
            None => self.align_default,
        };
        TooltipStyle {
            recipe: self,
            side,
            align,
        }
    }
}

#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct TooltipStyle {
    recipe: &'static TooltipRecipe,
    side: TooltipSide,
    align: TooltipAlign,
}

impl TooltipStyle {
    pub const fn side(self) -> TooltipSide {
        self.side
    }

    pub const fn align(self) -> TooltipAlign {
        self.align
    }
}

impl AttrValue for TooltipStyle {
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

const TOOLTIP_ROOT: &str = tw_join!("relative", "inline-block");

/// Trigger has no intrinsic visual treatment; its native button behavior is
/// preserved while the named slot remains available for additive classes.
const TOOLTIP_TRIGGER: &str = "";

/// `w-max` instead of `whitespace-nowrap`: the bubble is absolutely
/// positioned inside the trigger-sized root, so without an explicit sizing
/// basis it would shrink-wrap to the trigger's width. `max-content` keeps it
/// on one line when it fits, while the runtime caps `max-width` to the
/// viewport so long text wraps instead of overflowing the window.
pub const TOOLTIP_CONTENT_BASE: &str = tw_join!(
    "z-50",
    "overflow-hidden",
    "bg-gray-900",
    "text-gray-50",
    "px-3",
    "py-1.5",
    "text-xs",
    "shadow-md",
    "w-max",
    "break-words",
    rounded::MD
);

pub const TOOLTIP_RECIPE: TooltipRecipe = TooltipRecipe {
    root: TOOLTIP_ROOT,
    trigger: TOOLTIP_TRIGGER,
    content: TOOLTIP_CONTENT_BASE,
    side_default: TooltipSide::Top,
    align_default: TooltipAlign::Center,
};

pub const fn tooltip_recipe() -> &'static TooltipRecipe {
    &TOOLTIP_RECIPE
}

impl crate::components::_proa::variant_spec::ComponentSpec for TooltipRecipe {
    const NAME: &'static str = "Tooltip";

    fn base_classes(&self) -> &'static str {
        self.root
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![
            crate::components::_proa::variant_spec::variant_metadata::<TooltipSide>(),
            crate::components::_proa::variant_spec::variant_metadata::<TooltipAlign>(),
        ]
    }

    fn anatomy(&self) -> Option<crate::components::_proa::Anatomy> {
        Some(super::TOOLTIP_ANATOMY)
    }
}
