// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Spinner;
use proa::WebContext;
use proa::{MdRenderSync, WriteBuf, WriteError};

/// Spinner is visual-only — renders nothing in markdown.
impl<Loader: ::proa::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Spinner {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa::omit!("Loading");
        Ok(())
    }
}
