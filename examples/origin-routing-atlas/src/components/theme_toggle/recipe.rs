// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Theme Toggle recipe — named slot classes for the theme toggle button.

use crate::components::_proa::theme::rounded;
use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThemeToggleRecipeProps;

/// Static named classes for every Theme Toggle part.
#[derive(Debug, Clone, Copy)]
pub struct ThemeToggleRecipe {
    pub root: &'static str,
    pub moon_icon: &'static str,
    pub sun_icon: &'static str,
}

impl ThemeToggleRecipe {
    pub const fn resolve(&'static self, _props: ThemeToggleRecipeProps) -> ThemeToggleStyle {
        ThemeToggleStyle { recipe: self }
    }
}

#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct ThemeToggleStyle {
    recipe: &'static ThemeToggleRecipe,
}

impl AttrValue for ThemeToggleStyle {
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

/// Ghost icon-button treatment matching shadcn's mode toggle.
const THEME_TOGGLE_ROOT: &str = tw_join!(
    "inline-flex",
    "h-9",
    "w-9",
    "shrink-0",
    "items-center",
    "justify-center",
    "text-muted-foreground",
    "transition-colors",
    "hover:bg-accent",
    "hover:text-accent-foreground",
    "focus-visible:outline-none",
    "focus-visible:ring-2",
    "focus-visible:ring-ring",
    "focus-visible:ring-offset-2",
    "disabled:pointer-events-none",
    "disabled:opacity-50",
    rounded::MD
);

/// The moon invites dark mode, so it shows while the page is light; the sun
/// takes over under the root `.dark` class. Pure CSS — the runtime only flips
/// the class, never the icons.
const THEME_TOGGLE_MOON_ICON: &str = tw_join!("dark:hidden");
const THEME_TOGGLE_SUN_ICON: &str = tw_join!("hidden", "dark:block");

pub const THEME_TOGGLE_RECIPE: ThemeToggleRecipe = ThemeToggleRecipe {
    root: THEME_TOGGLE_ROOT,
    moon_icon: THEME_TOGGLE_MOON_ICON,
    sun_icon: THEME_TOGGLE_SUN_ICON,
};

pub const fn theme_toggle_recipe() -> &'static ThemeToggleRecipe {
    &THEME_TOGGLE_RECIPE
}

impl crate::components::_proa::variant_spec::ComponentSpec for ThemeToggleRecipe {
    const NAME: &'static str = "ThemeToggle";

    fn base_classes(&self) -> &'static str {
        self.root
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        Vec::new()
    }

    fn anatomy(&self) -> Option<crate::components::_proa::Anatomy> {
        Some(super::THEME_TOGGLE_ANATOMY)
    }
}
