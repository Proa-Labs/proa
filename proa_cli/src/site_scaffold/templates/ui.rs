// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub fn badge_rs() -> &'static str {
    r#"use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

use super::recipe::badge_classes;

#[derive(Debug, Clone, Copy)]
pub struct Badge {
    pub label: &'static str,
    pub class: Option<&'static str>,
}

impl Badge {
    pub const fn new(label: &'static str) -> Self {
        Self { label, class: None }
    }
}

impl<L: DataLoader> WebRenderSync<L> for Badge {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = badge_classes().add_opt(self.class);
        html_sync! {
            <span data-badge class={classes}>{self.label}</span>
        }
        .render(cx)
    }
}
"#
}

pub fn badge_recipe_rs() -> &'static str {
    r#"use proa_core::ClassList;

pub const BADGE_BASE: &str = "inline-flex items-center border border-current px-2 py-1 font-mono text-[10px] uppercase tracking-[0.18em]";

pub fn badge_classes() -> ClassList<1> {
    ClassList::new().add(BADGE_BASE)
}
"#
}

pub fn badge_md_rs() -> &'static str {
    r#"use proa_core::{WebContext, MdRenderSync, WriteBuf, WriteError};

use super::Badge;

impl<L: proa_core::DataLoader, B: WriteBuf> MdRenderSync<L, B> for Badge {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_core::MdRenderSync::render_md(self.label, cx)
    }
}
"#
}

pub fn badge_mod_rs() -> &'static str {
    "mod badge;\nmod md;\nmod recipe;\n\npub use badge::Badge;\n"
}

pub fn button_rs() -> &'static str {
    r#"use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

use super::recipe::button_classes;

#[derive(Debug, Clone, Copy)]
pub struct Button {
    pub label: &'static str,
    pub href: Option<&'static str>,
    pub class: Option<&'static str>,
}

impl Button {
    pub const fn link(label: &'static str, href: &'static str) -> Self {
        Self { label, href: Some(href), class: None }
    }
}

impl<L: DataLoader> WebRenderSync<L> for Button {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = button_classes().add_opt(self.class);
        if self.href.is_some() {
            let href = self.href.unwrap_or_default();
            html_sync! { <a data-button href={href} class={classes}>{self.label}</a> }.render(cx)
        } else {
            html_sync! { <button data-button class={classes}>{self.label}</button> }.render(cx)
        }
    }
}
"#
}

pub fn button_recipe_rs() -> &'static str {
    r#"use proa_core::ClassList;

pub const BUTTON_BASE: &str = "inline-flex items-center justify-center border border-[#3de1e8] bg-[#3de1e8] px-5 py-3 font-mono text-xs font-bold uppercase tracking-[0.16em] text-[#0b0b0c] transition hover:bg-transparent hover:text-[#3de1e8]";

pub fn button_classes() -> ClassList<1> {
    ClassList::new().add(BUTTON_BASE)
}
"#
}

pub fn button_md_rs() -> &'static str {
    r#"use proa_core::{WebContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

use super::Button;

impl<L: proa_core::DataLoader, B: WriteBuf> MdRenderSync<L, B> for Button {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if self.href.is_some() {
            let href = self.href.unwrap_or_default();
            md_sync! { "[" { self.label } "](" { href } ")" }.render_md(cx)
        } else {
            proa_core::MdRenderSync::render_md(self.label, cx)
        }
    }
}
"#
}

pub fn button_mod_rs() -> &'static str {
    "mod button;\nmod md;\nmod recipe;\n\npub use button::Button;\n"
}

pub fn card_rs() -> &'static str {
    r#"use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;

use super::recipe::card_classes;

#[derive(Debug, Clone, Copy)]
pub struct Card {
    pub eyebrow: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub class: Option<&'static str>,
}

impl<L: DataLoader> WebRenderSync<L> for Card {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = card_classes().add_opt(self.class);
        html_sync! {
            <article data-card class={classes}>
                <p class="card-eyebrow">{self.eyebrow}</p>
                <h2>{self.title}</h2>
                <p>{self.body}</p>
            </article>
        }
        .render(cx)
    }
}
"#
}

pub fn card_recipe_rs() -> &'static str {
    r#"use proa_core::ClassList;

pub const CARD_BASE: &str = "border border-white/20 bg-white/[0.035] p-6";

pub fn card_classes() -> ClassList<1> {
    ClassList::new().add(CARD_BASE)
}
"#
}

pub fn card_md_rs() -> &'static str {
    r###"use proa_core::{WebContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

use super::Card;

impl<L: proa_core::DataLoader, B: WriteBuf> MdRenderSync<L, B> for Card {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { { self.eyebrow } "\n\n## " { self.title } "\n\n" { self.body } }.render_md(cx)
    }
}
"###
}

pub fn card_mod_rs() -> &'static str {
    "mod card;\nmod md;\nmod recipe;\n\npub use card::Card;\n"
}
