// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared HTML/Markdown representation negotiation for typed pages.

use super::response::{current_framework_request_context, defer, framework_response_to_axum};
use axum::http::{
    header::{ACCEPT, LINK, VARY},
    HeaderMap, HeaderValue,
};
use axum::response::IntoResponse;
use futures::FutureExt as _;
use proa_core::{DataLoader, MdRenderSync, WriteError};
use proa_framework_core::{
    render_sync_markdown_response, FrameworkError, FrameworkResponse, MarkdownResponse,
    RouteFinalization,
};
use std::sync::Arc;

use super::RouteResponse;

/// Metadata for the stable Markdown representation of a canonical page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownAlternate {
    path: &'static str,
}

impl MarkdownAlternate {
    /// Named route captures in the alternate path expand to percent-encoded segments.
    pub const fn new(path: &'static str) -> Self {
        Self { path }
    }

    pub const fn path(self) -> &'static str {
        self.path
    }

    /// Render an explicit alias sharing its canonical route's final capture.
    /// For `/guides/{slug}.md`, the renderer sees the same `slug` as on the HTML page.
    pub fn render_explicit<M: MdRenderSync>(
        self,
        markdown: M,
    ) -> Result<RepresentationResponse<proa_core::NoLoader>, WriteError> {
        let mut request = current_framework_request_context();
        let capture = self
            .path
            .rsplit('/')
            .next()
            .and_then(|segment| segment.strip_prefix('{'))
            .and_then(|segment| segment.strip_suffix("}.md"));
        if let (Some(key), Some(params)) = (capture, &request.route_params) {
            let params = proa_core::RouteParams::from_vec(
                params
                    .iter()
                    .map(|param| match param.value().strip_suffix(".md") {
                        Some(value) if param.key() == key => proa_core::RouteParam::new(key, value),
                        _ => param.clone(),
                    })
                    .collect(),
            );
            request.route_params = Some(params);
        }
        render_explicit_markdown(markdown, &request)
    }
}

/// A canonical page response after content negotiation.
///
/// The HTML variant preserves Proa's typed loader and streaming response. The
/// Markdown variant is rendered directly from the page's `MdRenderSync`
/// implementation and deliberately bypasses the HTML document shell.
pub enum RepresentationResponse<L: DataLoader> {
    Html {
        response: RouteResponse<L>,
        alternate: MarkdownAlternate,
    },
    Markdown {
        response: FrameworkResponse,
        canonical: bool,
    },
    /// A transferable root whose active render future stays local to finalization.
    MarkdownAsync {
        response: MarkdownResponse<L>,
        canonical: bool,
    },
}

impl<L: DataLoader + 'static> IntoResponse for RepresentationResponse<L> {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Html {
                response,
                alternate,
            } => {
                let mut response = response.into_response();
                merge_accept_vary(response.headers_mut());
                response
                    .headers_mut()
                    .append(LINK, alternate_link(alternate));
                response
            }
            Self::Markdown {
                response,
                canonical,
            } => markdown_into_response(response, canonical),
            Self::MarkdownAsync {
                response,
                canonical,
            } => {
                let request = current_framework_request_context();
                let mut response = defer(RouteFinalization::Local(Box::new(move || {
                    async move {
                        response
                            .finalize(&request)
                            .await
                            .map_err(FrameworkError::from)
                    }
                    .boxed_local()
                })));
                if canonical {
                    merge_accept_vary(response.headers_mut());
                }
                response
            }
        }
    }
}

fn markdown_into_response(
    response: FrameworkResponse,
    canonical: bool,
) -> axum::response::Response {
    let mut response = framework_response_to_axum(response);
    if canonical {
        merge_accept_vary(response.headers_mut());
    }
    response
}

/// Negotiate a canonical page between browser HTML and semantic Markdown.
///
/// Markdown wins only when the client explicitly gives `text/markdown` a
/// positive quality greater than the quality applicable to HTML. Missing,
/// wildcard-only, invalid, and equal-preference requests stay on HTML.
pub fn negotiate<L, M>(
    headers: &HeaderMap,
    html: RouteResponse<L>,
    markdown: M,
    alternate: MarkdownAlternate,
) -> Result<RepresentationResponse<L>, WriteError>
where
    L: DataLoader,
    M: MdRenderSync,
{
    if prefers_markdown(headers) {
        Ok(RepresentationResponse::Markdown {
            response: render_sync_markdown_response(
                markdown,
                &current_framework_request_context(),
            )?,
            canonical: true,
        })
    } else {
        Ok(RepresentationResponse::Html {
            response: html,
            alternate,
        })
    }
}

/// Render the explicit stable `.md` representation without content
/// negotiation or an HTML document shell.
pub fn explicit_markdown<M>(
    markdown: M,
) -> Result<RepresentationResponse<proa_core::NoLoader>, WriteError>
where
    M: MdRenderSync,
{
    render_explicit_markdown(markdown, &current_framework_request_context())
}

fn render_explicit_markdown<M: MdRenderSync>(
    markdown: M,
    request: &proa_framework_core::FrameworkRequestContext,
) -> Result<RepresentationResponse<proa_core::NoLoader>, WriteError> {
    Ok(RepresentationResponse::Markdown {
        response: render_sync_markdown_response(markdown, request)?,
        canonical: false,
    })
}

/// Negotiate before rendering, preserving the supplied request-scoped loader.
///
/// This constructs an unopened plan. The handler remains `Send` even when the
/// renderer creates local state across `.await`. The selected render runs at
/// response finalization; failures become the standard HTTP render error.
pub fn negotiate_async_with_loader<L, M>(
    headers: &HeaderMap,
    html: RouteResponse<L>,
    markdown: M,
    loader: Arc<L>,
    alternate: MarkdownAlternate,
) -> RepresentationResponse<L>
where
    L: DataLoader + 'static,
    M: proa_core::MdRender<L> + Send + 'static,
{
    if prefers_markdown(headers) {
        RepresentationResponse::MarkdownAsync {
            response: MarkdownResponse::with_loader(markdown, loader),
            canonical: true,
        }
    } else {
        RepresentationResponse::Html {
            response: html,
            alternate,
        }
    }
}

/// Construct a loader-aware plan for a stable Markdown URL, without negotiation.
pub fn explicit_markdown_async_with_loader<L, M>(
    markdown: M,
    loader: Arc<L>,
) -> RepresentationResponse<L>
where
    L: DataLoader + 'static,
    M: proa_core::MdRender<L> + Send + 'static,
{
    RepresentationResponse::MarkdownAsync {
        response: MarkdownResponse::with_loader(markdown, loader),
        canonical: false,
    }
}

fn prefers_markdown(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(ACCEPT).and_then(|value| value.to_str().ok()) else {
        return false;
    };

    let ranges: Vec<MediaRange<'_>> = value.split(',').filter_map(MediaRange::parse).collect();
    let has_explicit_markdown = ranges.iter().any(|range| {
        range.ty.eq_ignore_ascii_case("text")
            && range.subtype.eq_ignore_ascii_case("markdown")
            && range.quality > 0
    });
    if !has_explicit_markdown {
        return false;
    }

    let markdown = quality_for(&ranges, "text", "markdown");
    let html = quality_for(&ranges, "text", "html");
    markdown > 0 && markdown > html
}

#[derive(Debug, Clone, Copy)]
struct MediaRange<'a> {
    ty: &'a str,
    subtype: &'a str,
    quality: u16,
}

impl<'a> MediaRange<'a> {
    fn parse(raw: &'a str) -> Option<Self> {
        let mut parts = raw.trim().split(';');
        let media = parts.next()?.trim();
        let (ty, subtype) = media.split_once('/')?;
        let ty = ty.trim();
        let subtype = subtype.trim();
        if ty.is_empty() || subtype.is_empty() {
            return None;
        }

        let mut quality = 1_000;
        for parameter in parts {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("q") {
                quality = parse_quality(value.trim())?;
            }
        }

        Some(Self {
            ty,
            subtype,
            quality,
        })
    }

    fn specificity(self, ty: &str, subtype: &str) -> Option<u8> {
        if self.ty == "*" && self.subtype == "*" {
            Some(0)
        } else if self.ty.eq_ignore_ascii_case(ty) && self.subtype == "*" {
            Some(1)
        } else if self.ty.eq_ignore_ascii_case(ty) && self.subtype.eq_ignore_ascii_case(subtype) {
            Some(2)
        } else {
            None
        }
    }
}

fn quality_for(ranges: &[MediaRange<'_>], ty: &str, subtype: &str) -> u16 {
    ranges
        .iter()
        .filter_map(|range| {
            range
                .specificity(ty, subtype)
                .map(|specificity| (specificity, range.quality))
        })
        .max_by_key(|(specificity, quality)| (*specificity, *quality))
        .map(|(_, quality)| quality)
        .unwrap_or(0)
}

fn parse_quality(raw: &str) -> Option<u16> {
    if raw == "0" {
        return Some(0);
    }
    if raw == "1" {
        return Some(1_000);
    }

    let (whole, fraction) = raw.split_once('.')?;
    if !matches!(whole, "0" | "1") || fraction.len() > 3 || fraction.is_empty() {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut scaled = fraction.parse::<u16>().ok()?;
    for _ in fraction.len()..3 {
        scaled *= 10;
    }
    if whole == "1" {
        (scaled == 0).then_some(1_000)
    } else {
        Some(scaled)
    }
}

fn merge_accept_vary(headers: &mut HeaderMap) {
    let already_varies = headers.get_all(VARY).iter().any(|value| {
        value.to_str().is_ok_and(|text| {
            text.split(',')
                .any(|token| token.trim() == "*" || token.trim().eq_ignore_ascii_case("accept"))
        })
    });
    if !already_varies {
        headers.append(VARY, HeaderValue::from_static("Accept"));
    }
}

fn alternate_link(alternate: MarkdownAlternate) -> HeaderValue {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    use std::borrow::Cow;
    const SEGMENT: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    let mut path = Cow::Borrowed(alternate.path());
    if path.contains('{') {
        let _ = super::context::ROUTE_CONTEXT.try_with(|ctx| {
            for param in ctx.borrow_mut().current_route_params() {
                let capture = format!("{{{}}}", param.key());
                if path.contains(&capture) {
                    path = Cow::Owned(path.replace(
                        &capture,
                        &utf8_percent_encode(param.value(), SEGMENT).to_string(),
                    ));
                }
            }
        });
    }
    HeaderValue::from_str(&format!(
        "<{}>; rel=\"alternate\"; type=\"text/markdown\"",
        super::app_url(&path)
    ))
    .expect("Markdown alternate paths must be valid header values")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::header::CONTENT_TYPE;
    use axum::http::StatusCode;
    use axum::{
        http::{header, Request},
        routing::get,
        Router,
    };
    use http_body_util::BodyExt;
    use proa_core::WebContext;
    use proa_core::{text::StaticRaw, WriteBuf, WriteError};
    use proa_macros::md_sync;
    use std::{collections::HashMap, sync::Arc};
    use tower::ServiceExt;

    use crate::route::context::RouteContextLayer;

    #[derive(Clone, Copy)]
    struct MarkdownPage;

    impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for MarkdownPage {
        fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
            md_sync! { r###"
                # Route dossier

                Semantic body.
                "###
            }
            .render_md(cx)
        }
    }

    fn headers(accept: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(accept) = accept {
            headers.insert(ACCEPT, HeaderValue::from_str(accept).unwrap());
        }
        headers
    }

    #[test]
    fn negotiation_defaults_to_html_for_absent_wildcard_equal_and_zero_quality() {
        for accept in [
            None,
            Some("*/*"),
            Some("text/*"),
            Some("text/html, text/markdown"),
            Some("text/html;q=0.8, text/markdown;q=0.8"),
            Some("text/markdown;q=0, text/html;q=0.1"),
            Some("text/markdown;q=0.5, */*;q=1"),
        ] {
            assert!(!prefers_markdown(&headers(accept)), "{accept:?}");
        }
    }

    #[test]
    fn negotiation_selects_explicitly_preferred_markdown() {
        for accept in [
            "text/markdown",
            "text/html;q=0.4, text/markdown;q=0.9",
            "application/json;q=1, text/markdown;q=0.2",
        ] {
            assert!(prefers_markdown(&headers(Some(accept))), "{accept}");
        }
    }

    #[tokio::test]
    async fn canonical_html_advertises_markdown_and_varies_on_accept() {
        let app = Router::new()
            .route(
                "/",
                get(|| async {
                    negotiate(
                        &headers(None),
                        RouteResponse::ssr(StaticRaw("<!DOCTYPE html><p>HTML</p>")),
                        MarkdownPage,
                        MarkdownAlternate::new("/index.md"),
                    )
                    .unwrap()
                }),
            )
            .layer(RouteContextLayer::new(Arc::new(HashMap::new())));
        let response = app.oneshot(Request::new(Body::empty())).await.unwrap();

        assert_eq!(response.headers()[VARY], "Accept");
        assert_eq!(
            response.headers()[LINK],
            "</index.md>; rel=\"alternate\"; type=\"text/markdown\""
        );
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
    }

    #[tokio::test]
    async fn alternate_link_encodes_each_captured_segment() {
        let app = Router::new()
            .route(
                "/guides/{slug}",
                get(|| async {
                    negotiate(
                        &headers(None),
                        RouteResponse::ssr(StaticRaw("HTML")),
                        MarkdownPage,
                        MarkdownAlternate::new("/guides/{slug}.md"),
                    )
                    .unwrap()
                }),
            )
            .layer(RouteContextLayer::new(Arc::new(HashMap::new())));
        let request = Request::builder()
            .uri("/guides/a%2Fb%20c")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.headers()[LINK],
            "</guides/a%2Fb%20c.md>; rel=\"alternate\"; type=\"text/markdown\""
        );
    }

    #[tokio::test]
    async fn explicit_alias_strips_one_suffix_from_only_the_final_capture() {
        struct Page;
        impl MdRenderSync for Page {
            fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
                let params = cx.route_params().unwrap();
                assert_eq!(params.get("section"), Some("a.md"));
                assert_eq!(params.get("slug"), Some("b/c.md"));
                Ok(())
            }
        }
        const ALTERNATE: MarkdownAlternate = MarkdownAlternate::new("/{section}/{slug}.md");
        let app = Router::new()
            .route(
                "/{section}/{slug}",
                get(|| async { ALTERNATE.render_explicit(Page).unwrap() }),
            )
            .layer(RouteContextLayer::new(Arc::new(HashMap::new())));
        let request = Request::builder()
            .uri("/a.md/b%2Fc.md.md")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn canonical_and_explicit_markdown_bodies_are_identical() {
        let canonical = negotiate(
            &headers(Some("text/markdown")),
            RouteResponse::ssr(StaticRaw("unused")),
            MarkdownPage,
            MarkdownAlternate::new("/index.md"),
        )
        .unwrap()
        .into_response();
        let explicit = explicit_markdown(MarkdownPage).unwrap().into_response();

        assert_eq!(canonical.headers()[VARY], "Accept");
        assert!(explicit.headers().get(VARY).is_none());
        assert_eq!(
            canonical.headers()[CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert_eq!(
            explicit.headers()[CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert!(canonical.headers().get(header::LINK).is_none());

        let canonical = canonical.into_body().collect().await.unwrap().to_bytes();
        let explicit = explicit.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(canonical, explicit);
        assert_eq!(canonical, "# Route dossier\n\nSemantic body.\n\n");
    }
}

#[cfg(test)]
mod shared_context_tests {
    use super::*;
    use crate::route::context::{RouteContextLayer, ROUTE_CONTEXT};
    use axum::http::StatusCode;
    use axum::{
        body::Body,
        http::{header, Method, Request},
        routing::get,
        Router,
    };
    use http_body_util::BodyExt;
    use proa_core::{
        CspNonce, LoadError, LoadKey, LoadRequest, LoadValue, RenderOutcome, RouteParam,
        RouteParams, WebContext, WebRender, WriteBuf,
    };
    use std::{
        collections::HashMap,
        future::Future,
        rc::Rc,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };
    use tower::ServiceExt;

    struct Loader {
        loads: AtomicUsize,
    }
    impl DataLoader for Loader {
        fn load(
            &self,
            _: &LoadRequest,
        ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
            async move {
                tokio::task::yield_now().await;
                self.loads.fetch_add(1, Ordering::Relaxed);
                Ok(LoadValue::arc(Arc::new(String::from("*loaded*"))))
            }
        }
    }

    struct LoadedMarkdown {
        loader: Arc<Loader>,
    }
    impl proa_core::MdRender<Loader> for LoadedMarkdown {
        fn render_md(
            self,
            cx: &mut WebContext<Loader>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                assert!(Arc::ptr_eq(cx.loader(), &self.loader));
                assert_eq!(cx.request().method(), Some(&Method::GET));
                assert_eq!(cx.request().headers()["x-request"], "shared");
                assert_eq!(cx.request_location().unwrap().path(), "/article");
                assert_eq!(cx.route_params().unwrap().get("slug"), Some("article"));
                assert_eq!(cx.csp_nonce().unwrap().as_str(), "test-nonce");
                let local = Rc::new(7);
                let value = cx
                    .load_typed::<String>(LoadKey::named("article", "one"))
                    .await?;
                assert_eq!(*local, 7);
                cx.response_mut().set_status(StatusCode::CREATED);
                cx.response_mut()
                    .append_set_cookie(HeaderValue::from_static("a=1"));
                cx.response_mut()
                    .append_set_cookie(HeaderValue::from_static("b=2"));
                cx.response_mut()
                    .append_header(VARY, HeaderValue::from_static("Origin"));
                cx.response_mut()
                    .append_header(LINK, HeaderValue::from_static("</one>; rel=preload"));
                cx.response_mut()
                    .append_header(LINK, HeaderValue::from_static("</two>; rel=preload"));
                cx.head_mut().set_title("must not become an HTML shell");
                cx.write_md_text(value.as_str())
            })
        }
    }

    struct HtmlCount(Arc<AtomicUsize>);
    impl WebRender<Loader> for HtmlCount {
        fn render(
            self,
            cx: &mut WebContext<Loader>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            // proa-perf-ok: fixed trusted markup probes representation selection in this test
            let written = cx.out.extend_static_str("<p>selected</p>");
            RenderOutcome::<std::future::Ready<_>>::Done(written)
        }
    }

    #[tokio::test]
    async fn negotiated_handler_defers_local_markdown_and_preserves_request_and_response() {
        let loader = Arc::new(Loader {
            loads: AtomicUsize::new(0),
        });
        let html_calls = Arc::new(AtomicUsize::new(0));
        let handler_loader = Arc::clone(&loader);
        let handler_html = Arc::clone(&html_calls);
        let app = Router::new()
            .route(
                "/article",
                get(move |headers: HeaderMap| {
                    let loader = Arc::clone(&handler_loader);
                    let calls = Arc::clone(&handler_html);
                    async move {
                        // The route matcher normally installs these before finalization.
                        ROUTE_CONTEXT.with(|context| {
                            context
                                .borrow_mut()
                                .set_route_params(RouteParams::from_vec(vec![RouteParam::new(
                                    "slug", "article",
                                )]));
                        });
                        negotiate_async_with_loader(
                            &headers,
                            RouteResponse::ssr_async_with_loader(
                                HtmlCount(calls),
                                Arc::clone(&loader),
                            ),
                            LoadedMarkdown {
                                loader: Arc::clone(&loader),
                            },
                            loader,
                            MarkdownAlternate::new("/article.md"),
                        )
                    }
                }),
            )
            .layer(RouteContextLayer::new(Arc::new(HashMap::new())));
        let request = |accept| {
            let mut request = Request::builder()
                .uri("/article")
                .header(ACCEPT, accept)
                .header("x-request", "shared")
                .body(Body::empty())
                .unwrap();
            request
                .extensions_mut()
                .insert(CspNonce::new("test-nonce").unwrap());
            request
        };
        let response = app.clone().oneshot(request("text/markdown")).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert_eq!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
        assert_eq!(response.headers().get_all(LINK).iter().count(), 2);
        assert_eq!(
            response.headers().get_all(VARY).iter().collect::<Vec<_>>(),
            ["Origin", "Accept"]
        );
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "\\*loaded\\*"
        );
        assert_eq!(loader.loads.load(Ordering::Relaxed), 1);
        assert_eq!(html_calls.load(Ordering::Relaxed), 0);
        let html = app.oneshot(request("text/html")).await.unwrap();
        assert_eq!(html.status(), StatusCode::OK);
        assert_eq!(html_calls.load(Ordering::Relaxed), 1);
        assert_eq!(loader.loads.load(Ordering::Relaxed), 1);
    }

    struct Fails;
    impl proa_core::MdRender for Fails {
        fn render_md(
            self,
            _: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async {
                tokio::task::yield_now().await;
                Err(WriteError::render("private diagnostic"))
            })
        }
    }

    #[tokio::test]
    async fn explicit_async_errors_use_http_render_error_mapping() {
        let app = Router::new()
            .route(
                "/failed.md",
                get(|| async {
                    explicit_markdown_async_with_loader(Fails, Arc::new(proa_core::NoLoader))
                }),
            )
            .layer(RouteContextLayer::new(Arc::new(HashMap::new())));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/failed.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!response.headers().contains_key(VARY));
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "render error"
        );
    }

    struct Pages {
        source: String,
        pointers: Arc<Mutex<Vec<usize>>>,
    }
    impl MdRenderSync for Pages {
        fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
            cx.write_md_trusted(&self.source)?;
            *self.pointers.lock().unwrap() = cx
                .out
                .slices()
                .filter(|p| !p.is_empty())
                .map(|p| p.as_ptr() as usize)
                .collect();
            Ok(())
        }
    }

    #[tokio::test]
    async fn standalone_buffered_response_transfers_pages_without_copying() {
        let pointers = Arc::new(Mutex::new(Vec::new()));
        let source = "page contents\n".repeat(900);
        let response = explicit_markdown(Pages {
            source: source.clone(),
            pointers: Arc::clone(&pointers),
        })
        .unwrap()
        .into_response();
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            source.len().to_string()
        );
        let mut body = response.into_body();
        let mut actual = Vec::new();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let chunk = frame.unwrap().into_data().unwrap();
            actual.push(chunk.as_ptr() as usize);
            bytes.extend_from_slice(&chunk);
        }
        assert_eq!(actual, *pointers.lock().unwrap());
        assert!(actual.len() > 1);
        assert_eq!(bytes, source.as_bytes());
    }

    #[test]
    fn vary_merging_keeps_existing_values_and_wildcard() {
        for value in ["Origin", "*", "aCcEpT, Origin"] {
            let mut headers = HeaderMap::new();
            headers.append(VARY, HeaderValue::from_str(value).unwrap());
            merge_accept_vary(&mut headers);
            assert_eq!(headers[VARY], value);
            assert_eq!(
                headers.get_all(VARY).iter().count(),
                if value == "Origin" { 2 } else { 1 }
            );
        }
    }
}
