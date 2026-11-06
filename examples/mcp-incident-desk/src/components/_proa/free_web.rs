// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared form-binding support used by Free form components.
//!
//! This module is intentionally independent of the mixed crate's legacy
//! `web` builders so a Free source-install payload does not need to copy
//! unrelated or proprietary modules.

/// Convert a [`proa_ir::Binding`] to a JavaScript expression string.
///
/// Used by form-related components to emit `data-bind-*` attributes that
/// the client-side runtime evaluates at hydration time.
pub(crate) fn binding_to_js(binding: &proa_ir::Binding) -> String {
    match binding {
        proa_ir::Binding::Static => String::new(),
        proa_ir::Binding::FormState { key } => format!("formState.{}", key),
        proa_ir::Binding::FieldState { field, key } => format!("fields.{}.{}", field, key),
        proa_ir::Binding::Not { inner } => format!("!({})", binding_to_js(inner)),
        proa_ir::Binding::And { left, right } => {
            format!("({}) && ({})", binding_to_js(left), binding_to_js(right))
        }
        proa_ir::Binding::Or { left, right } => {
            format!("({}) || ({})", binding_to_js(left), binding_to_js(right))
        }
        proa_ir::Binding::Map { inner, expr } => {
            let inner_js = binding_to_js(inner);
            expr.replace('$', &inner_js)
        }
    }
}

/// Convert a non-static binding into an attribute value.
pub(crate) fn binding_to_js_attr(binding: &proa_ir::Binding) -> Option<String> {
    if binding.is_static() {
        None
    } else {
        Some(binding_to_js(binding))
    }
}
