// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Input component implementation
use super::recipe::{InputRecipeProps, InputSize, InputType, INPUT_RECIPE};
use crate::components::_proa::free_web::{binding_to_js_attr, JsonAttr};

use proa_core::{WebContext, WebRenderSync, WriteBuf};
use proa_ir::{FieldRegistration, ReactiveAttr};
use proa_macros::html_sync;
/// Type-safe Input component.
///
/// A form input field with support for various input types, sizes, and states.
/// Follows the shadcn/ui input design with Tailwind CSS styling.
///
/// # Example
///
/// ```ignore
/// use crate::components::{Input, InputType, InputSize};
/// use proa_macros::html_sync;
///
/// // Basic usage
/// html_sync! {
///     <div>
///         {Input {
///             input_type: InputType::Email,
///             placeholder: Some("you@example.com"),
///             size: Some(InputSize::Default),
///             ..Input::new()
///         }}
///     </div>
/// }.render(cx);
///
/// // With form registration (React Hook Form style)
/// let form = SignUpForm::form();
/// html_sync! {
///     <div>
///         {Input {
///             registration: Some(form.register_email()),
///             input_type: InputType::Email,
///             placeholder: Some("you@example.com"),
///             aria_invalid: form.field_email().has_error(),
///             ..Input::new()
///         }}
///     </div>
/// }.render(cx);
/// ```
pub struct Input {
    pub size: Option<InputSize>,
    pub input_type: InputType,
    pub name: Option<&'static str>,
    pub value: Option<&'static str>,
    pub placeholder: Option<&'static str>,
    pub disabled: ReactiveAttr<bool>,
    pub required: bool,
    pub readonly: bool,
    pub autofocus: bool,
    pub autocomplete: Option<&'static str>,
    pub min: Option<&'static str>,
    pub max: Option<&'static str>,
    pub minlength: Option<&'static str>,
    pub maxlength: Option<&'static str>,
    pub pattern: Option<&'static str>,
    pub step: Option<&'static str>,
    pub aria_label: Option<&'static str>,
    pub aria_describedby: Option<&'static str>,
    pub aria_invalid: ReactiveAttr<bool>,
    /// Field registration data (from form.register_*)
    pub registration: Option<FieldRegistration>,
    pub class: Option<&'static str>,
    pub id: Option<&'static str>,
}
impl Input {
    /// Create a new input with default settings
    pub const fn new() -> Self {
        Self {
            size: None,
            input_type: InputType::Text,
            name: None,
            value: None,
            placeholder: None,
            disabled: ReactiveAttr::static_value(false),
            required: false,
            readonly: false,
            autofocus: false,
            autocomplete: None,
            min: None,
            max: None,
            minlength: None,
            maxlength: None,
            pattern: None,
            step: None,
            aria_label: None,
            aria_describedby: None,
            aria_invalid: ReactiveAttr::static_value(false),
            registration: None,
            class: None,
            id: None,
        }
    }
}
impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}
impl<L: ::proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Input {
    #[inline(always)]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::shared::write_buf::WriteError> {
        let style = INPUT_RECIPE.resolve(InputRecipeProps { size: self.size });
        let is_disabled = self.disabled.default;
        let is_aria_invalid = self.aria_invalid.default;
        let disabled_binding_string = binding_to_js_attr(&self.disabled.binding);
        let aria_invalid_binding_string = binding_to_js_attr(&self.aria_invalid.binding);
        let disabled_binding = disabled_binding_string.as_deref();
        let aria_invalid_binding = aria_invalid_binding_string.as_deref();
        let registration = self.registration.as_ref();
        let rules_attr = JsonAttr(if let Some(reg) = registration {
            Some(reg.rules.as_slice())
        } else {
            None
        });
        let data_field = if let Some(reg) = registration {
            Some(reg.data_field)
        } else {
            None
        };
        let name_attr = if let Some(reg) = registration {
            Some(reg.name)
        } else {
            self.name
        };
        let validate_on = if let Some(reg) = registration {
            Some(validation_trigger_label(reg.validate_on))
        } else {
            None
        };
        html_sync! {
            <input
                type={self.input_type.as_str()}
                class={proa_macros::text!("{} {}", style, self.class)}
                id={self.id}
                name={name_attr}
                value={self.value}
                placeholder={self.placeholder}
                disabled={is_disabled}
                required={self.required}
                readonly={self.readonly}
                autofocus={self.autofocus}
                autocomplete={self.autocomplete}
                min={self.min}
                max={self.max}
                minlength={self.minlength}
                maxlength={self.maxlength}
                pattern={self.pattern}
                step={self.step}
                aria-label={self.aria_label}
                aria-describedby={self.aria_describedby}
                aria-invalid={is_aria_invalid}
                data-input
                data-slot="input"
                data-size={style.size().as_str()}
                data-field={data_field}
                data-rules={rules_attr}
                data-validate-on={validate_on}
                data-bind-disabled={disabled_binding}
                data-bind-aria-invalid={aria_invalid_binding}
            />
        }
        .render(cx)
    }
}

fn validation_trigger_label(trigger: proa_ir::ValidationTrigger) -> &'static str {
    match trigger {
        proa_ir::ValidationTrigger::Blur => "blur",
        proa_ir::ValidationTrigger::Input => "input",
        proa_ir::ValidationTrigger::Change => "change",
        proa_ir::ValidationTrigger::Submit => "submit",
    }
}
