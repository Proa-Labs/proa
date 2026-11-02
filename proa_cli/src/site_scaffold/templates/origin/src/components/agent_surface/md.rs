// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct AgentSurfaceMd {
    pub query: String,
    pub host: Option<String>,
}

// proa-perf-ok(scope): the curl commands render as inline code spans via
// typed `cx.code` composition; slots inside backticks stay literal.
impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for AgentSurfaceMd {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.md_block_boundary()?;
        md_sync! { "## Fetch this document again\n" }.render_md(cx)?;
        cx.md_block_boundary()?;
        // The commands go through `cx.code` so they arrive as copyable code
        // spans (template slots inside backticks stay literal by design).
        let accept = format!(
            "curl -H 'Accept: text/markdown' {}{}?q={}",
            super::agent_surface::base_url(&self.host),
            cx.url("/"),
            super::agent_surface::encode_query(&self.query)
        );
        cx.write_md_prepared_static(b"- ")?;
        cx.md_code(&accept)?;
        cx.write_md_prepared_static(b"\n")?;
        cx.md_block_boundary()
    }
}
