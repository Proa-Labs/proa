// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared overlay state and correlated accessibility identifiers.
//!
//! Dialogs, drawers, sheets, popovers, menus, hover cards, and floating
//! panels all need the same initial-state and ID projections. This module is
//! intentionally renderer-agnostic so the delegated runtime and rsjs islands
//! can consume the exact same contract.

use crate::components::{DisclosureState, PartId};

/// Root-owned state for an overlay surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayState {
    disclosure: DisclosureState,
    modal: bool,
}

impl OverlayState {
    pub const fn new(open: bool, modal: bool) -> Self {
        Self {
            disclosure: DisclosureState::from_open(open),
            modal,
        }
    }

    pub const fn disclosure(self) -> DisclosureState {
        self.disclosure
    }

    pub const fn is_open(self) -> bool {
        self.disclosure.is_open()
    }

    pub const fn is_modal(self) -> bool {
        self.modal
    }

    pub const fn data_state(self) -> &'static str {
        self.disclosure.as_str()
    }

    pub const fn aria_expanded(self) -> bool {
        self.disclosure.aria_expanded()
    }

    pub const fn aria_expanded_str(self) -> &'static str {
        self.disclosure.aria_expanded_str()
    }

    pub const fn aria_hidden_str(self) -> &'static str {
        self.disclosure.aria_hidden_str()
    }

    pub const fn hidden(self) -> bool {
        self.disclosure.hidden()
    }
}

/// Stable IDs shared by an overlay trigger and its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayIds<'a> {
    root: Option<&'a str>,
}

impl<'a> OverlayIds<'a> {
    pub const fn new(root: Option<&'a str>) -> Self {
        Self { root }
    }

    pub const fn trigger(self) -> PartId<'a> {
        PartId::new(self.root, "trigger")
    }

    pub const fn positioner(self) -> PartId<'a> {
        PartId::new(self.root, "positioner")
    }

    pub const fn content(self) -> PartId<'a> {
        PartId::new(self.root, "content")
    }

    pub const fn title(self) -> PartId<'a> {
        PartId::new(self.root, "title")
    }

    pub const fn description(self) -> PartId<'a> {
        PartId::new(self.root, "description")
    }
}
