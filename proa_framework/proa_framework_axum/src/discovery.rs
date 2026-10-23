// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Axum responses for sitemap.xml and robots.txt.

use axum::http::{
    header::{CACHE_CONTROL, CONTENT_TYPE},
    HeaderValue, StatusCode,
};
use axum::response::{IntoResponse, Response};
use proa_framework_core::{RobotsTxt, Sitemap};

const XML_UTF_8: HeaderValue = HeaderValue::from_static("application/xml; charset=utf-8");
const TEXT_PLAIN_UTF_8: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

/// Render a sitemap as `application/xml`.
pub fn sitemap_xml(sitemap: &Sitemap) -> Response {
    match sitemap.render_xml() {
        Ok(body) => ([(CONTENT_TYPE, XML_UTF_8), (CACHE_CONTROL, NO_STORE)], body).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
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
    async fn robots_route_points_at_the_prefixed_sitemap() {
        let app = Router::new().route(
            "/robots.txt",
            get(|| async {
                let base_path = BasePath::new("/apps/demo").unwrap();
                robots_txt(
                    &RobotsTxt::new()
                        .group("*")
                        .unwrap()
                        .allow_path(&base_path, "/")
                        .unwrap()
                        .sitemap_path("https://example.com", &base_path, "/sitemap.xml")
                        .unwrap(),
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
            "User-agent: *\nAllow: /apps/demo/\n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
        );
    }
}
