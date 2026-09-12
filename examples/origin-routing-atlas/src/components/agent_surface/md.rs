// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

pub struct AgentSurfaceMd {
    pub query: String,
    pub host: Option<String>,
}

// proa-perf-ok(scope): the curl commands render as inline code spans via
// typed `cx.code` composition; slots inside backticks stay literal.
impl<B: WriteBuf> MdRenderSync<B> for AgentSurfaceMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        md_sync! { r###"
            ## Fetch this document again
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()?;
        // The commands go through `cx.code` so they arrive as copyable code
        // spans (template slots inside backticks stay literal by design).
        let accept = format!(
            "curl -H 'Accept: text/markdown' {}/?q={}",
            super::agent_surface::base_url(&self.host),
            super::agent_surface::encode_query(&self.query)
        );
        cx.write_prepared_static(b"- ")?;
        cx.code(&accept)?;
        cx.write_prepared_static(b"\n")?;
        cx.block_boundary()
    }
}
