// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! FormIR - The top-level structure representing a form's client-side behavior
//!
//! This is the main IR that gets compiled to JavaScript.

use crate::expr::Expr;
use crate::stmt::Block;
use crate::trigger::Effect;
use crate::types::{FieldId, SignalId};
use crate::validate::{InputFormat, Transform, ValidationRule, ValidationTrigger};
use crate::value::Value;
use serde::Serialize;

/// The complete IR for a form's client-side behavior
#[derive(Debug, Clone, Default, Serialize)]
pub struct FormIR {
    /// Form identifier (used for scoping)
    pub id: &'static str,

    /// Signal definitions (reactive state)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<SignalDef>,

    /// Field definitions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldDef>,

    /// Derived/computed values
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived: Vec<DerivedDef>,

    /// Effects (event handlers and reactive effects)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,

    /// Form-level options
    #[serde(default)]
    pub options: FormOptions,
}

/// A signal definition (reactive state)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SignalDef {
    /// Signal identifier
    pub id: SignalId,
    /// Initial value
    pub initial: Value,
    /// Whether to persist to localStorage
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist: Option<&'static str>,
}

impl SignalDef {
    pub fn new(name: &'static str, initial: impl Into<Value>) -> Self {
        Self {
            id: SignalId::new(name),
            initial: initial.into(),
            persist: None,
        }
    }

    pub fn persisted(
        name: &'static str,
        initial: impl Into<Value>,
        storage_key: &'static str,
    ) -> Self {
        Self {
            id: SignalId::new(name),
            initial: initial.into(),
            persist: Some(storage_key),
        }
    }
}

/// A form field definition
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldDef {
    /// Field identifier (matches the input's name attribute)
    pub id: FieldId,
    /// Initial/default value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<Value>,
    /// Validation rules
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ValidationRule>,
    /// When to validate
    #[serde(default)]
    pub validate_on: ValidationTrigger,
    /// Transforms to apply before validation
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<Transform>,
    /// Input format mask (formats displayed value as user types)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<InputFormat>,
    /// Whether the field is disabled
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<Expr>,
    /// Whether the field is readonly
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readonly: Option<Expr>,
}

impl FieldDef {
    pub fn new(name: &'static str) -> Self {
        Self {
            id: FieldId::new(name),
            initial: None,
            rules: Vec::new(),
            validate_on: ValidationTrigger::default(),
            transforms: Vec::new(),
            format: None,
            disabled: None,
            readonly: None,
        }
    }

    pub fn with_initial(mut self, value: impl Into<Value>) -> Self {
        self.initial = Some(value.into());
        self
    }

    pub fn with_rules(mut self, rules: Vec<ValidationRule>) -> Self {
        self.rules = rules;
        self
    }

    pub fn with_rule(mut self, rule: ValidationRule) -> Self {
        self.rules.push(rule);
        self
    }

    pub fn validate_on(mut self, trigger: ValidationTrigger) -> Self {
        self.validate_on = trigger;
        self
    }

    pub fn with_transforms(mut self, transforms: Vec<Transform>) -> Self {
        self.transforms = transforms;
        self
    }

    pub fn disabled_when(mut self, condition: Expr) -> Self {
        self.disabled = Some(condition);
        self
    }
}

/// A derived/computed value
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DerivedDef {
    /// Signal identifier for the derived value
    pub id: SignalId,
    /// Expression to compute the value
    pub expr: Expr,
}

impl DerivedDef {
    pub fn new(name: &'static str, expr: Expr) -> Self {
        Self {
            id: SignalId::new(name),
            expr,
        }
    }
}

/// Form-level options
#[derive(Debug, Clone, Serialize)]
pub struct FormOptions {
    /// Form submission URL (if using fetch-based submission)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,

    /// HTTP method for submission
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<crate::types::Method>,

    /// Whether to validate all fields on submit
    pub validate_on_submit: bool,

    /// Whether to scroll to first error on submit
    pub scroll_to_error: bool,

    /// Whether to focus first error field on submit
    pub focus_error: bool,

    /// Whether to show error summary
    #[serde(default)]
    pub show_error_summary: bool,

    /// Selector for error summary container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_summary_selector: Option<&'static str>,

    /// Whether to disable submit button when form is invalid
    #[serde(default)]
    pub disable_submit_when_invalid: bool,

    /// Whether to disable submit button while submitting
    pub disable_submit_while_loading: bool,

    /// Whether to reset form on successful submit
    #[serde(default)]
    pub reset_on_success: bool,

    /// Custom submit handler (overrides default form submission)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_submit: Option<Block>,

    /// Handler for successful submission
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success: Option<Block>,

    /// Handler for failed submission
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_error: Option<Block>,
}

impl Default for FormOptions {
    fn default() -> Self {
        Self {
            action: None,
            method: None,
            validate_on_submit: true,
            scroll_to_error: true,
            focus_error: true,
            show_error_summary: false,
            error_summary_selector: None,
            disable_submit_when_invalid: false,
            disable_submit_while_loading: true,
            reset_on_success: false,
            on_submit: None,
            on_success: None,
            on_error: None,
        }
    }
}

impl FormIR {
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn with_signal(mut self, name: &'static str, initial: impl Into<Value>) -> Self {
        self.signals.push(SignalDef::new(name, initial));
        self
    }

    pub fn with_field(mut self, field: FieldDef) -> Self {
        self.fields.push(field);
        self
    }

    pub fn with_derived(mut self, name: &'static str, expr: Expr) -> Self {
        self.derived.push(DerivedDef::new(name, expr));
        self
    }

    pub fn with_effect(mut self, effect: Effect) -> Self {
        self.effects.push(effect);
        self
    }

    pub fn with_options(mut self, options: FormOptions) -> Self {
        self.options = options;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stmt::Stmt;
    use crate::trigger::Trigger;

    #[test]
    fn test_form_ir_creation() {
        let form = FormIR::new("signup")
            .with_signal("loading", false)
            .with_signal("error", Value::Null)
            .with_field(
                FieldDef::new("email")
                    .with_rule(ValidationRule::required("Email is required"))
                    .with_rule(ValidationRule::email("Invalid email format")),
            )
            .with_field(
                FieldDef::new("password")
                    .with_rule(ValidationRule::required("Password is required"))
                    .with_rule(ValidationRule::min_length(
                        8,
                        "Password must be at least 8 characters",
                    )),
            )
            .with_field(
                FieldDef::new("confirm_password")
                    .with_rule(ValidationRule::required("Please confirm your password"))
                    .with_rule(ValidationRule::matches("password", "Passwords must match")),
            )
            .with_derived("is_valid", Expr::FormValid)
            .with_effect(Effect::new(
                Trigger::Submit,
                vec![Stmt::set_signal("loading", true)],
            ));

        assert_eq!(form.id, "signup");
        assert_eq!(form.signals.len(), 2);
        assert_eq!(form.fields.len(), 3);
        assert_eq!(form.derived.len(), 1);
        assert_eq!(form.effects.len(), 1);
    }

    #[test]
    fn test_form_ir_serialization() {
        let form = FormIR::new("test").with_signal("count", 0).with_field(
            FieldDef::new("name").with_rule(ValidationRule::required("Name is required")),
        );

        let json = serde_json::to_string_pretty(&form).unwrap();
        assert!(json.contains("\"id\": \"test\""));
        assert!(json.contains("\"count\""));
        assert!(json.contains("\"name\""));
    }
}
