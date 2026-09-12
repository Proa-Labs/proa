// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Card component implementation
//!
//! Renders a container for grouped content with header, body, and footer sections.
use super::recipe::{CardRecipeProps, CARD_RECIPE};
use super::CARD_ANATOMY;
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
/// Card component - a container for grouped content
pub struct Card<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The card content
    pub children: Option<C>,
}
impl Card<()> {
    pub const fn new() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<C> Default for Card<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for Card<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = CARD_RECIPE.resolve(CardRecipeProps);
        html_sync! {
            <div
                data-card
                data-scope={CARD_ANATOMY.scope()}
                data-part="root"
                data-slot="card"
                class={proa_macros::text!("{} {}", style, self.class)}
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
/// CardHeader component - container for title and description
pub struct CardHeader<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The header content
    pub children: Option<C>,
}
impl<C> Default for CardHeader<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for CardHeader<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-card-header
                data-scope={CARD_ANATOMY.scope()}
                data-part="header"
                data-slot="card-header"
                class={proa_macros::text!("{} {}", CARD_RECIPE.header, self.class)}
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
/// CardTitle component - the card's heading
pub struct CardTitle<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The title content
    pub children: Option<C>,
}
impl<C> Default for CardTitle<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for CardTitle<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <h3
                data-card-title
                data-scope={CARD_ANATOMY.scope()}
                data-part="title"
                data-slot="card-title"
                class={proa_macros::text!("{} {}", CARD_RECIPE.title, self.class)}
            >
                {self.children}
            </h3>
        }
        .render(cx)
    }
}
/// CardDescription component - supplementary text below the title
pub struct CardDescription<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The description content
    pub children: Option<C>,
}
impl<C> Default for CardDescription<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for CardDescription<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <p
                data-card-description
                data-scope={CARD_ANATOMY.scope()}
                data-part="description"
                data-slot="card-description"
                class={proa_macros::text!("{} {}", CARD_RECIPE.description, self.class)}
            >
                {self.children}
            </p>
        }
        .render(cx)
    }
}
/// CardContent component - the main body of the card
pub struct CardContent<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The content
    pub children: Option<C>,
}
impl<C> Default for CardContent<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for CardContent<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-card-content
                data-scope={CARD_ANATOMY.scope()}
                data-part="content"
                data-slot="card-content"
                class={proa_macros::text!("{} {}", CARD_RECIPE.content, self.class)}
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
/// CardFooter component - container for action buttons at the bottom
pub struct CardFooter<C = ()> {
    /// Additional CSS classes
    pub class: Option<&'static str>,
    /// The footer content
    pub children: Option<C>,
}
impl<C> Default for CardFooter<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<B: WriteBuf, C: WebRenderSync<L, B>, L: ::proa_core::DataLoader> WebRenderSync<L, B>
    for CardFooter<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-card-footer
                data-scope={CARD_ANATOMY.scope()}
                data-part="footer"
                data-slot="card-footer"
                class={proa_macros::text!("{} {}", CARD_RECIPE.footer, self.class)}
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
