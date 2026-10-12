// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Input;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

/// Input renders its placeholder text in brackets — inputs are interactive,
/// so markdown shows a text indicator.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Input {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        proa_macros::omit!(
            self.value,
            "Markdown uses the placeholder as the form text indicator."
        );
        proa_macros::omit!(
            self.aria_label,
            "Screen-reader labels are HTML control metadata."
        );
        if let Some(placeholder) = self.placeholder {
            md_sync! { "[" { placeholder } "]" }.render_md(cx)?;
        }
        Ok(())
    }
}
