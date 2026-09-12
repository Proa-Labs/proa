// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Form builder for ergonomic IR construction
//!
//! This provides the main user-facing API for building forms.

use crate::builder::ExprBuilder;
use crate::expr::Expr;
use crate::form::{DerivedDef, FieldDef, FormIR, SignalDef};
use crate::stmt::{Block, FetchOptions, Stmt};
use crate::trigger::{Effect, Trigger};
use crate::types::{Method, Selector, ToastLevel};
use crate::validate::{InputFormat, Transform, ValidationRule, ValidationTrigger};
use crate::value::Value;

/// Builder for constructing a form's client-side behavior
pub struct FormBuilder {
    ir: FormIR,
}

impl FormBuilder {
    /// Create a new form builder
    pub fn new(id: &'static str) -> Self {
        Self {
            ir: FormIR::new(id),
        }
    }

    // =========================================================================
    // Signals
    // =========================================================================

    /// Add a signal (reactive state)
    pub fn signal(mut self, name: &'static str, initial: impl Into<Value>) -> Self {
        self.ir.signals.push(SignalDef::new(name, initial));
        self
    }

    /// Add a persisted signal (saved to localStorage)
    pub fn signal_persisted(
        mut self,
        name: &'static str,
        initial: impl Into<Value>,
        storage_key: &'static str,
    ) -> Self {
        self.ir
            .signals
            .push(SignalDef::persisted(name, initial, storage_key));
        self
    }

    // =========================================================================
    // Fields
    // =========================================================================

    /// Add a field with configuration
    pub fn field(
        mut self,
        name: &'static str,
        config: impl FnOnce(FieldBuilder) -> FieldBuilder,
    ) -> Self {
        let builder = config(FieldBuilder::new(name));
        self.ir.fields.push(builder.build());
        self
    }

    /// Add a simple field with just validation rules
    pub fn field_with_rules(mut self, name: &'static str, rules: Vec<ValidationRule>) -> Self {
        self.ir.fields.push(FieldDef::new(name).with_rules(rules));
        self
    }

    // =========================================================================
    // Derived State
    // =========================================================================

    /// Add a derived/computed value
    pub fn derived(mut self, name: &'static str, expr: impl Into<Expr>) -> Self {
        self.ir.derived.push(DerivedDef::new(name, expr.into()));
        self
    }

    /// Add a derived value using the builder
    pub fn derived_from(
        mut self,
        name: &'static str,
        builder: impl FnOnce() -> ExprBuilder,
    ) -> Self {
        self.ir
            .derived
            .push(DerivedDef::new(name, builder().build()));
        self
    }

    // =========================================================================
    // Effects
    // =========================================================================

    /// Add an effect (event handler)
    pub fn effect(mut self, trigger: Trigger, body: Block) -> Self {
        self.ir.effects.push(Effect::new(trigger, body));
        self
    }

    /// Add a conditional effect
    pub fn effect_when(
        mut self,
        trigger: Trigger,
        condition: impl Into<Expr>,
        body: Block,
    ) -> Self {
        self.ir
            .effects
            .push(Effect::new(trigger, body).with_condition(condition.into()));
        self
    }

    /// Handle field input
    pub fn on_input(self, field: &'static str, body: Block) -> Self {
        self.effect(Trigger::on_input(field), body)
    }

    /// Handle field change
    pub fn on_change(self, field: &'static str, body: Block) -> Self {
        self.effect(Trigger::on_change(field), body)
    }

    /// Handle field blur
    pub fn on_blur(self, field: &'static str, body: Block) -> Self {
        self.effect(Trigger::on_blur(field), body)
    }

    /// Handle element click
    pub fn on_click(self, selector: &'static str, body: Block) -> Self {
        self.effect(Trigger::on_click(selector), body)
    }

    /// Handle form submission
    pub fn on_submit(mut self, body: Block) -> Self {
        self.ir.options.on_submit = Some(body);
        self
    }

    /// Handle signal change
    pub fn on_signal_change(self, signal: &'static str, body: Block) -> Self {
        self.effect(Trigger::on_signal(signal), body)
    }

    /// Handle mount
    pub fn on_mount(self, body: Block) -> Self {
        self.effect(Trigger::Mount, body)
    }

    // =========================================================================
    // Form Options
    // =========================================================================

    /// Set the form action URL
    pub fn action(mut self, url: &'static str) -> Self {
        self.ir.options.action = Some(url);
        self
    }

    /// Set the form method
    pub fn method(mut self, method: Method) -> Self {
        self.ir.options.method = Some(method);
        self
    }

    /// Disable submit button when form is invalid
    pub fn disable_submit_when_invalid(mut self) -> Self {
        self.ir.options.disable_submit_when_invalid = true;
        self
    }

    /// Reset form on successful submission
    pub fn reset_on_success(mut self) -> Self {
        self.ir.options.reset_on_success = true;
        self
    }

    /// Show error summary
    pub fn show_error_summary(mut self, selector: &'static str) -> Self {
        self.ir.options.show_error_summary = true;
        self.ir.options.error_summary_selector = Some(selector);
        self
    }

    /// Set success handler
    pub fn on_success(mut self, body: Block) -> Self {
        self.ir.options.on_success = Some(body);
        self
    }

    /// Set error handler
    pub fn on_error(mut self, body: Block) -> Self {
        self.ir.options.on_error = Some(body);
        self
    }

    // =========================================================================
    // Build
    // =========================================================================

    /// Build the FormIR
    pub fn build(self) -> FormIR {
        self.ir
    }
}

/// Builder for configuring a form field
pub struct FieldBuilder {
    def: FieldDef,
}

impl FieldBuilder {
    /// Create a new field builder
    pub fn new(name: &'static str) -> Self {
        Self {
            def: FieldDef::new(name),
        }
    }

    /// Set initial value
    pub fn initial(mut self, value: impl Into<Value>) -> Self {
        self.def.initial = Some(value.into());
        self
    }

    /// Add a validation rule
    pub fn validate(mut self, rule: ValidationRule) -> Self {
        self.def.rules.push(rule);
        self
    }

    /// Add required validation
    pub fn required(self, message: &'static str) -> Self {
        self.validate(ValidationRule::required(message))
    }

    /// Add email validation
    pub fn email(self, message: &'static str) -> Self {
        self.validate(ValidationRule::email(message))
    }

    /// Add min length validation
    pub fn min_length(self, min: usize, message: &'static str) -> Self {
        self.validate(ValidationRule::min_length(min, message))
    }

    /// Add max length validation
    pub fn max_length(self, max: usize, message: &'static str) -> Self {
        self.validate(ValidationRule::max_length(max, message))
    }

    /// Add pattern validation
    pub fn pattern(self, pattern: &'static str, message: &'static str) -> Self {
        self.validate(ValidationRule::pattern(pattern, message))
    }

    /// Add matches (cross-field) validation
    pub fn matches(self, other_field: &'static str, message: &'static str) -> Self {
        self.validate(ValidationRule::matches(other_field, message))
    }

    /// Add async validation
    pub fn validate_async(
        self,
        endpoint: &'static str,
        debounce_ms: u32,
        message: &'static str,
    ) -> Self {
        self.validate(ValidationRule::async_validate(
            endpoint,
            debounce_ms,
            message,
        ))
    }

    /// Add custom validation expression
    pub fn validate_custom(self, expr: impl Into<Expr>, message: &'static str) -> Self {
        self.validate(ValidationRule::custom(expr.into(), message))
    }

    /// Set validation trigger
    pub fn validate_on(mut self, trigger: ValidationTrigger) -> Self {
        self.def.validate_on = trigger;
        self
    }

    /// Add a transform
    pub fn transform(mut self, transform: Transform) -> Self {
        self.def.transforms.push(transform);
        self
    }

    /// Trim whitespace before validation
    pub fn trim(self) -> Self {
        self.transform(Transform::Trim)
    }

    /// Convert to lowercase before validation
    pub fn lowercase(self) -> Self {
        self.transform(Transform::Lowercase)
    }

    /// Set input format mask
    pub fn format(mut self, format: InputFormat) -> Self {
        self.def.format = Some(format);
        self
    }

    /// Format as phone number: (123) 456-7890
    pub fn phone(self) -> Self {
        self.format(InputFormat::Phone)
    }

    /// Format as credit card: 1234 5678 9012 3456
    pub fn credit_card(self) -> Self {
        self.format(InputFormat::CreditCard)
    }

    /// Format as expiry date: MM/YY
    pub fn expiry(self) -> Self {
        self.format(InputFormat::Date { pattern: "MM/YY" })
    }

    /// Format as CVC: 3-4 digits only
    pub fn cvc(self) -> Self {
        self.format(InputFormat::Cvc)
    }

    /// Format as name: letters, spaces, hyphens, apostrophes only (max 50 chars)
    pub fn name(self) -> Self {
        self.format(InputFormat::Name)
    }

    /// Disable field when condition is true
    pub fn disabled_when(mut self, condition: impl Into<Expr>) -> Self {
        self.def.disabled = Some(condition.into());
        self
    }

    /// Make field readonly when condition is true
    pub fn readonly_when(mut self, condition: impl Into<Expr>) -> Self {
        self.def.readonly = Some(condition.into());
        self
    }

    /// Build the field definition
    pub fn build(self) -> FieldDef {
        self.def
    }
}

/// Helper for building statement blocks
pub struct BlockBuilder {
    stmts: Block,
}

impl BlockBuilder {
    pub fn new() -> Self {
        Self { stmts: Vec::new() }
    }

    /// Set a signal value
    pub fn set(mut self, signal: &'static str, value: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::set_signal(signal, value.into()));
        self
    }

    /// Set a field value
    pub fn set_field(mut self, field: &'static str, value: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::set_field(field, value.into()));
        self
    }

    /// Set a field error
    pub fn set_error(mut self, field: &'static str, error: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::set_error(field, error.into()));
        self
    }

    /// Conditional execution
    pub fn if_then(mut self, cond: impl Into<Expr>, then: Block) -> Self {
        self.stmts.push(Stmt::if_then(cond.into(), then));
        self
    }

    /// Conditional with else
    pub fn if_then_else(mut self, cond: impl Into<Expr>, then: Block, else_: Block) -> Self {
        self.stmts
            .push(Stmt::if_then_else(cond.into(), then, else_));
        self
    }

    /// Show element
    pub fn show(mut self, selector: &'static str) -> Self {
        self.stmts.push(Stmt::show(selector));
        self
    }

    /// Hide element
    pub fn hide(mut self, selector: &'static str) -> Self {
        self.stmts.push(Stmt::hide(selector));
        self
    }

    /// Toast notification
    pub fn toast(mut self, message: impl Into<Expr>, level: ToastLevel) -> Self {
        self.stmts.push(Stmt::toast(message.into(), level));
        self
    }

    /// Redirect to URL
    pub fn redirect(mut self, url: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::redirect(url.into()));
        self
    }

    /// Fetch data
    pub fn fetch(
        mut self,
        url: impl Into<Expr>,
        options: FetchOptions,
        on_success: Block,
        on_error: Block,
    ) -> Self {
        self.stmts.push(Stmt::Fetch {
            url: url.into(),
            options,
            on_success,
            on_error,
        });
        self
    }

    /// Debounced execution
    pub fn debounce(mut self, key: &'static str, ms: u32, body: Block) -> Self {
        self.stmts.push(Stmt::Debounce { key, ms, body });
        self
    }

    /// Focus element
    pub fn focus(mut self, selector: &'static str) -> Self {
        self.stmts.push(Stmt::Focus {
            selector: Selector::new(selector),
        });
        self
    }

    /// Add class to element
    pub fn add_class(mut self, selector: &'static str, class: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::AddClass {
            selector: Selector::new(selector),
            class: class.into(),
        });
        self
    }

    /// Remove class from element
    pub fn remove_class(mut self, selector: &'static str, class: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::RemoveClass {
            selector: Selector::new(selector),
            class: class.into(),
        });
        self
    }

    /// Set element text
    pub fn set_text(mut self, selector: &'static str, value: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::SetText {
            selector: Selector::new(selector),
            value: value.into(),
        });
        self
    }

    /// Log to console
    pub fn log(mut self, value: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::log(value.into()));
        self
    }

    /// Submit the form
    pub fn submit(mut self) -> Self {
        self.stmts.push(Stmt::SubmitForm);
        self
    }

    /// Reset the form
    pub fn reset(mut self) -> Self {
        self.stmts.push(Stmt::ResetForm);
        self
    }

    /// Prevent default event behavior
    pub fn prevent_default(mut self) -> Self {
        self.stmts.push(Stmt::PreventDefault);
        self
    }

    /// Copy to clipboard
    pub fn copy_to_clipboard(mut self, value: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::CopyToClipboard {
            value: value.into(),
        });
        self
    }

    /// Set localStorage
    pub fn set_local_storage(mut self, key: &'static str, value: impl Into<Expr>) -> Self {
        self.stmts.push(Stmt::SetLocalStorage {
            key,
            value: value.into(),
        });
        self
    }

    /// Add a raw statement
    pub fn stmt(mut self, stmt: Stmt) -> Self {
        self.stmts.push(stmt);
        self
    }

    /// Build the block
    pub fn build(self) -> Block {
        self.stmts
    }
}

impl Default for BlockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function for creating blocks
pub fn block() -> BlockBuilder {
    BlockBuilder::new()
}

/// Convenience function for creating forms
pub fn form(id: &'static str) -> FormBuilder {
    FormBuilder::new(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_builder() {
        let form = form("signup")
            .signal("loading", false)
            .signal("error", Value::Null)
            .field("email", |f| {
                f.required("Email is required")
                    .email("Invalid email")
                    .trim()
            })
            .field("password", |f| {
                f.required("Password is required")
                    .min_length(8, "At least 8 characters")
            })
            .field("confirm", |f| {
                f.required("Please confirm password")
                    .matches("password", "Passwords must match")
            })
            .derived_from("is_valid", ExprBuilder::form_valid)
            .on_submit(block().set("loading", true).build())
            .disable_submit_when_invalid()
            .build();

        assert_eq!(form.id, "signup");
        assert_eq!(form.signals.len(), 2);
        assert_eq!(form.fields.len(), 3);
        assert_eq!(form.derived.len(), 1);
        assert!(form.options.disable_submit_when_invalid);
    }

    #[test]
    fn test_field_builder() {
        let field = FieldBuilder::new("username")
            .required("Username is required")
            .min_length(3, "At least 3 characters")
            .max_length(20, "At most 20 characters")
            .pattern(
                "^[a-z0-9_]+$",
                "Only lowercase letters, numbers, and underscores",
            )
            .validate_async("/api/check-username", 300, "Username is taken")
            .trim()
            .lowercase()
            .build();

        assert_eq!(field.id.0, "username");
        assert_eq!(field.rules.len(), 5);
        assert_eq!(field.transforms.len(), 2);
    }

    #[test]
    fn test_block_builder() {
        let body = block()
            .set("loading", true)
            .hide("#error")
            .show("#spinner")
            .build();

        assert_eq!(body.len(), 3);
    }
}
