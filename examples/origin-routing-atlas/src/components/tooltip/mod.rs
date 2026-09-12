// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tooltip component - displays informative text on hover/focus
//!
//! Unlike Popover, Tooltip is triggered by hover and keyboard focus,
//! not by click. It's meant for brief, non-interactive content.

use crate::components::_proa::Anatomy;

mod md;
mod recipe;
#[allow(clippy::module_inception)]
mod tooltip;

/// Static anatomy contract shared by SSR and the delegated runtime.
pub const TOOLTIP_ANATOMY: Anatomy = Anatomy::new("tooltip", &["root", "trigger", "content"]);

pub use recipe::{
    tooltip_recipe, TooltipAlign, TooltipRecipe, TooltipRecipeProps, TooltipSide, TooltipStyle,
    TOOLTIP_RECIPE,
};
pub use tooltip::{Tooltip, TooltipContent, TooltipDelay, TooltipTrigger};
