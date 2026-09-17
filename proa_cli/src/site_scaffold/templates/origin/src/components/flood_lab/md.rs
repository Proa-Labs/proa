// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct FloodLabMd;

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for FloodLabMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        md_sync! { r###"
            ## Render lab

            The HTML page can flood its table with up to 100,000 synthetic rows. The browser's build and layout times are measured on the client; `GET /lab/rows?n=<count>` renders the same rows on the server and reports its measured time.
            "###
        }
        .render_md(cx)?;
        cx.md_block_boundary()
    }
}
