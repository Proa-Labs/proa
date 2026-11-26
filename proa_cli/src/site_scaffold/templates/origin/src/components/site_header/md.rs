// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa::{MdRenderSync, WebContext, WriteBuf, WriteError};
use proa::md_sync;

/// The header contributes the document identity line to the markdown
/// representation.
pub struct SiteHeaderMd;

impl<Loader: ::proa::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for SiteHeaderMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        md_sync! { r###"ORIGIN — how the internet reaches websites"### }.render_md(cx)?;
        cx.md_block_boundary()
    }
}
