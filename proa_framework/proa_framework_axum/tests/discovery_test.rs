// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    routing::get,
    Router,
};
use proa_framework_axum::{
    BasePath, ChangeFreq, DiscoveryError, FrameworkBuilder, IslandManifest, ManifestAssets,
    RobotsTxt, SitemapFile, SitemapPriority, SitemapUrl,
};
use tower::ServiceExt;

fn builder() -> FrameworkBuilder {
    FrameworkBuilder::new(IslandManifest {
        islands: HashMap::new(),
        assets: ManifestAssets::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    })
}

fn entries() -> Vec<SitemapUrl> {
    vec![
        SitemapUrl::new("/").unwrap(),
        SitemapUrl::new("/pricing?q=1&lang=en")
            .unwrap()
            .lastmod("2026-10-25")
            .unwrap()
            .changefreq(ChangeFreq::Weekly)
            .priority(SitemapPriority::HALF),
        SitemapUrl::new("https://example.com:443/docs").unwrap(),
    ]
}

fn robots() -> RobotsTxt {
    RobotsTxt::new()
        .group("*")
        .unwrap()
        .allow("/")
        .unwrap()
        .disallow("/admin")
        .unwrap()
}

async fn generate_sitemaps() -> Result<Vec<SitemapFile>, DiscoveryError> {
    tokio::task::yield_now().await;
    Ok(vec![
        SitemapFile::new("pages", [SitemapUrl::new("/")?])?,
        SitemapFile::new(
            "products-0",
            [SitemapUrl::new("/products/1?q=a&b=c")?.lastmod("2026-10-25")?],
        )?,
    ])
}

#[tokio::test]
async fn async_generated_robots_preserves_groups_and_extensions_under_final_mount() {
    async fn generate_robots(paths: &[&str]) -> Result<RobotsTxt, DiscoveryError> {
        tokio::task::yield_now().await;
        let mut robots = RobotsTxt::new().group_agents(["Applebot", "Bingbot"])?;
        for path in paths {
            robots = robots.disallow(path)?;
        }
        Ok(robots
            .crawl_delay(std::time::Duration::from_millis(1250))
            .directive("Clean-param", "ref /public/")?
            .group("*")?
            .allow("/")?
            .host("example.com")?
            .sitemap("https://example.com/app/sitemap.xml")?
            .sitemap("https://cdn.example/extra.xml")?
            .with_base_path(BasePath::new("/old").unwrap()))
    }

    let app = builder()
        .with_robots(generate_robots(&["/private/", "/admin/"]).await.unwrap())
        .with_sitemaps(generate_sitemaps().await.unwrap())
        .with_base_path(BasePath::new("/app").unwrap())
        .with_public_origin("https://example.com")
        .unwrap()
        .try_build()
        .unwrap();
    let (status, headers, body) = fetch(&app, "/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(
        body,
        concat!(
            "User-agent: Applebot\nUser-agent: Bingbot\n",
            "Disallow: /app/private/\nDisallow: /app/admin/\n",
            "Crawl-delay: 1.25\nClean-param: ref /public/\n\n",
            "User-agent: *\nAllow: /app$\nAllow: /app/\n\n",
            "Host: example.com\nSitemap: https://example.com/app/sitemap.xml\n",
            "Sitemap: https://cdn.example/extra.xml\n"
        )
    );
    assert_eq!(fetch(&app, "/robots.txt").await.2, body);
}

#[tokio::test]
async fn multiple_sitemaps_serve_an_index_and_separate_documents() {
    let app = builder()
        .with_sitemaps(generate_sitemaps().await.unwrap())
        .with_public_origin("https://old.example")
        .unwrap()
        .with_base_path(BasePath::new("/old").unwrap())
        .with_robots(
            robots()
                .sitemap("https://example.com/app/sitemap.xml")
                .unwrap(),
        )
        .with_public_origin("https://example.com")
        .unwrap()
        .with_base_path(BasePath::new("/app").unwrap())
        .try_build()
        .unwrap();
    let (status, headers, index) = fetch(&app, "/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/xml; charset=utf-8"
    );
    assert!(index.contains("<sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">"));
    assert!(index.contains("<loc>https://example.com/app/sitemap/pages.xml</loc>"));
    assert!(index.contains("<loc>https://example.com/app/sitemap/products-0.xml</loc>"));
    assert_eq!(index.matches("<sitemap>").count(), 2);
    assert!(!index.contains("<url>"));
    for (path, expected_loc) in [
        ("/sitemap/pages.xml", "https://example.com/app/"),
        (
            "/sitemap/products-0.xml",
            "https://example.com/app/products/1?q=a&amp;b=c",
        ),
    ] {
        let (status, headers, xml) = fetch(&app, path).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "application/xml; charset=utf-8"
        );
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(xml.matches("<url>").count(), 1);
        assert!(xml.contains(&format!("<loc>{expected_loc}</loc>")));
        let head = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(
            head.headers()[header::CONTENT_TYPE],
            "application/xml; charset=utf-8"
        );
        assert!(to_bytes(head.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty());
    }
    let products = fetch(&app, "/sitemap/products-0.xml").await.2;
    assert!(products.contains("<lastmod>2026-10-25</lastmod>"));
    let robots = fetch(&app, "/robots.txt").await.2;
    assert_eq!(robots.matches("Sitemap:").count(), 1);
    assert!(robots.contains("Sitemap: https://example.com/app/sitemap.xml\n"));
    assert_eq!(
        fetch(&app, "/sitemap/missing.xml").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fetch(&app, "/app/sitemap/pages.xml").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn multiple_sitemaps_reject_invalid_startup_configuration() {
    assert!(matches!(
        builder()
            .with_sitemaps(generate_sitemaps().await.unwrap())
            .try_build(),
        Err(DiscoveryError::MissingPublicOrigin)
    ));
    let configured = || builder().with_public_origin("https://example.com").unwrap();
    assert!(matches!(
        configured().with_sitemaps([]).try_build(),
        Err(DiscoveryError::EmptySitemapIndex)
    ));
    let file = SitemapFile::new("pages", [SitemapUrl::new("/").unwrap()]).unwrap();
    assert!(matches!(
        configured().with_sitemaps([file.clone(), file]).try_build(),
        Err(DiscoveryError::DuplicateSitemapId)
    ));
    let foreign = SitemapFile::new(
        "foreign",
        [SitemapUrl::new("https://other.example/").unwrap()],
    )
    .unwrap();
    assert!(matches!(
        configured().with_sitemaps([foreign]).try_build(),
        Err(DiscoveryError::CrossOrigin)
    ));
}

#[tokio::test]
async fn sitemap_setters_replace_previous_configuration() {
    let app = builder()
        .with_public_origin("https://example.com")
        .unwrap()
        .with_sitemaps(generate_sitemaps().await.unwrap())
        .with_sitemap([SitemapUrl::new("/single").unwrap()])
        .build();
    assert!(fetch(&app, "/sitemap.xml")
        .await
        .2
        .contains("<loc>https://example.com/single</loc>"));
    assert_eq!(
        fetch(&app, "/sitemap/pages.xml").await.0,
        StatusCode::NOT_FOUND
    );
    let app = builder()
        .with_public_origin("https://example.com")
        .unwrap()
        .with_sitemap([SitemapUrl::new("/single").unwrap()])
        .with_sitemaps(generate_sitemaps().await.unwrap())
        .build();
    assert!(fetch(&app, "/sitemap.xml")
        .await
        .2
        .contains("<sitemapindex"));
    assert_eq!(fetch(&app, "/sitemap/pages.xml").await.0, StatusCode::OK);
    assert!(!fetch(&app, "/sitemap.xml").await.2.contains("/single"));
}

async fn fetch(app: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header(header::HOST, "untrusted.example")
                .header("x-forwarded-host", "forwarded.example")
                .header("x-forwarded-prefix", "/untrusted-prefix")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn builder_uses_one_origin_and_mount_for_both_documents() {
    let app = builder()
        .with_public_origin("https://example.com/")
        .unwrap()
        .with_base_path(BasePath::new("/apps/demo").unwrap())
        .with_sitemap(entries())
        .with_robots(robots())
        .with_dynamic_routes(Router::new().route("/pricing", get(|| async { "pricing" })))
        .try_build()
        .unwrap();

    let (status, headers, xml) = fetch(&app, "/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/xml; charset=utf-8"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(xml.contains("<loc>https://example.com/apps/demo/</loc>"));
    assert!(xml.contains("<loc>https://example.com/apps/demo/pricing?q=1&amp;lang=en</loc>"));
    assert!(xml.contains("<loc>https://example.com:443/docs</loc>"));
    assert!(xml.contains("<lastmod>2026-10-25</lastmod>"));
    assert!(xml.contains("<changefreq>weekly</changefreq>"));
    assert!(xml.contains("<priority>0.5</priority>"));
    assert_eq!(xml.matches("<url>").count(), 3);

    let (status, headers, text) = fetch(&app, "/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(
        text,
        "User-agent: *\nAllow: /apps/demo$\nAllow: /apps/demo/\nDisallow: /apps/demo/admin\n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
    );
    assert_eq!(fetch(&app, "/pricing").await.0, StatusCode::OK);
    // Incoming routes stay unprefixed behind a prefix-stripping proxy.
    assert_eq!(
        fetch(&app, "/apps/demo/sitemap.xml").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fetch(&app, "/apps/demo/robots.txt").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn discovery_uses_final_configuration_independently_of_setter_order() {
    let early = builder()
        .with_public_origin("https://example.com")
        .unwrap()
        .with_base_path(BasePath::new("/final").unwrap())
        .with_sitemap(entries())
        .with_robots(robots())
        .try_build()
        .unwrap();
    let late = builder()
        .with_public_origin("https://old.example")
        .unwrap()
        .with_base_path(BasePath::new("/old").unwrap())
        .with_robots(robots())
        .with_sitemap(entries())
        .with_base_path(BasePath::new("/final").unwrap())
        .with_public_origin("https://example.com")
        .unwrap()
        .try_build()
        .unwrap();
    for path in ["/sitemap.xml", "/robots.txt"] {
        assert_eq!(fetch(&early, path).await, fetch(&late, path).await);
    }
}

#[tokio::test]
async fn discovery_routes_are_individually_opt_in() {
    let plain = builder().build();
    assert_eq!(fetch(&plain, "/sitemap.xml").await.0, StatusCode::NOT_FOUND);
    assert_eq!(fetch(&plain, "/robots.txt").await.0, StatusCode::NOT_FOUND);

    let sitemap_only = builder()
        .with_public_origin("https://example.com")
        .unwrap()
        .with_sitemap(entries())
        .build();
    assert_eq!(fetch(&sitemap_only, "/sitemap.xml").await.0, StatusCode::OK);
    assert_eq!(
        fetch(&sitemap_only, "/robots.txt").await.0,
        StatusCode::NOT_FOUND
    );

    let robots_only = builder().with_robots(robots()).try_build().unwrap();
    assert_eq!(
        fetch(&robots_only, "/sitemap.xml").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fetch(&robots_only, "/robots.txt").await.2,
        "User-agent: *\nAllow: /\nDisallow: /admin\n"
    );
}

#[tokio::test]
async fn builder_preserves_special_rules_groups_and_additional_sitemaps() {
    let robots = RobotsTxt::new()
        .group("*")
        .unwrap()
        .disallow("")
        .unwrap()
        .disallow("*.pdf$")
        .unwrap()
        .group("PrivateBot")
        .unwrap()
        .disallow("/")
        .unwrap()
        .group("PublicBot")
        .unwrap()
        .sitemap("https://cdn.example/news.xml")
        .unwrap()
        .sitemap("https://example.com/mount/sitemap.xml")
        .unwrap();
    let app = builder()
        .with_public_origin("https://example.com")
        .unwrap()
        .with_sitemap(entries())
        .with_robots(robots)
        .with_base_path(BasePath::new("/mount").unwrap())
        .build();
    let body = fetch(&app, "/robots.txt").await.2;
    assert_eq!(
        body.matches("Sitemap: https://example.com/mount/sitemap.xml")
            .count(),
        1
    );
    assert!(body.contains("Disallow: \nDisallow: *.pdf$\n"));
    assert!(body.contains("User-agent: PrivateBot\nDisallow: /mount$\nDisallow: /mount/\n"));
    assert!(body.contains("User-agent: PublicBot\nDisallow:\n"));
    assert!(body.contains("Sitemap: https://cdn.example/news.xml\n"));
}

#[tokio::test]
async fn builder_does_not_prefix_rules_twice() {
    let base = BasePath::new("/apps/demo").unwrap();
    let app = builder()
        .with_base_path(base.clone())
        .with_robots(robots().with_base_path(base))
        .try_build()
        .unwrap();
    assert_eq!(
        fetch(&app, "/robots.txt").await.2,
        "User-agent: *\nAllow: /apps/demo$\nAllow: /apps/demo/\nDisallow: /apps/demo/admin\n"
    );
}

#[tokio::test]
async fn discovery_supports_head_and_keeps_framework_security_headers() {
    let app = builder()
        .with_public_origin("https://example.com")
        .unwrap()
        .with_sitemap(entries())
        .with_robots(robots())
        .build();
    for path in ["/sitemap.xml", "/robots.txt"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert!(to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty());
    }
}

#[test]
fn builder_reports_missing_or_invalid_public_origin() {
    assert!(matches!(
        builder().with_sitemap(entries()).try_build(),
        Err(DiscoveryError::MissingPublicOrigin)
    ));
    assert!(matches!(
        builder().with_public_origin("https://example.com/path"),
        Err(DiscoveryError::InvalidOrigin)
    ));
}

#[test]
fn builder_rejects_cross_origin_entries_after_final_configuration() {
    assert!(matches!(
        builder()
            .with_sitemap([SitemapUrl::new("https://other.example/page").unwrap()])
            .with_public_origin("https://example.com")
            .unwrap()
            .try_build(),
        Err(DiscoveryError::CrossOrigin)
    ));
}

#[test]
fn builder_validates_composed_urls_before_serving() {
    let long_path = format!("/{}", "x".repeat(2000));
    assert!(matches!(
        builder()
            .with_public_origin("https://example.com")
            .unwrap()
            .with_sitemap([SitemapUrl::new(long_path).unwrap()])
            .with_base_path(BasePath::new(&format!("/{}", "p".repeat(100))).unwrap())
            .try_build(),
        Err(DiscoveryError::LocTooLong)
    ));
}
