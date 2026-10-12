// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Separator;
use crate::components::_proa::md_support::block_boundary;
use proa_core::WebContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for Separator {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        // `---` directly under a paragraph line would turn that line into a
        // setext heading; a full blank-line boundary avoids it.
        proa_macros::content_part!("layout-boundary", block_boundary(cx))?;
        cx.md_hr()?;
        Ok(())
    }
}
