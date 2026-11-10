// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{ClassList, DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

const THEME_TOGGLE_CLASS: &str = "theme-toggle-button";

#[derive(Debug, Clone, Copy, Default)]
pub struct ThemeToggle {
    pub class: Option<&'static str>,
}

impl ThemeToggle {
    pub const fn new() -> Self {
        Self { class: None }
    }
}

/// The theme switch.
///
/// The behavior used to be a delegated `document` click listener shipped as a
/// raw `<script>` next to this markup. It is an rsjs island now: the handler
/// binds to this button, so N toggles on a page cost N handlers instead of N
/// duplicate document listeners.
///
/// The `dark` class has to land on `<html>`, which no island renders, so this
/// goes through `rsjs::root`. `color-scheme` is not set here at all: CSS keys
/// it off the same class, which keeps the two from drifting.
///
/// Note this reads the live root class rather than tracking a signal. The
/// server cannot know the theme (it is resolved from `localStorage` or
/// `prefers-color-scheme` before first paint), so a hydrated signal would
/// start wrong on every page that loads dark.
#[rsjs(component, client)]
impl<L, B> WebRenderSync<L, B> for ThemeToggle
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let classes = ClassList::<2>::new()
            .add(THEME_TOGGLE_CLASS)
            .add_opt(self.class);

        html_sync! {
            <button
                type="button"
                class={classes}
                data-component="theme-toggle"
                data-theme-toggle=""
                aria-label="Toggle theme"
                on_click={|_| {
                    if rsjs::root::has_class("dark") {
                        rsjs::root::set_class("dark", false);
                        rsjs::local_storage::set_item("theme", "light");
                    } else {
                        rsjs::root::set_class("dark", true);
                        rsjs::local_storage::set_item("theme", "dark");
                    }
                }}
            >
                <svg
                    class="theme-toggle-icon theme-toggle-moon"
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    width="17"
                    height="17"
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
                    class="theme-toggle-icon theme-toggle-sun hidden"
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    width="17"
                    height="17"
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
