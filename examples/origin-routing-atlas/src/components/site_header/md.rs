// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

/// The header contributes the document identity line to the markdown
/// representation.
pub struct SiteHeaderMd;

impl<B: WriteBuf> MdRenderSync<B> for SiteHeaderMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        md_sync! { r###"ORIGIN — how the internet reaches websites"### }.render_md(cx)?;
        cx.block_boundary()
    }
}
