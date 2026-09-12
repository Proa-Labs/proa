// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Legacy form handle for SSR with progressive enhancement.
//!
//! New Proa forms should use the `wellformed` crate directly, via
//! `wellformed_macros::form_schema!(mod name = "schema.json", runtime =
//! ::wellformed, client = ::rsjs)`. This module remains only as compatibility
//! surface for older
//! `#[derive(proa_macros::FormSchema)]` users.

use crate::reactive::{Binding, ReactiveAttr};
use crate::validate::{ValidationRule, ValidationTrigger};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;

/// Validation mode - when validation runs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationMode {
    /// Validate on blur (default)
    #[default]
    OnBlur,
    /// Validate on every input change
    OnChange,
    /// Only validate on submit
    OnSubmit,
    /// Validate on blur, then on change after first error
    OnTouched,
}

/// State for a single form field
#[derive(Debug, Clone)]
pub struct FieldState {
    field_name: &'static str,
}

impl FieldState {
    /// Create a new field state accessor
    pub const fn new(field_name: &'static str) -> Self {
        Self { field_name }
    }
}

impl FieldState {
    /// Get the field's current error message (None if valid)
    pub fn error(&self) -> ReactiveAttr<Option<&'static str>> {
        ReactiveAttr {
            default: None,
            binding: Binding::FieldState {
                field: self.field_name,
                key: "error",
            },
        }
    }

    /// Check if the field has an error
    pub fn has_error(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false,
            binding: Binding::FieldState {
                field: self.field_name,
                key: "hasError",
            },
        }
    }

    /// Check if the field has been touched (blurred at least once)
    pub fn touched(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false,
            binding: Binding::FieldState {
                field: self.field_name,
                key: "touched",
            },
        }
    }

    /// Check if the field is dirty (value changed from initial)
    pub fn dirty(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false,
            binding: Binding::FieldState {
                field: self.field_name,
                key: "dirty",
            },
        }
    }

    /// Get the field's current value
    pub fn value(&self) -> ReactiveAttr<String> {
        ReactiveAttr {
            default: String::new(),
            binding: Binding::FieldState {
                field: self.field_name,
                key: "value",
            },
        }
    }
}

/// Form-level state
#[derive(Debug, Clone, Default)]
pub struct FormState {
    _private: (),
}

impl FormState {
    /// Check if the entire form is valid
    pub fn is_valid(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false, // Forms start invalid until validated
            binding: Binding::FormState { key: "isValid" },
        }
    }

    /// Check if the form is currently submitting
    pub fn is_submitting(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false,
            binding: Binding::FormState {
                key: "isSubmitting",
            },
        }
    }

    /// Check if any field has been modified
    pub fn is_dirty(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false,
            binding: Binding::FormState { key: "isDirty" },
        }
    }

    /// Check if any field has been touched
    pub fn is_touched(&self) -> ReactiveAttr<bool> {
        ReactiveAttr {
            default: false,
            binding: Binding::FormState { key: "isTouched" },
        }
    }

    /// Get the number of times the form has been submitted
    pub fn submit_count(&self) -> ReactiveAttr<u32> {
        ReactiveAttr {
            default: 0,
            binding: Binding::FormState { key: "submitCount" },
        }
    }
}

/// Registration result - the props to spread onto an input
#[derive(Debug, Clone)]
pub struct FieldRegistration {
    /// Field name for the input
    pub name: &'static str,
    /// Data attribute for form binding
    pub data_field: &'static str,
    /// Validation rules for this field
    pub rules: Vec<ValidationRule>,
    /// When to validate
    pub validate_on: ValidationTrigger,
}

impl FieldRegistration {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            data_field: name,
            rules: Vec::new(),
            validate_on: ValidationTrigger::default(),
        }
    }

    pub fn with_rules(mut self, rules: Vec<ValidationRule>) -> Self {
        self.rules = rules;
        self
    }

    pub fn validate_on(mut self, trigger: ValidationTrigger) -> Self {
        self.validate_on = trigger;
        self
    }
}

/// Collected field metadata for script generation
#[derive(Debug, Clone, Serialize)]
pub struct FieldMeta {
    pub name: &'static str,
    pub rules: Vec<ValidationRule>,
    pub validate_on: ValidationTrigger,
}

/// Collected binding metadata for script generation
#[derive(Debug, Clone, Serialize)]
pub struct BindingMeta {
    pub selector: String,
    pub attribute: String,
    pub binding: Binding,
}

/// Handle for interacting with a form - like useForm() in React Hook Form
///
/// Created by calling `T::use_form()` on a type that derives `FormSchema`.
pub struct FormHandle<T> {
    /// The form ID
    pub id: &'static str,
    /// Validation mode
    pub mode: ValidationMode,
    /// Phantom data for the schema type
    _schema: std::marker::PhantomData<T>,
    /// Collected field metadata (populated during render)
    fields: RefCell<HashMap<&'static str, FieldMeta>>,
    /// Collected bindings (populated during render)
    bindings: RefCell<Vec<BindingMeta>>,
}

impl<T> FormHandle<T> {
    /// Create a new form handle
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            mode: ValidationMode::default(),
            _schema: std::marker::PhantomData,
            fields: RefCell::new(HashMap::new()),
            bindings: RefCell::new(Vec::new()),
        }
    }

    /// Set the validation mode
    pub fn mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Get form-level state (is_valid, is_submitting, etc.)
    pub fn form_state(&self) -> FormState {
        FormState::default()
    }

    /// Get attributes to spread on the form element
    pub fn attrs(&self) -> FormAttrs {
        FormAttrs { id: self.id }
    }

    /// Register a binding for an element
    pub fn register_binding(&self, selector: String, attribute: String, binding: Binding) {
        if !binding.is_static() {
            self.bindings.borrow_mut().push(BindingMeta {
                selector,
                attribute,
                binding,
            });
        }
    }

    /// Get collected field metadata
    pub fn field_meta(&self) -> HashMap<&'static str, FieldMeta> {
        self.fields.borrow().clone()
    }

    /// Get collected bindings
    pub fn binding_meta(&self) -> Vec<BindingMeta> {
        self.bindings.borrow().clone()
    }
}

/// Attributes to spread on the form element
#[derive(Debug, Clone)]
pub struct FormAttrs {
    pub id: &'static str,
}

/// Marker trait for types that can be used as form field accessors
pub trait FormFields {
    /// Get the field name from a closure that accesses a field
    fn field_name<F, R>(f: F) -> &'static str
    where
        F: FnOnce(&Self) -> R;
}

/// Trait implemented by the deprecated `proa_macros::FormSchema` derive macro.
#[deprecated(
    since = "0.1.2",
    note = "use wellformed_macros::form_schema! with a Wellformed schema"
)]
pub trait FormSchema: Sized + FormFields {
    /// Create a form handle for this schema
    fn use_form() -> FormHandle<Self>;

    /// Get field metadata (rules, etc.) for a field
    fn field_rules(name: &'static str) -> Vec<ValidationRule>;
}

/// Extension methods for FormHandle that require FormSchema
#[allow(deprecated)]
impl<T: FormSchema> FormHandle<T> {
    /// Register a legacy field and get its props.
    pub fn register<F, R>(&self, accessor: F) -> FieldRegistration
    where
        F: FnOnce(&T) -> R,
    {
        let name = T::field_name(accessor);
        let rules = T::field_rules(name);

        // Store field metadata
        self.fields.borrow_mut().insert(
            name,
            FieldMeta {
                name,
                rules: rules.clone(),
                validate_on: match self.mode {
                    ValidationMode::OnBlur => ValidationTrigger::Blur,
                    ValidationMode::OnChange => ValidationTrigger::Input,
                    ValidationMode::OnSubmit => ValidationTrigger::Submit,
                    ValidationMode::OnTouched => ValidationTrigger::Blur,
                },
            },
        );

        FieldRegistration::new(name).with_rules(rules)
    }

    /// Get state for a specific field
    ///
    /// Usage: `form.field(|f| f.email).error()`
    pub fn field<F, R>(&self, accessor: F) -> FieldState
    where
        F: FnOnce(&T) -> R,
    {
        let name = T::field_name(accessor);
        FieldState { field_name: name }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_state_is_valid() {
        let state = FormState::default();
        let is_valid = state.is_valid();

        assert!(!is_valid.default);
        assert!(matches!(
            is_valid.binding,
            Binding::FormState { key: "isValid" }
        ));
    }

    #[test]
    fn test_form_state_not_valid() {
        let state = FormState::default();
        let not_valid = !state.is_valid();

        assert!(not_valid.default); // !false = true
        assert!(matches!(not_valid.binding, Binding::Not { .. }));
    }

    #[test]
    fn test_field_state() {
        let field = FieldState {
            field_name: "email",
        };

        let error = field.error();
        assert!(error.default.is_none());
        assert!(matches!(
            error.binding,
            Binding::FieldState {
                field: "email",
                key: "error"
            }
        ));
    }

    #[test]
    fn test_combined_disabled_state() {
        let state = FormState::default();

        // disabled = is_submitting | !is_valid
        let disabled = state.is_submitting() | !state.is_valid();

        assert!(disabled.default); // false || true = true
        assert!(matches!(disabled.binding, Binding::Or { .. }));
    }
}
