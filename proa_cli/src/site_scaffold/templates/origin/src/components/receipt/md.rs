// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct ReceiptMd;

impl<B: WriteBuf> MdRenderSync<B> for ReceiptMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        md_sync! { r###"
            ---

            Rendered by Proa · https://proa.so/docs
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()
    }
}
