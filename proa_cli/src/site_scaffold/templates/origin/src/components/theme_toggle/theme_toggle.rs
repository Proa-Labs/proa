// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND (ISC AND MIT)

//! Theme Toggle component implementation
//!
//! SSR markup for the light/dark theme toggle button. The JavaScript runtime
//! handles the click: it flips the `dark` class on `<html>`, mirrors
//! `color-scheme`, and persists the choice under the `theme` localStorage key
//! (falling back to `prefers-color-scheme` when nothing is stored). The icon
//! swap is pure CSS via the root `.dark` class, so the button needs no
//! per-instance state.
//!
//! The moon and sun glyphs embed SVG geometry adapted from Lucide
//! (Feather-derived icons, ISC AND MIT).
use super::recipe::{ThemeToggleRecipeProps, THEME_TOGGLE_RECIPE};
use super::THEME_TOGGLE_ANATOMY;
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

/// ThemeToggle - a button that switches the document between light and dark.
///
/// # Example
///
/// ```ignore
/// use crate::components::ThemeToggle;
/// use proa_macros::html_sync;
///
/// html_sync! {
///     {ThemeToggle::new()}
/// }
/// ```
pub struct ThemeToggle {
    /// Accessible label; also used as the hover title.
    pub aria_label: &'static str,
    /// Additional classes fused after the recipe classes.
    pub class: Option<&'static str>,
}

impl ThemeToggle {
    pub const fn new() -> Self {
        Self {
            aria_label: "Toggle theme",
            class: None,
        }
    }
}

impl Default for ThemeToggle {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ThemeToggle {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let style = THEME_TOGGLE_RECIPE.resolve(ThemeToggleRecipeProps);
        html_sync! {
            <button
                type="button"
                data-theme-toggle-button
                data-scope={THEME_TOGGLE_ANATOMY.scope()}
                data-part="root"
                data-slot="theme-toggle"
                aria-label={self.aria_label}
                title={self.aria_label}
                class={proa_macros::text!("{} {}", style, self.class)}
            >
                <svg
                    data-scope={THEME_TOGGLE_ANATOMY.scope()}
                    data-part="moon-icon"
                    data-slot="theme-toggle-moon-icon"
                    class={THEME_TOGGLE_RECIPE.moon_icon}
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <path d="M20.99 12.79A9 9 0 1 1 11.21 3.01 7 7 0 0 0 20.99 12.79Z" />
                </svg>
                <svg
                    data-scope={THEME_TOGGLE_ANATOMY.scope()}
                    data-part="sun-icon"
                    data-slot="theme-toggle-sun-icon"
                    class={THEME_TOGGLE_RECIPE.sun_icon}
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <circle cx="12" cy="12" r="4" />
                    <path d="M12 2v2" />
                    <path d="M12 20v2" />
                    <path d="m4.93 4.93 1.41 1.41" />
                    <path d="m17.66 17.66 1.41 1.41" />
                    <path d="M2 12h2" />
                    <path d="M20 12h2" />
                    <path d="m6.34 17.66-1.41 1.41" />
                    <path d="m19.07 4.93-1.41 1.41" />
                </svg>
            </button>
        }
        .render(cx)
    }
}
