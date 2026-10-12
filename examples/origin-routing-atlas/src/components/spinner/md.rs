// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Spinner;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

/// Spinner is visual-only — renders nothing in markdown.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Spinner {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!("Loading");
        Ok(())
    }
}
