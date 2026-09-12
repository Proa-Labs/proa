// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Separator;
use crate::components::_proa::md_support::block_boundary;
use proa_core::md::MdContext;
use proa_core::{MdRenderSync, WriteBuf, WriteError};

impl<B: WriteBuf> MdRenderSync<B> for Separator {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        // `---` directly under a paragraph line would turn that line into a
        // setext heading; a full blank-line boundary avoids it.
        block_boundary(cx)?;
        cx.hr()?;
        Ok(())
    }
}
