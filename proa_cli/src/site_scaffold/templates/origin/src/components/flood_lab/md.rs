// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct FloodLabMd;

impl<B: WriteBuf> MdRenderSync<B> for FloodLabMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        md_sync! { r###"
            ## Render lab

            The HTML page can flood its table with up to 100,000 synthetic rows. The browser's build and layout times are measured on the client; `GET /lab/rows?n=<count>` renders the same rows on the server and reports its measured time.
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()
    }
}
