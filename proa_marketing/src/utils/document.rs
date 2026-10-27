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

/// The owned response/cache boundary; child components stay typed.
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

#[cfg(test)]
mod tests {
    use super::*;
    use proa_macros::md_sync;

    #[test]
    fn markdown_slots_escape_labels_and_link_destinations() {
        let label = "[injected](https://evil.test)";
        let url = "https://example.test/a (b)";
        let output = render_markdown(md_sync! { r###"[{label}]({Url(url)})"### });
        assert!(output.contains(r"\[injected\]"));
        assert!(output.contains(r"https://example.test/a%20\(b\)"));
        assert!(!output.contains("](https://evil.test)"));
        assert_eq!(render_markdown(Url("javascript:alert(1)")), "");
    }
}
