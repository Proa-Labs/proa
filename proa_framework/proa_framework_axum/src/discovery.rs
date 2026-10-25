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
use proa_framework_core::{absolute_http_url, DiscoveryError, RobotsTxt, Sitemap, SitemapUrl};

const XML_UTF_8: HeaderValue = HeaderValue::from_static("application/xml; charset=utf-8");
const TEXT_PLAIN_UTF_8: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

/// Render a sitemap as `application/xml`.
///
/// A render failure is a server configuration error (for example a base path
/// that pushes a loc past the length limit), so it returns a generic 500 and
/// logs the cause instead of exposing it to the client.
pub fn sitemap_xml(sitemap: &Sitemap) -> Response {
    match sitemap.render_xml() {
        Ok(body) => ([(CONTENT_TYPE, XML_UTF_8), (CACHE_CONTROL, NO_STORE)], body).into_response(),
        Err(error) => {
            tracing::error!(%error, "failed to render sitemap.xml");
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
pub fn robots_txt(robots: &RobotsTxt) -> Response {
    (
        [(CONTENT_TYPE, TEXT_PLAIN_UTF_8), (CACHE_CONTROL, NO_STORE)],
        robots.render_txt(),
    )
        .into_response()
}

/// Bind the builder's final configuration and cache static documents at startup.
pub(crate) fn discovery_routes(
    origin: Option<&str>,
    base_path: &BasePath,
    urls: Option<Vec<SitemapUrl>>,
    robots: Option<RobotsTxt>,
) -> Result<Router, DiscoveryError> {
    let mut router = Router::new();
    let sitemap_loc = if let Some(urls) = urls {
        let origin = origin.ok_or(DiscoveryError::MissingPublicOrigin)?;
        let mut sitemap = Sitemap::new(origin)?.with_base_path(base_path.clone());
        for url in urls {
            sitemap = sitemap.with_entry(url)?;
        }
        let body = Bytes::from(sitemap.render_xml()?);
        router = router.route(
            "/sitemap.xml",
            get(move || {
                let body = body.clone();
                async move { ([(CONTENT_TYPE, XML_UTF_8), (CACHE_CONTROL, NO_STORE)], body) }
            }),
        );
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
        let body = Bytes::from(robots.render_txt());
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

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use proa_core::BasePath;
    use proa_framework_core::{ChangeFreq, RobotsTxt, Sitemap, SitemapPriority, SitemapUrl};
    use tower::ServiceExt;

    use super::{robots_txt, sitemap_xml};

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
            "User-agent: *\nAllow: /apps/demo$\nAllow: /apps/demo/\n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
        );
    }
}
