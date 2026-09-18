// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

use crate::model::RESOLVED;

pub struct LiveWireMd;

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for LiveWireMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        md_sync! { r###"
            ## Recently resolved (replay)

            Illustrative replay items using documentation ranges, not live incidents.

            {for (what, when) in RESOLVED.iter() {
            - {*what} ({*when})
            }}
            "###
        }
        .render_md(cx)?;
        cx.md_block_boundary()
    }
}
