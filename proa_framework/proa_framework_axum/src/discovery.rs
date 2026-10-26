// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Axum responses for sitemap.xml and robots.txt.

use axum::http::{
    header::{CACHE_CONTROL, CONTENT_TYPE},
    HeaderValue, StatusCode,
};
use axum::response::{IntoResponse, Response};
use axum::{body::Bytes, routing::get, Router};
use proa_core::BasePath;
use proa_framework_core::{
    absolute_http_url, DiscoveryError, RobotsTxt, Sitemap, SitemapFile, SitemapIndex,
    SitemapIndexEntry, SitemapUrl, MAX_SITEMAP_ENTRIES,
};

const XML_UTF_8: HeaderValue = HeaderValue::from_static("application/xml; charset=utf-8");
// Media type and encoding: https://www.rfc-editor.org/rfc/rfc9309.html#section-2.3
const TEXT_PLAIN_UTF_8: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
// Proa's HTTP caching policy is separate from the startup byte cache.
// Crawler cache guidance: https://www.rfc-editor.org/rfc/rfc9309.html#section-2.4
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

pub(crate) enum SitemapConfig {
    Single(Vec<SitemapUrl>),
    Multiple(Vec<SitemapFile>),
}

/// Render a sitemap as `application/xml`.
///
/// A render failure is a server configuration error (for example a base path
/// that pushes a loc past the length limit), so it returns a generic 500 and
/// logs the cause instead of exposing it to the client.
pub fn sitemap_xml(sitemap: &Sitemap) -> Response {
    document_response(sitemap.render_xml(), XML_UTF_8)
}

/// Render a sitemap index as `application/xml` for a custom/dynamic handler.
/// Render errors are logged and returned as a generic 500.
pub fn sitemap_index_xml(index: &SitemapIndex) -> Response {
    document_response(index.render_xml(), XML_UTF_8)
}

fn document_response(
    document: Result<String, DiscoveryError>,
    content_type: HeaderValue,
) -> Response {
    match document {
        Ok(body) => (
            [(CONTENT_TYPE, content_type), (CACHE_CONTROL, NO_STORE)],
            body,
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "failed to render discovery document");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(CONTENT_TYPE, TEXT_PLAIN_UTF_8), (CACHE_CONTROL, NO_STORE)],
                "Internal Server Error",
            )
                .into_response()
        }
    }
}

/// Render robots.txt as `text/plain`.
/// See [RFC 9309 §2.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.3).
/// Exceeding [Google's processing limit](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#file-format)
/// logs the error and returns a generic 500, rather than silently losing rules.
pub fn robots_txt(robots: &RobotsTxt) -> Response {
    document_response(robots.try_render_txt(), TEXT_PLAIN_UTF_8)
}

/// Bind the builder's final configuration and cache static documents at startup.
pub(crate) fn discovery_routes(
    origin: Option<&str>,
    base_path: &BasePath,
    sitemaps: Option<SitemapConfig>,
    robots: Option<RobotsTxt>,
) -> Result<Router, DiscoveryError> {
    let mut router = Router::new();
    let sitemap_loc = if let Some(sitemaps) = sitemaps {
        let origin = origin.ok_or(DiscoveryError::MissingPublicOrigin)?;
        let xml = match sitemaps {
            SitemapConfig::Single(urls) => render_sitemap(origin, base_path, urls)?,
            SitemapConfig::Multiple(files) => {
                if files.len() > MAX_SITEMAP_ENTRIES {
                    return Err(DiscoveryError::TooManySitemapEntries);
                }
                let mut index = SitemapIndex::new(origin)?.with_base_path(base_path.clone());
                let mut ids = std::collections::HashSet::new();
                for file in files {
                    if !ids.insert(file.id().to_owned()) {
                        return Err(DiscoveryError::DuplicateSitemapId);
                    }
                    // A sitemap's directory bounds its default URL scope.
                    // Keep child documents alongside the index so they cover
                    // application pages: https://www.sitemaps.org/protocol.html#location
                    let path = format!("/sitemap-{}.xml", file.id());
                    index = index.with_entry(SitemapIndexEntry::new(path.clone())?)?;
                    let xml = render_sitemap(origin, base_path, file.into_urls())?;
                    router = router.route(&path, xml_route(xml));
                }
                index.render_xml()?
            }
        };
        router = router.route("/sitemap.xml", xml_route(xml));
        Some(absolute_http_url(origin, base_path, "/sitemap.xml")?)
    } else {
        None
    };
    if let Some(robots) = robots {
        // Rules stay application paths; the final mount applies once at render.
        let mut robots = robots.with_base_path(base_path.clone());
        if let Some(loc) = sitemap_loc {
            if !robots.sitemaps().contains(&loc) {
                robots = robots.sitemap(&loc)?;
            }
        }
        let body = Bytes::from(robots.try_render_txt()?);
        router = router.route(
            "/robots.txt",
            get(move || {
                let body = body.clone();
                async move {
                    (
                        [(CONTENT_TYPE, TEXT_PLAIN_UTF_8), (CACHE_CONTROL, NO_STORE)],
                        body,
                    )
                }
            }),
        );
    }
    Ok(router)
}

fn render_sitemap(
    origin: &str,
    base_path: &BasePath,
    urls: Vec<SitemapUrl>,
) -> Result<String, DiscoveryError> {
    let mut sitemap = Sitemap::new(origin)?.with_base_path(base_path.clone());
    for url in urls {
        sitemap = sitemap.with_entry(url)?;
    }
    sitemap.render_xml()
}

fn xml_route(xml: String) -> axum::routing::MethodRouter {
    let body = Bytes::from(xml);
    get(move || {
        let body = body.clone();
        async move { ([(CONTENT_TYPE, XML_UTF_8), (CACHE_CONTROL, NO_STORE)], body) }
    })
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use proa_core::BasePath;
    use proa_framework_core::{
        ChangeFreq, RobotsTxt, Sitemap, SitemapIndex, SitemapIndexEntry, SitemapPriority,
        SitemapUrl,
    };
    use tower::ServiceExt;

    use super::{robots_txt, sitemap_index_xml, sitemap_xml};

    #[tokio::test]
    async fn custom_index_response_uses_xml_and_maps_render_errors() {
        let index = SitemapIndex::new("https://example.com")
            .unwrap()
            .with_entry(SitemapIndexEntry::new("/sitemap/0.xml").unwrap())
            .unwrap();
        let response = sitemap_index_xml(&index);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "application/xml; charset=utf-8"
        );
        let xml = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(std::str::from_utf8(&xml)
            .unwrap()
            .contains("<loc>https://example.com/sitemap/0.xml</loc>"));
        let response = sitemap_index_xml(&SitemapIndex::new("https://example.com").unwrap());
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .as_ref(),
            b"Internal Server Error"
        );
    }

    #[tokio::test]
    async fn sitemap_route_uses_base_path_and_xml_content_type() {
        let app = Router::new().route(
            "/sitemap.xml",
            get(|| async {
                sitemap_xml(
                    &Sitemap::new("https://example.com")
                        .unwrap()
                        .with_base_path(BasePath::new("/apps/demo").unwrap())
                        .with_entry(
                            SitemapUrl::new("/pricing")
                                .unwrap()
                                .lastmod("2026-10-23")
                                .unwrap()
                                .changefreq(ChangeFreq::Weekly)
                                .priority(SitemapPriority::HALF),
                        )
                        .unwrap()
                        .with_url("/search?q=a&b=1")
                        .unwrap(),
                )
            }),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sitemap.xml")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "application/xml; charset=utf-8"
        );
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("<loc>https://example.com/apps/demo/pricing</loc>"));
        assert!(body.contains("<loc>https://example.com/apps/demo/search?q=a&amp;b=1</loc>"));
        assert!(body.contains("<priority>0.5</priority>"));
    }

    #[tokio::test]
    async fn sitemap_render_failure_is_a_generic_server_error() {
        // The loc fits at with_url time; the base path added later pushes it over.
        let path = format!("/{}", "a".repeat(2000));
        let sitemap = Sitemap::new("https://example.com")
            .unwrap()
            .with_url(path)
            .unwrap()
            .with_base_path(BasePath::new(&format!("/{}", "p".repeat(100))).unwrap());

        let response = sitemap_xml(&sitemap);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"Internal Server Error");
    }

    #[tokio::test]
    async fn robots_route_points_at_the_prefixed_sitemap() {
        let app = Router::new().route(
            "/robots.txt",
            get(|| async {
                let base_path = BasePath::new("/apps/demo").unwrap();
                robots_txt(
                    &RobotsTxt::new()
                        .group("*")
                        .unwrap()
                        .allow("/")
                        .unwrap()
                        .sitemap_path("https://example.com", &base_path, "/sitemap.xml")
                        .unwrap()
                        .with_base_path(base_path),
                )
            }),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/robots.txt")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert_eq!(
            body,
            "User-agent: *\nAllow: /apps/demo$\nAllow: /apps/demo?\nAllow: /apps/demo/\n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
        );
    }
}
