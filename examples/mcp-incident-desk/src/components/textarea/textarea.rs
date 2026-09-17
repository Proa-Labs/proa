// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Textarea component implementation
#![allow(unused_must_use)]

use super::recipe::{TextareaRecipeProps, TextareaSize, TEXTAREA_RECIPE};
use crate::components::_proa::free_web::{binding_to_js_attr, JsonAttr};

use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_ir::{FieldRegistration, ReactiveAttr};
use proa_macros::html_sync;
/// Type-safe Textarea component.
///
/// A form textarea field with support for various sizes and states.
/// Follows the shadcn/ui textarea design with Tailwind CSS styling.
///
/// # Example
///
/// ```ignore
/// use crate::components::{Textarea, TextareaSize};
/// use proa_macros::html_sync;
///
/// // Basic usage
/// html_sync! {
///     <div>
///         {Textarea {
///             placeholder: Some("Type your message here..."),
///             size: Some(TextareaSize::Default),
///             ..Textarea::new()
///         }}
///     </div>
/// }.render(cx);
///
/// // With form registration
/// let form = ContactForm::form();
/// html_sync! {
///     <div>
///         {Textarea {
///             registration: Some(form.register_message()),
///             placeholder: Some("Your message"),
///             aria_invalid: form.field_message().has_error(),
///             ..Textarea::new()
///         }}
///     </div>
/// }.render(cx);
/// ```
pub struct Textarea {
    pub size: Option<TextareaSize>,
    pub name: Option<&'static str>,
    pub value: Option<&'static str>,
    pub placeholder: Option<&'static str>,
    pub disabled: ReactiveAttr<bool>,
    pub required: bool,
    pub readonly: bool,
    pub autofocus: bool,
    pub rows: Option<&'static str>,
    pub cols: Option<&'static str>,
    pub minlength: Option<&'static str>,
    pub maxlength: Option<&'static str>,
    pub wrap: Option<TextareaWrap>,
    pub aria_label: Option<&'static str>,
    pub aria_describedby: Option<&'static str>,
    pub aria_invalid: ReactiveAttr<bool>,
    /// Field registration data (from form.register_*)
    pub registration: Option<FieldRegistration>,
    pub class: Option<&'static str>,
    pub id: Option<&'static str>,
}
/// Wrap mode for textarea
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextareaWrap {
    #[default]
    Soft,
    Hard,
    Off,
}
impl TextareaWrap {
    /// Get the HTML wrap attribute value
    pub const fn as_str(self) -> &'static str {
        match self {
            TextareaWrap::Soft => "soft",
            TextareaWrap::Hard => "hard",
            TextareaWrap::Off => "off",
        }
    }
}
impl Textarea {
    /// Create a new textarea with default settings
    pub const fn new() -> Self {
        Self {
            size: None,
            name: None,
            value: None,
            placeholder: None,
            disabled: ReactiveAttr::static_value(false),
            required: false,
            readonly: false,
            autofocus: false,
            rows: None,
            cols: None,
            minlength: None,
            maxlength: None,
            wrap: None,
            aria_label: None,
            aria_describedby: None,
            aria_invalid: ReactiveAttr::static_value(false),
            registration: None,
            class: None,
            id: None,
        }
    }
}
impl Default for Textarea {
    fn default() -> Self {
        Self::new()
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Textarea {
    #[inline(always)]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = TEXTAREA_RECIPE.resolve(TextareaRecipeProps { size: self.size });
        let is_disabled = self.disabled.default;
        let is_aria_invalid = self.aria_invalid.default;
        let disabled_binding_string = binding_to_js_attr(&self.disabled.binding);
        let aria_invalid_binding_string = binding_to_js_attr(&self.aria_invalid.binding);
        let disabled_binding = disabled_binding_string.as_deref();
        let aria_invalid_binding = aria_invalid_binding_string.as_deref();
        let registration = self.registration.as_ref();
        let rules_attr = JsonAttr(registration.map(|reg| reg.rules.as_slice()));
        let data_field = registration.map(|reg| reg.data_field);
        let name_attr = registration.map(|reg| reg.name).or(self.name);
        let validate_on = registration.map(|reg| match reg.validate_on {
            proa_ir::ValidationTrigger::Blur => "blur",
            proa_ir::ValidationTrigger::Input => "input",
            proa_ir::ValidationTrigger::Change => "change",
            proa_ir::ValidationTrigger::Submit => "submit",
        });
        let wrap_attr = self.wrap.map(|w| w.as_str());
        html_sync! {
            <textarea
                class={proa_macros::text!("{} {}", style, self.class)}
                id={self.id}
                name={name_attr}
                placeholder={self.placeholder}
                disabled={is_disabled}
                required={self.required}
                readonly={self.readonly}
                autofocus={self.autofocus}
                rows={self.rows}
                cols={self.cols}
                minlength={self.minlength}
                maxlength={self.maxlength}
                wrap={wrap_attr}
                aria-label={self.aria_label}
                aria-describedby={self.aria_describedby}
                aria-invalid={is_aria_invalid}
                data-textarea
                data-slot="textarea"
                data-size={style.size().as_str()}
                data-field={data_field}
                data-rules={rules_attr}
                data-validate-on={validate_on}
                data-bind-disabled={disabled_binding}
                data-bind-aria-invalid={aria_invalid_binding}
            >
                {proa_core::raw_text_ok(self.value.unwrap_or(""))}
            </textarea>
        }
        .render(cx)
    }
}
