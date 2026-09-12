// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Spinner;
use proa_core::md::MdContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Spinner is visual-only — renders nothing in markdown.
impl<B: WriteBuf> MdRenderSync<B> for Spinner {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
