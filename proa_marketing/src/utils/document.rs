// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, MdRenderSync, WebContext, WriteBuf, WriteError};

/// Compose an absolute JSON URL without allocating an intermediate string.
pub struct SiteUrl<'a> {
    pub origin: &'a str,
    pub path: &'a str,
}

impl std::fmt::Display for SiteUrl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.origin)?;
        f.write_str(self.path)
    }
}

impl serde::Serialize for SiteUrl<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Render an owned document for this response; child components stay typed.
pub fn render_markdown(root: impl MdRenderSync) -> String {
    let mut cx = WebContext::new();
    root.render_md(&mut cx)
        .expect("render Markdown into an owned buffer");
    cx.into_output().into_string()
}

/// A link-destination leaf, with URL escaping rather than prose escaping.
/// Page layout and punctuation belong to the surrounding `md_sync!` template.
pub struct Url<'a>(pub &'a str);

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Url<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.write_md_url(self.0)
    }
}
