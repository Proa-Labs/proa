// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::Badge;
use proa_core::WebContext;
use proa_core::{MdCodeContent, MdRenderSync, WriteBuf, WriteError};

impl<Loader: ::proa_core::DataLoader, B: WriteBuf, C: MdCodeContent> MdRenderSync<Loader, B>
    for Badge<C>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.write_md_inline_code(
            self.children
                .as_ref()
                .map_or("", MdCodeContent::md_code_content),
        )
    }
}
