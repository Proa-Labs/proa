// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, fmt, sync::Arc};

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
                if byte < b' ' || byte == 127 || byte == b'/' || byte == b'\\' {
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
        AppUrl(UrlKind::Path(self.clone(), path))
    }
}

fn hex(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|value| value as u8)
}

/// Browser URLs prepared by a framework adapter for one public asset catalog.
///
/// Keys are paths relative to the public directory. Values are complete,
/// ready-to-render URLs, including any deployment prefix.
#[derive(Clone, Debug)]
pub struct PublicAssetUrls(Arc<BTreeMap<String, Arc<str>>>);

impl PublicAssetUrls {
    pub fn new(urls: BTreeMap<String, Arc<str>>) -> Self {
        Self(Arc::new(urls))
    }

    pub fn get(&self, logical: &str) -> Option<&str> {
        self.0.get(logical).map(AsRef::as_ref)
    }

    pub(super) fn resolve<'a>(urls: Option<&Self>, logical: &'a str) -> AppUrl<'a> {
        if let Some(urls) = urls {
            // Literal filenames can contain URL delimiters. A catalog entry
            // for the exact spelling takes precedence and is already encoded.
            if let Some(url) = urls.0.get(logical) {
                return AppUrl(UrlKind::Public {
                    url: url.clone(),
                    suffix: "",
                });
            }
            if let Some(split) = logical.find(['?', '#']) {
                let (path, suffix) = logical.split_at(split);
                if let Some(url) = urls.0.get(path) {
                    return AppUrl(UrlKind::Public {
                        url: url.clone(),
                        suffix,
                    });
                }
            }
        }
        AppUrl(UrlKind::MissingPublic(logical))
    }
}

/// Files the framework itself puts in front of a browser, such as the RSJS
/// runtime and island modules, published under content-hashed names.
///
/// Keys are the logical names the framework knows a file by. Each value is
/// the file's path from the origin root, with no leading slash and no
/// deployment prefix: one build output is the same under every mount and on
/// any asset host, so the prefix is added when a URL is written.
#[derive(Clone, Debug, Default)]
pub struct FrameworkAssets(Arc<FrameworkAssetIndex>);

#[derive(Debug, Default)]
struct FrameworkAssetIndex {
    by_logical: BTreeMap<String, FrameworkAsset>,
    integrity_by_path: BTreeMap<Arc<str>, Arc<str>>,
}

/// One published framework file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameworkAsset {
    /// Path from the origin root, such as `_proa/immutable/rsjs/runtime.<hash>.js`.
    pub path: Arc<str>,
    /// Subresource Integrity value for the file's bytes.
    pub integrity: Arc<str>,
}

impl FrameworkAssets {
    pub fn new(assets: BTreeMap<String, FrameworkAsset>) -> Self {
        let integrity_by_path = assets
            .values()
            .map(|asset| (asset.path.clone(), asset.integrity.clone()))
            .collect();
        Self(Arc::new(FrameworkAssetIndex {
            by_logical: assets,
            integrity_by_path,
        }))
    }

    pub fn get(&self, logical: &str) -> Option<&FrameworkAsset> {
        self.0.by_logical.get(logical)
    }

    /// The integrity value of the file published at `path`.
    pub fn integrity(&self, path: &str) -> Option<&str> {
        self.0.integrity_by_path.get(path).map(AsRef::as_ref)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FrameworkAsset)> {
        self.0
            .by_logical
            .iter()
            .map(|(logical, asset)| (logical.as_str(), asset))
    }
}

/// A URL composed without allocating an intermediate string.
///
/// Use directly in `href={...}`, `src={...}` or `action={...}`.
/// Use [`try_to_string`](Self::try_to_string) at the HTTP boundary if the
/// path may reference a managed public asset. Display shows a diagnostic
/// for an unresolved asset.
#[derive(Clone, Debug)]
pub struct AppUrl<'a>(UrlKind<'a>);

#[derive(Clone, Debug)]
enum UrlKind<'a> {
    Path(BasePath, &'a str),
    Public { url: Arc<str>, suffix: &'a str },
    MissingPublic(&'a str),
}

impl AppUrl<'_> {
    fn segments(&self) -> Result<(&str, &str), &str> {
        match &self.0 {
            UrlKind::Path(base_path, path) => {
                let prefix = if path.starts_with('/') && !path.starts_with("//") {
                    base_path.as_str()
                } else {
                    ""
                };
                Ok((prefix, path))
            }
            UrlKind::Public { url, suffix } => Ok((url, suffix)),
            UrlKind::MissingPublic(path) => Err(path),
        }
    }

    fn render_segments(&self) -> Result<(&str, &str), WriteError> {
        self.segments().map_err(missing_public)
    }

    /// Compose an owned URL, returning a render error for a missing asset.
    pub fn try_to_string(&self) -> Result<String, WriteError> {
        let (prefix, path) = self.render_segments()?;
        Ok([prefix, path].concat())
    }
}

/// A URL that head metadata accepts: a string, or the value `cx.url(...)`
/// returns.
///
/// Passing the `cx.url(...)` value itself, rather than a string made from it,
/// is what keeps a managed `public/...` asset and a deployment mount correct:
/// the head resolves application paths against its own trusted origin and
/// mount, so it needs the path before the request's prefix is applied.
///
/// ```
/// # use proa_core::{HeadMedia, WebContext};
/// let cx = WebContext::new();
/// let image = HeadMedia::from_url(cx.url("/og/home.png"))?;
/// let owned = HeadMedia::from_url(String::from("https://cdn.example/og.png"))?;
/// # let _ = (image, owned);
/// # Ok::<(), proa_core::WriteError>(())
/// ```
pub trait TryIntoHeadUrl {
    /// Borrowed when the input can lend its text, owned otherwise, so a
    /// caller that only reads the URL does not pay for a copy.
    type Url: AsRef<str> + Into<String>;

    /// Fails for a `public/...` path that the asset catalog does not contain.
    fn try_into_head_url(self) -> Result<Self::Url, WriteError>;
}

impl<'a> TryIntoHeadUrl for &'a str {
    type Url = &'a str;
    fn try_into_head_url(self) -> Result<Self::Url, WriteError> {
        Ok(self)
    }
}

impl TryIntoHeadUrl for String {
    type Url = String;
    fn try_into_head_url(self) -> Result<Self::Url, WriteError> {
        Ok(self)
    }
}

impl<'a> TryIntoHeadUrl for std::borrow::Cow<'a, str> {
    type Url = std::borrow::Cow<'a, str>;
    fn try_into_head_url(self) -> Result<Self::Url, WriteError> {
        Ok(self)
    }
}

// The owned string types an application is likely to keep a URL in. A plain
// `&str` parameter accepted all of these through deref coercion; a generic
// one has to name them.
macro_rules! impl_head_url_for_borrowed_strings {
    ($($ty:ty),* $(,)?) => {$(
        impl<'a> TryIntoHeadUrl for &'a $ty {
            type Url = &'a str;
            fn try_into_head_url(self) -> Result<Self::Url, WriteError> {
                Ok(self)
            }
        }
    )*};
}
impl_head_url_for_borrowed_strings!(String, Box<str>, Arc<str>, std::borrow::Cow<'_, str>);

impl<'a> TryIntoHeadUrl for AppUrl<'a> {
    type Url = std::borrow::Cow<'a, str>;
    fn try_into_head_url(self) -> Result<Self::Url, WriteError> {
        match self.0 {
            // The head applies its own mount to application paths, so it
            // receives the path without this request's prefix.
            UrlKind::Path(_, path) => Ok(std::borrow::Cow::Borrowed(path)),
            // proa-perf-ok: one URL per head field, prepared once per page
            UrlKind::Public { url, suffix } => Ok([url.as_ref(), suffix].concat().into()),
            UrlKind::MissingPublic(path) => Err(missing_public(path)),
        }
    }
}

impl<'a> TryIntoHeadUrl for &'a AppUrl<'_> {
    type Url = std::borrow::Cow<'a, str>;
    fn try_into_head_url(self) -> Result<Self::Url, WriteError> {
        match &self.0 {
            UrlKind::Path(_, path) => Ok(std::borrow::Cow::Borrowed(path)),
            UrlKind::Public { url, suffix: "" } => Ok(std::borrow::Cow::Borrowed(url)),
            // proa-perf-ok: one URL per head field, prepared once per page
            UrlKind::Public { url, suffix } => Ok([url.as_ref(), suffix].concat().into()),
            UrlKind::MissingPublic(path) => Err(missing_public(path)),
        }
    }
}

fn missing_public(path: &str) -> WriteError {
    WriteError::render(format!("public asset `public/{path}` is missing"))
}

impl fmt::Display for AppUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.segments() {
            Ok((prefix, path)) => {
                f.write_str(prefix)?;
                f.write_str(path)
            }
            Err(path) => write!(f, "missing public asset `public/{path}`"),
        }
    }
}

impl AttrValue for AppUrl<'_> {
    fn should_render(&self) -> bool {
        true
    }

    fn url_str(&self) -> Option<&str> {
        match &self.0 {
            UrlKind::Path(_, path) => Some(path),
            UrlKind::Public { url, .. } => Some(url),
            UrlKind::MissingPublic(_) => None,
        }
    }

    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        let (prefix, path) = self.render_segments()?;
        prefix.render_attr_value(cx)?;
        path.render_attr_value(cx)
    }
}

// Markdown composition uses the active URL/text escaping mode of the context.
impl<L: DataLoader, B: WriteBuf> crate::MdRenderSync<L, B> for AppUrl<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let (prefix, path) = self.render_segments()?;
        crate::MdRenderSync::render_md(prefix, cx)?;
        crate::MdRenderSync::render_md(path, cx)
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
        for path in [
            "/user/a%40example.test/proxy/3000/",
            "/user/a%20b/proxy/3000/",
            "/apps/%E2%9C%93/",
        ] {
            assert!(BasePath::new(path).is_ok(), "rejected {path:?}");
        }
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

    #[test]
    fn missing_public_assets_report_the_logical_path() {
        let mut cx = WebContext::new();
        let url = cx.url("public/img/missing.png");
        let error = url.try_to_string().unwrap_err();
        assert!(error.to_string().contains("public/img/missing.png"));
        assert_eq!(url.render_attr_value(&mut cx), Err(error.clone()));
        assert_eq!(crate::MdRenderSync::render_md(url, &mut cx), Err(error));
        assert!(cx.into_output().is_empty());
    }
}
