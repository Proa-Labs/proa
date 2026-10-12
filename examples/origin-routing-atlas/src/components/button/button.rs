// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Button component implementation
use super::recipe::{
    ButtonColor, ButtonRadius, ButtonRecipeProps, ButtonSize, ButtonVariant, BUTTON_RECIPE,
};
use crate::components::_proa::free_web::binding_to_js_attr;
use crate::components::spinner::{Spinner, SpinnerSize};
use proa_actions::ActionDataStatic;

use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_ir::ReactiveAttr;
use proa_macros::html_sync;

/// Stable DOM contract for Button across Rust and generated render targets.
pub const BUTTON_ANATOMY: crate::components::_proa::Anatomy =
    crate::components::_proa::Anatomy::new("button", &["root"]);

/// Button type for form behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonType {
    Button,
    Submit,
    Reset,
}
impl ButtonType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ButtonType::Button => "button",
            ButtonType::Submit => "submit",
            ButtonType::Reset => "reset",
        }
    }
}
/// Type-safe Button component using ButtonRecipe.
///
/// Generic over the children type `C`, which can be any type implementing
/// `WebRender<NoLoader, Vec<u8>>`. This allows using `html_sync! { ... }` directly as children.
///
/// The button supports four independent styling dimensions:
/// - **size**: Sm, Md, Lg, Icon (controls height and padding)
/// - **color**: Primary, Secondary, Destructive (semantic color meaning)
/// - **variant**: Solid, Outline, Ghost (visual style)
/// - **radius**: None, Sm, Md, Lg, Full (border radius)
///
/// # Example
///
/// ```ignore
/// use crate::components::{Button, ButtonColor, ButtonVariant, ButtonSize};
/// use proa_macros::html_sync;
///
/// html_sync! {
///     <div>
///         {Button {
///             color: Some(ButtonColor::Destructive),
///             variant: Some(ButtonVariant::Outline),
///             size: Some(ButtonSize::Lg),
///             children: Some(html_sync!{ "Delete" }),
///             ..Button::default()
///         }}
///     </div>
/// }.render(cx);
/// ```
pub struct Button<C = ()> {
    pub size: Option<ButtonSize>,
    pub color: Option<ButtonColor>,
    pub variant: Option<ButtonVariant>,
    pub radius: Option<ButtonRadius>,
    pub disabled: ReactiveAttr<bool>,
    pub loading: ReactiveAttr<bool>,
    pub button_type: ButtonType,
    pub class: Option<&'static str>,
    pub id: Option<&'static str>,
    pub children: Option<C>,
    pub onclick: Option<ActionDataStatic>,
    pub onblur: Option<ActionDataStatic>,
    pub onfocus: Option<ActionDataStatic>,
}
impl Button<()> {
    /// Create a new button with default settings and no children
    pub const fn new() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            disabled: ReactiveAttr::static_value(false),
            loading: ReactiveAttr::static_value(false),
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: None,
            onclick: None,
            onblur: None,
            onfocus: None,
        }
    }
    /// Create a button with text content
    pub const fn text(text: &'static str) -> Button<&'static str> {
        Button {
            size: None,
            color: None,
            variant: None,
            radius: None,
            disabled: ReactiveAttr::static_value(false),
            loading: ReactiveAttr::static_value(false),
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: Some(text),
            onclick: None,
            onblur: None,
            onfocus: None,
        }
    }
}
impl<C> Default for Button<C>
where
    C: Default,
{
    fn default() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            disabled: ReactiveAttr::default(),
            loading: ReactiveAttr::default(),
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: None,
            onclick: None,
            onblur: None,
            onfocus: None,
        }
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf, C: WebRenderSync<L, B>> WebRenderSync<L, B>
    for Button<C>
{
    #[inline(always)]
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
        let onclick_action = if let Some(action) = self.onclick.as_ref() {
            Some(action.action)
        } else {
            None
        };
        let onblur_action = if let Some(action) = self.onblur.as_ref() {
            Some(action.action)
        } else {
            None
        };
        let onfocus_action = if let Some(action) = self.onfocus.as_ref() {
            Some(action.action)
        } else {
            None
        };
        let is_disabled = self.disabled.default || self.loading.default;
        let disabled_binding_string = binding_to_js_attr(&self.disabled.binding);
        let loading_binding_string = binding_to_js_attr(&self.loading.binding);
        let disabled_binding = disabled_binding_string.as_deref();
        let loading_binding = loading_binding_string.as_deref();
        let is_loading = self.loading.default;
        html_sync! {
            <button
                data-button
                data-scope={BUTTON_ANATOMY.scope()}
                data-part="root"
                data-slot="button"
                data-size={style.size().as_str()}
                data-color={style.color().as_str()}
                data-variant={style.variant().as_str()}
                data-radius={style.radius().as_str()}
                type={self.button_type.as_str()}
                class={proa_macros::text!("{} {}", style, self.class)}
                id={self.id}
                disabled={is_disabled}
                data-onclick={onclick_action}
                data-onblur={onblur_action}
                data-onfocus={onfocus_action}
                data-bind-disabled={disabled_binding}
                data-bind-loading={loading_binding}
            >
                {if is_loading { Spinner { size: Some(SpinnerSize::Sm), class: None, id: None } }}
                {self.children}
            </button>
        }
        .render(cx)
    }
}
