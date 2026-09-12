// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

use crate::model::RESOLVED;

pub struct LiveWireMd;

impl<B: WriteBuf> MdRenderSync<B> for LiveWireMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        md_sync! { r###"
            ## Recently resolved (replay)

            Illustrative replay items using documentation ranges, not live incidents.

            {for (what, when) in RESOLVED.iter() {
            - {*what} ({*when})
            }}
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()
    }
}
