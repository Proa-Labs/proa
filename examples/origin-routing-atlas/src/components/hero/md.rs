// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct HeroMd;

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for HeroMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        md_sync! { "See how the internet reaches any website: trace a domain, IP, prefix, or ASN with `GET /?q=<resource>`.\n" }
        .render_md(cx)?;
        cx.md_block_boundary()
    }
}
