// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Separator;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Separator {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        // `---` directly under a paragraph line would turn that line into a
        // setext heading; a full blank-line boundary avoids it.
        cx.md_block_boundary()?;
        cx.md_hr()?;
        Ok(())
    }
}
