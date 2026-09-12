// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Badge component implementation
//!
//! Renders a small status indicator or label with variant styling.
use super::recipe::{BadgeRecipeProps, BadgeVariant, BADGE_RECIPE};
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
/// Badge component - displays a small status indicator or label
pub struct Badge<C = ()> {
    /// Style variant
    pub variant: BadgeVariant,
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The badge content (text or other elements)
    pub children: Option<C>,
}
impl Badge<()> {
    pub const fn new() -> Self {
        Self {
            variant: BadgeVariant::Default,
            class: None,
            children: None,
        }
    }
}
impl<C> Default for Badge<C> {
    fn default() -> Self {
        Self {
            variant: BadgeVariant::Default,
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for Badge<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = BADGE_RECIPE.resolve(BadgeRecipeProps {
            variant: Some(self.variant),
        });
        html_sync! {
            <span
                data-badge
                data-slot="badge"
                data-variant={style.variant().as_str()}
                class={proa_macros::text!("{} {}", style, self.class)}
            >
                {self.children}
            </span>
        }
        .render(cx)
    }
}
