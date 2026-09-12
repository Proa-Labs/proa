// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Markdown rendering for the theme toggle component.

use super::ThemeToggle;
use proa_core::md::MdContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// A theme toggle is interactive page chrome with no Markdown equivalent —
/// a document has no theme to switch — so it renders nothing, like other
/// purely presentational controls.
impl<B: WriteBuf> MdRenderSync<B> for ThemeToggle {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
