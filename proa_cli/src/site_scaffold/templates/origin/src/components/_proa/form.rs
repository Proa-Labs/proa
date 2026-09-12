// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared form-control state and deterministic accessibility identifiers.

use crate::components::_proa::PartId;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// State axes owned by a form-control root.
///
/// Parts should read these values from their root instead of accepting their
/// own conflicting disabled/invalid/required flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormControlState {
    pub disabled: bool,
    pub invalid: bool,
    pub read_only: bool,
    pub required: bool,
}

impl FormControlState {
    pub const fn enabled() -> Self {
        Self {
            disabled: false,
            invalid: false,
            read_only: false,
            required: false,
        }
    }

    pub const fn data_disabled(self) -> Option<&'static str> {
        if self.disabled {
            Some("true")
        } else {
            None
        }
    }

    pub const fn data_invalid(self) -> Option<&'static str> {
        if self.invalid {
            Some("true")
        } else {
            None
        }
    }

    pub const fn data_read_only(self) -> Option<&'static str> {
        if self.read_only {
            Some("true")
        } else {
            None
        }
    }

    pub const fn data_required(self) -> Option<&'static str> {
        if self.required {
            Some("true")
        } else {
            None
        }
    }

    /// Explicit ARIA tokens are required because generic boolean attributes
    /// use HTML presence semantics, which are not valid for ARIA values.
    pub const fn aria_disabled(self) -> &'static str {
        bool_token(self.disabled)
    }

    pub const fn aria_invalid(self) -> &'static str {
        bool_token(self.invalid)
    }

    pub const fn aria_read_only(self) -> &'static str {
        bool_token(self.read_only)
    }

    pub const fn aria_required(self) -> &'static str {
        bool_token(self.required)
    }
}

const fn bool_token(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

/// Correlated IDs for a labeled form control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormControlIds<'a> {
    root: Option<&'a str>,
}

impl<'a> FormControlIds<'a> {
    pub const fn new(root: Option<&'a str>) -> Self {
        Self { root }
    }

    pub const fn control(self) -> PartId<'a> {
        PartId::new(self.root, "control")
    }

    pub const fn label(self) -> PartId<'a> {
        PartId::new(self.root, "label")
    }

    pub const fn description(self) -> PartId<'a> {
        PartId::new(self.root, "description")
    }

    pub const fn error(self) -> PartId<'a> {
        PartId::new(self.root, "error")
    }

    pub const fn described_by(self, has_description: bool, has_error: bool) -> DescribedBy<'a> {
        DescribedBy {
            root: self.root,
            has_description,
            has_error,
        }
    }
}

/// Allocation-free `aria-describedby` value for optional help and error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescribedBy<'a> {
    root: Option<&'a str>,
    has_description: bool,
    has_error: bool,
}

impl AttrValue for DescribedBy<'_> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        self.root.is_some() && (self.has_description || self.has_error)
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        let Some(root) = self.root else {
            return Ok(());
        };

        if self.has_description {
            write_part_id(cx, root, "description")?;
        }
        if self.has_description && self.has_error {
            cx.out.extend_static(b" ")?;
        }
        if self.has_error {
            write_part_id(cx, root, "error")?;
        }
        Ok(())
    }
}

fn write_part_id<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    root: &str,
    part: &str,
) -> Result<(), WriteError> {
    let dialect = cx.__proa_current_escape_dialect();
    proa_core::shared::escape::escape_attr_value_for_dialect(&mut cx.out, root, dialect)?;
    cx.out.extend_static(b"--")?;
    proa_core::shared::escape::escape_attr_value_for_dialect(&mut cx.out, part, dialect)
}
