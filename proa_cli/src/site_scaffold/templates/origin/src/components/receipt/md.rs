// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa::{MdRenderSync, WebContext, WriteBuf, WriteError};
use proa::md_sync;

pub struct ReceiptMd;

impl<Loader: ::proa::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for ReceiptMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        md_sync! { r###"
            ---

            Rendered by Proa · https://proa.so/docs
            "###
        }
        .render_md(cx)?;
        cx.md_block_boundary()
    }
}
