// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{fmt, sync::Arc};

use crate::shared::attr_value::AttrValue;
use crate::{DataLoader, WebContext, WriteBuf, WriteError};

/// The browser-visible mount path of an application behind a prefix-stripping proxy.
///
/// Configure once at startup. This affects outgoing URLs, not incoming routing.
/// Empty and `/` select the origin root; a trailing slash is normalized away.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BasePath(Option<Arc<str>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("base path must be empty or an absolute URL path without empty/dot segments, a query, or a fragment")]
pub struct InvalidBasePath;

impl BasePath {
    pub fn new(path: &str) -> Result<Self, InvalidBasePath> {
        if path.is_empty() || path == "/" {
            return Ok(Self::default());
        }
        let path = path.strip_suffix('/').unwrap_or(path);
        if !path.starts_with('/') || path.contains(['?', '#', '\\']) {
            return Err(InvalidBasePath);
        }
        let uri: http::Uri = path.parse().map_err(|_| InvalidBasePath)?;
        if uri.path() != path || uri.authority().is_some() || uri.scheme().is_some() {
            return Err(InvalidBasePath);
        }
        for segment in path[1..].split('/') {
            if segment.is_empty() {
                return Err(InvalidBasePath);
            }
            // Reject literal and percent-encoded dot segments and separators:
            // browsers/proxies would normalize those into a different mount.
            let mut decoded = Vec::with_capacity(segment.len());
            let mut bytes = segment.bytes();
            while let Some(byte) = bytes.next() {
                let byte = if byte == b'%' {
                    let high = bytes.next().and_then(hex).ok_or(InvalidBasePath)?;
                    let low = bytes.next().and_then(hex).ok_or(InvalidBasePath)?;
                    high * 16 + low
                } else {
                    byte
                };
                if byte <= b' ' || byte == 127 || byte == b'/' || byte == b'\\' {
                    return Err(InvalidBasePath);
                }
                decoded.push(byte);
            }
            if decoded == b"." || decoded == b".." {
                return Err(InvalidBasePath);
            }
        }
        Ok(Self(Some(Arc::from(path))))
    }

    /// Normalized prefix, or the empty string for an application at `/`.
    pub fn as_str(&self) -> &str {
        self.0.as_deref().unwrap_or("")
    }

    /// Prefix an unprefixed application-local root-relative URL.
    ///
    /// Pass the application's unprefixed path (for example `/static/app.css`).
    /// External, protocol-relative, document-relative, query-only and fragment
    /// references pass through. This is URL composition, not URL sanitization.
    /// The returned attribute value shares the prefix and allocates no string.
    pub fn url<'a>(&self, path: &'a str) -> AppUrl<'a> {
        AppUrl {
            base_path: self.clone(),
            path,
        }
    }
}

fn hex(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|value| value as u8)
}

/// A URL composed without allocating an intermediate string.
///
/// Use directly in `href={...}`, `src={...}` or `action={...}`. Use
/// `.to_string()` at the HTTP boundary when a redirect needs an owned URL.
#[derive(Clone, Debug)]
pub struct AppUrl<'a> {
    base_path: BasePath,
    path: &'a str,
}

impl AppUrl<'_> {
    fn prefix(&self) -> &str {
        if self.path.starts_with('/') && !self.path.starts_with("//") {
            self.base_path.as_str()
        } else {
            ""
        }
    }
}

impl fmt::Display for AppUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.prefix())?;
        f.write_str(self.path)
    }
}

impl AttrValue for AppUrl<'_> {
    fn should_render(&self) -> bool {
        true
    }

    fn url_str(&self) -> Option<&str> {
        // A prefixed root-relative URL has no scheme. Pass-through URLs retain
        // the regular dynamic-attribute scheme validation.
        Some(self.path)
    }

    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        self.prefix().render_attr_value(cx)?;
        self.path.render_attr_value(cx)
    }
}

// Markdown composition uses the active URL/text escaping mode of the context.
impl<L: DataLoader, B: WriteBuf> crate::MdRenderSync<L, B> for AppUrl<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        crate::MdRenderSync::render_md(self.prefix(), cx)?;
        crate::MdRenderSync::render_md(self.path, cx)
    }
}

impl<L: DataLoader, B: WriteBuf> crate::MdRender<L, B> for AppUrl<'_> {
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> crate::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        crate::RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            crate::MdRenderSync::render_md(self, cx),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_mounts_and_composes_local_urls() {
        for input in ["", "/", "/apps/demo", "/apps/demo/"] {
            let base = BasePath::new(input).unwrap();
            let expected = input.trim_end_matches('/');
            for path in ["/", "/docs/start", "/static/app.css?v=2#main"] {
                assert_eq!(base.url(path).to_string(), format!("{expected}{path}"));
            }
            for path in [
                "https://cdn.test/a.js",
                "//cdn.test/a.js",
                "./a",
                "#a",
                "?q=1",
                "",
            ] {
                assert_eq!(base.url(path).to_string(), path);
            }
        }
        assert!(BasePath::new("/user/a%40example.test/proxy/3000/").is_ok());
    }

    #[test]
    fn rejects_paths_browsers_would_reinterpret() {
        for path in [
            "apps/demo",
            "//host",
            "/app//",
            "/a//b",
            "/a/./b",
            "/a/..",
            "/a/%2e%2e",
            "/a/%2f",
            "/a/%5c",
            "/a/%0a",
            "/a/%",
            "/a/%zz",
            "/a?x=1",
            "/a#b",
            "/a b",
            "/a\\b",
        ] {
            assert!(BasePath::new(path).is_err(), "accepted {path:?}");
        }
    }

    #[test]
    fn markdown_composition_matches_a_complete_url() {
        let base = BasePath::new("/apps/a-b").unwrap();
        let path = "/some_path?q=(value)&mode=dark";
        let mut composed = WebContext::new();
        crate::MdRenderSync::render_md(base.url(path), &mut composed).unwrap();
        let mut complete = WebContext::new();
        crate::MdRenderSync::render_md("/apps/a-b/some_path?q=(value)&mode=dark", &mut complete)
            .unwrap();
        assert_eq!(
            composed.into_output().as_slice(),
            complete.into_output().as_slice()
        );
    }

    #[test]
    fn attribute_composition_escapes_both_segments() {
        let url = BasePath::new("/apps/a&b").unwrap().url("/find?q=\"<&");
        let mut cx = WebContext::new();
        url.render_attr_value(&mut cx).unwrap();
        assert_eq!(
            cx.into_output().into_string(),
            "/apps/a&amp;b/find?q=&quot;&lt;&amp;"
        );
    }
}
