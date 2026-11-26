// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Markdown rendering for the textarea component.

use super::Textarea;
use proa::md_sync;
use proa::WebContext;
use proa::{MdRenderSync, WriteBuf, WriteError};

/// Textarea renders its value — as a fenced code block when multiline,
/// as plain text otherwise. With no value, the placeholder is shown in
/// brackets (matching Input's convention).
impl<Loader: ::proa::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Textarea {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        if let Some(value) = self.value {
            if value
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, b'\r' | b'\n'))
            {
                cx.md_fenced_code("", value)?;
            } else {
                cx.write_md_text(value)?;
            }
        } else if let Some(placeholder) = self.placeholder {
            md_sync! { "[{placeholder}]" }.render_md(cx)?;
        }
        Ok(())
    }
}
