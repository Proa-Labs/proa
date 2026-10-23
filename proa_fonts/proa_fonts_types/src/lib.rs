// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared font declarations, independent of rendering and token parsing.

pub mod request;

/// Style axis of a single `@font-face`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
}

impl FontStyle {
    /// The CSS `font-style` value.
    #[inline]
    pub const fn css(self) -> &'static str {
        match self {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
        }
    }
}
