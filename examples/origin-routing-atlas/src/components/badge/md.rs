// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Badge;
use proa_core::md::MdContext;
use proa_core::{MdCodeContent, MdRenderSync, WriteBuf, WriteError};

impl<B: WriteBuf, C: MdCodeContent> MdRenderSync<B> for Badge<C> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_inline_code(
            self.children
                .as_ref()
                .map_or("", MdCodeContent::md_code_content),
        )
    }
}
