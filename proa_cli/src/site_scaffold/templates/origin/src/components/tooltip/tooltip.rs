// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tooltip component implementations
//!
//! SSR markup for tooltips. The JavaScript hydration handles hover/focus
//! detection, positioning, and show/hide animations.
use super::recipe::{TooltipAlign, TooltipRecipeProps, TooltipSide, TOOLTIP_RECIPE};
use super::TOOLTIP_ANATOMY;
use crate::components::_proa::DisclosureState;
use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;
/// Delay preset for tooltip show delay
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TooltipDelay {
    /// No delay (0ms)
    None,
    /// Short delay (100ms)
    Short,
    /// Normal delay (200ms) - default
    #[default]
    Normal,
    /// Long delay (400ms)
    Long,
}
impl TooltipDelay {
    pub const fn as_str(self) -> &'static str {
        match self {
            TooltipDelay::None => "0",
            TooltipDelay::Short => "100",
            TooltipDelay::Normal => "200",
            TooltipDelay::Long => "400",
        }
    }
}
/// Tooltip root component - manages the tooltip state
pub struct Tooltip<Trigger = (), Content = ()> {
    /// Preferred side for the tooltip
    pub side: TooltipSide,
    /// Alignment relative to trigger
    pub align: TooltipAlign,
    /// Delay before showing - used as data attribute for JS
    pub delay: TooltipDelay,
    /// The trigger element
    pub trigger: Option<Trigger>,
    /// The tooltip content
    pub content: Option<Content>,
}
impl Tooltip<(), ()> {
    pub const fn new() -> Self {
        Self {
            side: TooltipSide::Top,
            align: TooltipAlign::Center,
            delay: TooltipDelay::Normal,
            trigger: None,
            content: None,
        }
    }
}
impl<Trigger, Content> Default for Tooltip<Trigger, Content> {
    fn default() -> Self {
        Self {
            side: TooltipSide::Top,
            align: TooltipAlign::Center,
            delay: TooltipDelay::Normal,
            trigger: None,
            content: None,
        }
    }
}
impl<
        L: ::proa_core::DataLoader,
        B: WriteBuf,
        Trigger: WebRenderSync<L, B>,
        Content: WebRenderSync<L, B>,
    > WebRenderSync<L, B> for Tooltip<Trigger, Content>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = TOOLTIP_RECIPE.resolve(TooltipRecipeProps {
            side: Some(self.side),
            align: Some(self.align),
        });
        let state = DisclosureState::Closed;
        html_sync! {
            <div
                data-tooltip-root
                data-scope={TOOLTIP_ANATOMY.scope()}
                data-part="root"
                data-slot="tooltip"
                data-state={state}
                data-side={style.side().as_str()}
                data-align={style.align().as_str()}
                data-delay={self.delay.as_str()}
                class={style}
                style="--proa-overlay-display: none;"
            >
                {self.trigger}
                {self.content}
            </div>
        }
        .render(cx)
    }
}
/// TooltipTrigger - the element that shows the tooltip on hover/focus
pub struct TooltipTrigger<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for TooltipTrigger<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for TooltipTrigger<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <button
                type="button"
                data-tooltip-trigger
                data-scope={TOOLTIP_ANATOMY.scope()}
                data-part="trigger"
                data-slot="tooltip-trigger"
                class={proa_macros::text!("{} {}", TOOLTIP_RECIPE.trigger, self.class)}
            >
                {self.children}
            </button>
        }
        .render(cx)
    }
}
/// TooltipContent - the tooltip container (typically just text)
pub struct TooltipContent<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for TooltipContent<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for TooltipContent<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-tooltip-content
                data-scope={TOOLTIP_ANATOMY.scope()}
                data-part="content"
                data-slot="tooltip-content"
                role="tooltip"
                class={proa_macros::text!("{} {}", TOOLTIP_RECIPE.content, self.class)}
                style="display: var(--proa-overlay-display, none); position: absolute;"
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
