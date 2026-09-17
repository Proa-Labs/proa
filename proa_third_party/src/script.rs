// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    csp_nonce_attr, DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;

/// When an external script may start downloading. These are browser lifecycle
/// stages, independent of whether the page contains any hydrated islands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScriptStrategy {
    /// Start downloading immediately, without blocking the HTML parser.
    Async,
    /// Wait for window.load so scripts do not compete with initial resources.
    #[default]
    AfterLoad,
    /// Wait for window.load, then idle time (with a two-second bound).
    Idle,
}

impl ScriptStrategy {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Async => "async",
            Self::AfterLoad => "after-load",
            Self::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("script URL must be an HTTPS URL with a host, or a root-relative path")]
pub struct InvalidScriptUrl;

pub(crate) fn valid_resource_url(value: &str) -> bool {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return false;
    }
    let Ok(uri) = value.parse::<http::Uri>() else {
        return false;
    };
    if value.starts_with('/') && !value.starts_with("//") {
        return true;
    }
    uri.scheme_str() == Some("https")
        && uri
            .authority()
            .is_some_and(|host| !host.host().is_empty() && !host.as_str().contains('@'))
}

/// An external script loaded at most once per resolved URL in a document.
///
/// Only pass URLs whose code you trust. CSP nonces are read from WebContext.
/// Repeated declarations use the first declaration's strategy and attributes.
#[derive(Debug, Clone, Copy)]
pub struct Script<'a> {
    src: &'a str,
    strategy: ScriptStrategy,
    integrity: Option<&'a str>,
}

impl<'a> Script<'a> {
    pub fn new(src: &'a str) -> Result<Self, InvalidScriptUrl> {
        if !valid_resource_url(src) {
            return Err(InvalidScriptUrl);
        }
        Ok(Self {
            src,
            strategy: ScriptStrategy::default(),
            integrity: None,
        })
    }

    pub const fn strategy(mut self, strategy: ScriptStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set Subresource Integrity; the browser request uses anonymous CORS.
    pub const fn integrity(mut self, integrity: &'a str) -> Self {
        self.integrity = Some(integrity);
        self
    }
}

// Library integrations deliberately support custom response writers.
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Script<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let nonce = cx.csp_nonce_cloned();
        html_sync! {
            <script
                data-proa-script
                data-src={self.src}
                data-strategy={self.strategy.as_str()}
                data-integrity={self.integrity}
                nonce={csp_nonce_attr(nonce.as_ref())}
            >
                {StaticRaw(crate::LOADER)}
                {StaticRaw(include_str!("script.js"))}
            </script>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_executable_schemes_and_malformed_urls() {
        for url in [
            "javascript:alert(1)",
            "data:text/javascript,alert(1)",
            "//evil.test/x",
            "https://",
            "https:///x",
            "https://[bad",
            "/\\evil.test",
            "https://x.test/\nfoo",
        ] {
            assert!(Script::new(url).is_err(), "{url}");
        }
        assert!(Script::new("/scripts/widget.js").is_ok());
        assert!(Script::new("https://cdn.example.com/widget.js?v=1").is_ok());
    }

    #[test]
    fn escapes_configuration_and_preserves_the_request_nonce() {
        let mut cx =
            WebContext::new().with_csp_nonce(proa_core::CspNonce::new("YWJjZA==").unwrap());
        Script::new("https://cdn.example.com/a.js?a=1&b=%22x%22")
            .unwrap()
            .integrity("sha384-example")
            .render(&mut cx)
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("a=1&amp;b=%22x%22"));
        assert!(html.contains(r#"nonce="YWJjZA==""#));
        assert!(html.contains(r#"data-strategy="after-load""#));
        assert!(html.contains(r#"data-integrity="sha384-example""#));
        assert_eq!(html.matches("<script").count(), 1);
    }
}
