// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared form-binding support used by Free form components.
//!
//! This module is intentionally independent of the mixed crate's legacy
//! `web` builders so a Free source-install payload does not need to copy
//! unrelated or proprietary modules.

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};
use serde::Serialize;
use std::io;

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

/// JSON attribute value rendered directly through Proa's attribute escaping.
pub(crate) struct JsonAttr<'a, T: Serialize + ?Sized>(pub Option<&'a T>);

impl<T: Serialize + ?Sized> AttrValue for JsonAttr<'_, T> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        self.0.is_some()
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        let Some(value) = self.0 else {
            return Ok(());
        };

        let mut writer = JsonAttrWriter { cx, err: None };
        serde_json::to_writer(&mut writer, value).map_err(|_| {
            writer.err.unwrap_or(WriteError::InvalidAttribute(
                "failed to serialize JSON attribute",
            ))
        })
    }
}

struct JsonAttrWriter<'cx, B: WriteBuf, L: DataLoader> {
    cx: &'cx mut WebContext<L, B>,
    err: Option<WriteError>,
}

impl<B: WriteBuf, L: DataLoader> io::Write for JsonAttrWriter<'_, B, L> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s = std::str::from_utf8(buf).map_err(io::Error::other)?;
        let dialect = self.cx.__proa_current_escape_dialect();

        match proa_core::shared::escape::escape_attr_value_for_dialect(&mut self.cx.out, s, dialect)
        {
            Ok(()) => Ok(buf.len()),
            Err(err) => {
                self.err = Some(err);
                Err(io::Error::other("failed to write escaped JSON attribute"))
            }
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
