// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical state values shared by server markup and client behavior.
//!
//! Interactive components expose state through stable `data-state` values.
//! Keeping those values in one place prevents the static renderer, delegated
//! runtime, and rsjs islands from drifting apart.

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

/// Open/closed state for disclosures and overlays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisclosureState {
    Open,
    #[default]
    Closed,
}

impl DisclosureState {
    pub const fn from_open(open: bool) -> Self {
        if open {
            Self::Open
        } else {
            Self::Closed
        }
    }

    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    pub const fn aria_expanded(self) -> bool {
        self.is_open()
    }

    /// WAI-ARIA token for `aria-expanded`.
    ///
    /// Use this instead of rendering a generic `bool` into an ARIA attribute;
    /// boolean HTML attributes and ARIA token attributes serialize differently.
    pub const fn aria_expanded_str(self) -> &'static str {
        match self {
            Self::Open => "true",
            Self::Closed => "false",
        }
    }

    /// WAI-ARIA token for `aria-hidden`.
    pub const fn aria_hidden_str(self) -> &'static str {
        match self {
            Self::Open => "false",
            Self::Closed => "true",
        }
    }

    pub const fn hidden(self) -> bool {
        !self.is_open()
    }
}

impl AttrValue for DisclosureState {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.as_str().as_bytes())
    }
}

/// Selected/unselected state for collection items and tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionState {
    Selected,
    #[default]
    Unselected,
}

impl SelectionState {
    pub const fn from_selected(selected: bool) -> Self {
        if selected {
            Self::Selected
        } else {
            Self::Unselected
        }
    }

    pub const fn is_selected(self) -> bool {
        matches!(self, Self::Selected)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Unselected => "unselected",
        }
    }

    /// WAI-ARIA token used by selectable collection items.
    pub const fn aria_selected(self) -> &'static str {
        match self {
            Self::Selected => "true",
            Self::Unselected => "false",
        }
    }
}

impl AttrValue for SelectionState {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.as_str().as_bytes())
    }
}

/// Two- or three-state value for checkable controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckedState {
    Checked,
    Indeterminate,
    #[default]
    Unchecked,
}

impl CheckedState {
    pub const fn from_checked(checked: bool) -> Self {
        if checked {
            Self::Checked
        } else {
            Self::Unchecked
        }
    }

    pub const fn is_checked(self) -> bool {
        matches!(self, Self::Checked)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Checked => "checked",
            Self::Indeterminate => "indeterminate",
            Self::Unchecked => "unchecked",
        }
    }

    /// WAI-ARIA token used by checkbox-like controls.
    pub const fn aria_checked(self) -> &'static str {
        match self {
            Self::Checked => "true",
            Self::Indeterminate => "mixed",
            Self::Unchecked => "false",
        }
    }

    /// Compatibility projection used by the original Checkbox API.
    pub const fn as_data_state(&self) -> &'static str {
        (*self).as_str()
    }

    /// Compatibility projection used by the original Checkbox API.
    pub const fn as_aria_checked(&self) -> &'static str {
        (*self).aria_checked()
    }
}

impl AttrValue for CheckedState {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.as_str().as_bytes())
    }
}
