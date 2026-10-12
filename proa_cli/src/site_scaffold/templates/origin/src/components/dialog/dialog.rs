// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND (ISC AND MIT)

//! Dialog component implementations
//!
//! These components render the initial SSR markup. The TypeScript hydration
//! code handles the interactive behaviors (portal, focus trap, etc.).
use super::recipe::{DialogContentRecipeProps, DialogRootRecipeProps, DialogSize, DIALOG_RECIPE};
use super::DIALOG_ANATOMY;
use crate::components::button::{
    ButtonColor, ButtonRadius, ButtonRecipeProps, ButtonSize, ButtonVariant, BUTTON_RECIPE,
};
use proa_core::{StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

const CLOSE_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>"#;
/// Dialog root component - manages the dialog state
///
/// On SSR, this renders a container with data attributes for hydration.
/// The TypeScript code reads these attributes and sets up the interactive behavior.
pub struct Dialog<Trigger = (), Content = ()> {
    /// Whether the dialog starts open (for SSR)
    pub default_open: bool,
    /// Dialog ID for linking trigger and content
    pub id: Option<&'static str>,
    /// The trigger element (button that opens dialog)
    pub trigger: Option<Trigger>,
    /// The dialog content
    pub content: Option<Content>,
}
impl Dialog<(), ()> {
    pub const fn new() -> Self {
        Self {
            default_open: false,
            id: None,
            trigger: None,
            content: None,
        }
    }
}
impl<Trigger, Content> Default for Dialog<Trigger, Content> {
    fn default() -> Self {
        Self {
            default_open: false,
            id: None,
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
    > WebRenderSync<L, B> for Dialog<Trigger, Content>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let state = crate::components::_proa::OverlayState::new(self.default_open, true);
        let initial_display = if state.hidden() { "none" } else { "block" };
        let style = DIALOG_RECIPE.resolve_root(DialogRootRecipeProps);
        html_sync! {
            <div
                id={self.id}
                data-dialog-root
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="root"
                data-slot="dialog"
                data-state={state.data_state()}
                data-dialog-id={self.id}
                class={style}
                style={proa_core::style_ok(proa_macros::text!("--proa-overlay-display: {};", initial_display))}
            >
                {self.trigger}
                {self.content}
            </div>
        }
        .render(cx)
    }
}
/// DialogTrigger - the element that opens the dialog
pub struct DialogTrigger<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for DialogTrigger<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogTrigger<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <button
                type="button"
                data-dialog-trigger
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="trigger"
                data-slot="dialog-trigger"
                aria-haspopup="dialog"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.trigger, self.class)}
            >
                {self.children}
            </button>
        }
        .render(cx)
    }
}
/// A dialog trigger styled through the typed Button recipe.
///
/// This renders one semantic `<button>` carrying the Dialog trigger contract.
/// It exists separately from [`DialogTrigger`] so existing struct literals
/// remain source-compatible and callers never need to nest a Button inside
/// another button.
pub struct DialogButtonTrigger<C = ()> {
    pub size: Option<ButtonSize>,
    pub color: Option<ButtonColor>,
    pub variant: Option<ButtonVariant>,
    pub radius: Option<ButtonRadius>,
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl DialogButtonTrigger<()> {
    /// Create a typed dialog button trigger with Button's default appearance.
    pub const fn new() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            class: None,
            children: None,
        }
    }

    /// Create a typed dialog button trigger with a static text label.
    pub const fn text(text: &'static str) -> DialogButtonTrigger<&'static str> {
        DialogButtonTrigger {
            size: None,
            color: None,
            variant: None,
            radius: None,
            class: None,
            children: Some(text),
        }
    }
}
impl<C> Default for DialogButtonTrigger<C> {
    fn default() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogButtonTrigger<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = BUTTON_RECIPE.resolve(ButtonRecipeProps {
            size: self.size,
            color: self.color,
            variant: self.variant,
            radius: self.radius,
        });
        html_sync! {
            <button
                type="button"
                data-dialog-trigger
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="trigger"
                data-slot="dialog-trigger"
                data-size={style.size().as_str()}
                data-color={style.color().as_str()}
                data-variant={style.variant().as_str()}
                data-radius={style.radius().as_str()}
                aria-haspopup="dialog"
                class={proa_macros::text!("{} {}", style, self.class)}
            >
                {self.children}
            </button>
        }
        .render(cx)
    }
}
/// DialogOverlay - the backdrop behind the dialog
#[derive(Default)]
pub struct DialogOverlay {
    pub class: Option<&'static str>,
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DialogOverlay {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-dialog-overlay
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="overlay"
                data-slot="dialog-overlay"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.overlay, self.class)}
                aria-hidden="true"
            ></div>
        }
        .render(cx)
    }
}
/// DialogContent - the main dialog container
pub struct DialogContent<C = ()> {
    pub size: Option<DialogSize>,
    pub class: Option<&'static str>,
    pub children: Option<C>,
    /// Whether to show the close button (X icon)
    pub show_close: bool,
}
impl<C> Default for DialogContent<C> {
    fn default() -> Self {
        Self {
            size: None,
            class: None,
            children: None,
            show_close: true,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogContent<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = DIALOG_RECIPE.resolve_content(DialogContentRecipeProps { size: self.size });
        let size = style.size();
        html_sync! {
            <div
                data-dialog-portal
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="portal"
                data-slot="dialog-portal"
                style="display: var(--proa-overlay-display, none);"
            >
                <div
                    data-dialog-overlay
                    data-scope={DIALOG_ANATOMY.scope()}
                    data-part="overlay"
                    data-slot="dialog-overlay"
                    class={DIALOG_RECIPE.overlay}
                    aria-hidden="true"
                ></div>
                <div
                    data-dialog-content
                    data-scope={DIALOG_ANATOMY.scope()}
                    data-part="content"
                    data-slot="dialog-content"
                    data-size={size.as_str()}
                    class={proa_macros::text!("{} {}", style, self.class)}
                    role="dialog"
                    aria-modal="true"
                    tabindex="-1"
                >
                    {self.children}
                    {if self.show_close { html_sync! {
                            <button type="button" data-dialog-close data-scope={DIALOG_ANATOMY.scope()} data-part="close-button" data-slot="dialog-close-button" class={DIALOG_RECIPE.close_button}>
                                <span data-scope={DIALOG_ANATOMY.scope()} data-part="close-label" data-slot="dialog-close-label" class={DIALOG_RECIPE.close_label}>
                                    "Close"
                                </span>
                                {StaticRaw(CLOSE_ICON)}
                            </button>
                        } }}
                </div>
            </div>
        }
            .render(cx)
    }
}
/// DialogHeader - container for title and description
pub struct DialogHeader<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for DialogHeader<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogHeader<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="header"
                data-slot="dialog-header"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.header, self.class)}
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
/// DialogTitle - accessible dialog title
pub struct DialogTitle<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for DialogTitle<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogTitle<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <h2
                data-dialog-title
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="title"
                data-slot="dialog-title"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.title, self.class)}
            >
                {self.children}
            </h2>
        }
        .render(cx)
    }
}
/// DialogDescription - optional description text
pub struct DialogDescription<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for DialogDescription<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogDescription<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <p
                data-dialog-description
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="description"
                data-slot="dialog-description"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.description, self.class)}
            >
                {self.children}
            </p>
        }
        .render(cx)
    }
}
/// DialogFooter - container for action buttons
pub struct DialogFooter<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for DialogFooter<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogFooter<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <div
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="footer"
                data-slot="dialog-footer"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.footer, self.class)}
            >
                {self.children}
            </div>
        }
        .render(cx)
    }
}
/// DialogClose - wraps an author-provided control that closes the dialog
///
/// This renders a `<span>`, not a `<button>`, and it has to: most callers pass
/// a `Button` (or a link) as the child, and `<button>` may not contain
/// interactive content. The parser enforces that — a nested `<button>` start
/// tag closes the open one — so promoting this element would emit the author's
/// button as a *sibling*, stripped of `data-dialog-close`, and their Cancel
/// would stop closing the dialog.
///
/// When the close is the control rather than a wrapper around one, reach for
/// [`DialogCloseButton`]: a span holding a bare label is not focusable, not
/// keyboard-activatable, and has no role.
pub struct DialogClose<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl<C> Default for DialogClose<C> {
    fn default() -> Self {
        Self {
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogClose<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        html_sync! {
            <span
                data-dialog-close
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="close"
                data-slot="dialog-close"
                class={proa_macros::text!("{} {}", DIALOG_RECIPE.close, self.class)}
            >
                {self.children}
            </span>
        }
        .render(cx)
    }
}
/// A dialog close styled through the typed Button recipe.
///
/// The counterpart to [`DialogButtonTrigger`], and it exists for the same
/// reason: one semantic `<button>` carrying the close contract, so a caller
/// with nothing but a label ("Cancel") gets a real control — focusable,
/// keyboard-activatable, announced as a button — instead of the bare `<span>`
/// [`DialogClose`] must stay in order to keep wrapping author-provided buttons.
pub struct DialogCloseButton<C = ()> {
    pub size: Option<ButtonSize>,
    pub color: Option<ButtonColor>,
    pub variant: Option<ButtonVariant>,
    pub radius: Option<ButtonRadius>,
    pub class: Option<&'static str>,
    pub children: Option<C>,
}
impl DialogCloseButton<()> {
    /// Create a typed dialog close button with Button's default appearance.
    pub const fn new() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            class: None,
            children: None,
        }
    }

    /// Create a typed dialog close button with a static text label.
    pub const fn text(text: &'static str) -> DialogCloseButton<&'static str> {
        DialogCloseButton {
            size: None,
            color: None,
            variant: None,
            radius: None,
            class: None,
            children: Some(text),
        }
    }
}
impl<C> Default for DialogCloseButton<C> {
    fn default() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            class: None,
            children: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for DialogCloseButton<C>
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = BUTTON_RECIPE.resolve(ButtonRecipeProps {
            size: self.size,
            color: self.color,
            variant: self.variant,
            radius: self.radius,
        });
        html_sync! {
            <button
                type="button"
                data-dialog-close
                data-scope={DIALOG_ANATOMY.scope()}
                data-part="close"
                data-slot="dialog-close"
                data-size={style.size().as_str()}
                data-color={style.color().as_str()}
                data-variant={style.variant().as_str()}
                data-radius={style.radius().as_str()}
                class={proa_macros::text!("{} {}", style, self.class)}
            >
                {self.children}
            </button>
        }
        .render(cx)
    }
}
