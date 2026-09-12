// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct HeroMd;

impl<B: WriteBuf> MdRenderSync<B> for HeroMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        md_sync! { r###"
            See how the internet reaches any website: trace a domain, IP, prefix, or ASN with `GET /?q=<resource>`.
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()
    }
}
