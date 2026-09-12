// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Reactive attribute types for SSR with progressive enhancement
//!
//! These types represent values that have a static default (rendered at SSR time)
//! but can be reactively updated on the client via JavaScript bindings.

use serde::Serialize;
use std::ops::{BitAnd, BitOr, Not};

/// A binding expression that describes how a value is computed on the client
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Binding {
    /// A literal/static value (no client-side binding)
    Static,
    /// Bound to form state (is_valid, is_submitting, etc.)
    FormState { key: &'static str },
    /// Bound to a field's state (error, touched, dirty, value)
    FieldState {
        field: &'static str,
        key: &'static str,
    },
    /// Logical NOT of another binding
    Not { inner: Box<Binding> },
    /// Logical AND of two bindings
    And {
        left: Box<Binding>,
        right: Box<Binding>,
    },
    /// Logical OR of two bindings
    Or {
        left: Box<Binding>,
        right: Box<Binding>,
    },
    /// Map a binding through a function (for string interpolation, etc.)
    Map {
        inner: Box<Binding>,
        /// JavaScript expression where `$` is the value
        expr: &'static str,
    },
}

impl Binding {
    /// Create a negated version of this binding
    pub fn negated(self) -> Self {
        match self {
            // Double negation elimination
            Binding::Not { inner } => *inner,
            other => Binding::Not {
                inner: Box::new(other),
            },
        }
    }

    /// Create an AND combination
    pub fn and(self, other: Binding) -> Self {
        Binding::And {
            left: Box::new(self),
            right: Box::new(other),
        }
    }

    /// Create an OR combination
    pub fn or(self, other: Binding) -> Self {
        Binding::Or {
            left: Box::new(self),
            right: Box::new(other),
        }
    }

    /// Check if this is a static (non-reactive) binding
    pub fn is_static(&self) -> bool {
        matches!(self, Binding::Static)
    }
}

/// A reactive attribute that has a default value for SSR and an optional
/// client-side binding for reactivity.
///
/// When rendered:
/// - SSR outputs the `default` value
/// - If `binding` is not `Static`, JS is generated to update the attribute
#[derive(Debug, Clone, PartialEq)]
pub struct ReactiveAttr<T> {
    /// The default value rendered at SSR time
    pub default: T,
    /// How this value is computed/updated on the client
    pub binding: Binding,
}

impl<T> ReactiveAttr<T> {
    /// Create a static (non-reactive) attribute
    pub const fn static_value(value: T) -> Self {
        Self {
            default: value,
            binding: Binding::Static,
        }
    }

    /// Create a reactive attribute bound to form state
    pub const fn form_state(default: T, key: &'static str) -> Self {
        Self {
            default,
            binding: Binding::FormState { key },
        }
    }

    /// Create a reactive attribute bound to field state
    pub const fn field_state(default: T, field: &'static str, key: &'static str) -> Self {
        Self {
            default,
            binding: Binding::FieldState { field, key },
        }
    }

    /// Check if this attribute is static (no client-side binding)
    pub fn is_static(&self) -> bool {
        self.binding.is_static()
    }

    /// Map the binding through a JavaScript expression
    pub fn map_binding(self, expr: &'static str) -> Self {
        Self {
            default: self.default,
            binding: Binding::Map {
                inner: Box::new(self.binding),
                expr,
            },
        }
    }
}

impl<T: Default> Default for ReactiveAttr<T> {
    fn default() -> Self {
        Self::static_value(T::default())
    }
}

// Allow conversion from plain values to static ReactiveAttr
impl<T> From<T> for ReactiveAttr<T> {
    fn from(value: T) -> Self {
        Self::static_value(value)
    }
}

// Implement Not for ReactiveAttr<bool>
impl Not for ReactiveAttr<bool> {
    type Output = ReactiveAttr<bool>;

    fn not(self) -> Self::Output {
        ReactiveAttr {
            default: !self.default,
            binding: self.binding.negated(),
        }
    }
}

// Implement BitAnd for ReactiveAttr<bool> (using & operator)
impl BitAnd for ReactiveAttr<bool> {
    type Output = ReactiveAttr<bool>;

    fn bitand(self, rhs: Self) -> Self::Output {
        ReactiveAttr {
            default: self.default && rhs.default,
            binding: match (&self.binding, &rhs.binding) {
                (Binding::Static, Binding::Static) => Binding::Static,
                (Binding::Static, _) if !self.default => Binding::Static, // false && x = false
                (_, Binding::Static) if !rhs.default => Binding::Static,  // x && false = false
                (Binding::Static, _) if self.default => rhs.binding,      // true && x = x
                (_, Binding::Static) if rhs.default => self.binding,      // x && true = x
                _ => self.binding.and(rhs.binding),
            },
        }
    }
}

// Implement BitOr for ReactiveAttr<bool> (using | operator)
impl BitOr for ReactiveAttr<bool> {
    type Output = ReactiveAttr<bool>;

    fn bitor(self, rhs: Self) -> Self::Output {
        ReactiveAttr {
            default: self.default || rhs.default,
            binding: match (&self.binding, &rhs.binding) {
                (Binding::Static, Binding::Static) => Binding::Static,
                (Binding::Static, _) if self.default => Binding::Static, // true || x = true
                (_, Binding::Static) if rhs.default => Binding::Static,  // x || true = true
                (Binding::Static, _) if !self.default => rhs.binding,    // false || x = x
                (_, Binding::Static) if !rhs.default => self.binding,    // x || false = x
                _ => self.binding.or(rhs.binding),
            },
        }
    }
}

/// Reactive string attribute that can change based on a condition
#[derive(Debug, Clone, PartialEq)]
pub struct ReactiveClass {
    /// Classes always applied
    pub base: Option<&'static str>,
    /// Conditional classes: (condition, class_if_true, class_if_false)
    pub conditional: Vec<(ReactiveAttr<bool>, &'static str, &'static str)>,
}

impl ReactiveClass {
    pub fn new() -> Self {
        Self {
            base: None,
            conditional: Vec::new(),
        }
    }

    pub fn base(mut self, class: &'static str) -> Self {
        self.base = Some(class);
        self
    }

    /// Add a conditional class
    pub fn when(
        mut self,
        condition: ReactiveAttr<bool>,
        class_if_true: &'static str,
        class_if_false: &'static str,
    ) -> Self {
        self.conditional
            .push((condition, class_if_true, class_if_false));
        self
    }

    /// Get the default class string for SSR
    pub fn default_class(&self) -> String {
        let mut classes = Vec::new();
        if let Some(base) = self.base {
            classes.push(base);
        }
        for (cond, if_true, if_false) in &self.conditional {
            if cond.default {
                if !if_true.is_empty() {
                    classes.push(if_true);
                }
            } else if !if_false.is_empty() {
                classes.push(if_false);
            }
        }
        classes.join(" ")
    }
}

impl Default for ReactiveClass {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_attr() {
        let attr: ReactiveAttr<bool> = false.into();
        assert!(!attr.default);
        assert!(attr.is_static());
    }

    #[test]
    fn test_not_operator() {
        let is_valid = ReactiveAttr::form_state(false, "isValid");
        let not_valid = !is_valid;

        assert!(not_valid.default); // !false = true
        assert!(matches!(not_valid.binding, Binding::Not { .. }));
    }

    #[test]
    fn test_double_negation() {
        let is_valid = ReactiveAttr::form_state(false, "isValid");
        let double_neg = !!is_valid.clone();

        // Double negation should eliminate
        assert!(matches!(double_neg.binding, Binding::FormState { .. }));
    }

    #[test]
    fn test_and_operator() {
        let is_valid = ReactiveAttr::form_state(true, "isValid");
        let is_dirty = ReactiveAttr::form_state(true, "isDirty");

        let combined = is_valid & is_dirty;
        assert!(combined.default);
        assert!(matches!(combined.binding, Binding::And { .. }));
    }

    #[test]
    fn test_or_operator() {
        let is_submitting = ReactiveAttr::form_state(false, "isSubmitting");
        let not_valid = !ReactiveAttr::form_state(true, "isValid");

        let disabled = is_submitting | not_valid;
        assert!(!disabled.default); // false || false = false
        assert!(matches!(disabled.binding, Binding::Or { .. }));
    }

    #[test]
    fn test_short_circuit_optimizations() {
        // true && x = x
        let always_true: ReactiveAttr<bool> = true.into();
        let is_valid = ReactiveAttr::form_state(true, "isValid");
        let result = always_true & is_valid;
        assert!(matches!(result.binding, Binding::FormState { .. }));

        // false && x = false (static)
        let always_false: ReactiveAttr<bool> = false.into();
        let is_valid = ReactiveAttr::form_state(true, "isValid");
        let result = always_false & is_valid;
        assert!(result.is_static());
    }

    #[test]
    fn test_reactive_class() {
        let is_valid = ReactiveAttr::form_state(false, "isValid");

        let class = ReactiveClass::new().base("btn").when(
            is_valid,
            "bg-blue-600",
            "bg-gray-400 cursor-not-allowed",
        );

        assert_eq!(class.default_class(), "btn bg-gray-400 cursor-not-allowed");
    }
}
