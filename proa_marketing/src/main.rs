// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Marketing site server binary deployed through Railway.

use std::env;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::http::header::{HeaderValue, CACHE_CONTROL, LINK, X_CONTENT_TYPE_OPTIONS};
use axum::routing::{any, get, post};
use axum::Router;
use tower::ServiceBuilder;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod analytics;
mod components;
mod data;
mod endpoints;
mod layout;
mod pages;
mod static_assets;
mod utils;

#[cfg(test)]
mod agent_tests;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let app = app();

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(5555);
    let addr = listen_addr(&host, port)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!(
        "proa_marketing listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn listen_addr(host: &str, port: u16) -> anyhow::Result<SocketAddr> {
    Ok(SocketAddr::new(host.parse::<IpAddr>()?, port))
}

fn app() -> Router {
    let budget = Arc::new(Mutex::new(endpoints::api::RequestBudget::default()));
    let agent_api = Router::new()
        .route(
            "/api",
            get(endpoints::agent_discovery::site_metadata)
                .fallback(endpoints::api::method_not_allowed),
        )
        .route(
            "/api/v1",
            get(endpoints::agent_discovery::site_metadata)
                .fallback(endpoints::api::method_not_allowed),
        )
        .route(
            "/api/v1/search",
            get(endpoints::api::search_handler).fallback(endpoints::api::method_not_allowed),
        )
        .route(
            "/api/v1/docs",
            get(endpoints::api::list_handler).fallback(endpoints::api::method_not_allowed),
        )
        .route(
            "/api/v1/docs/page",
            get(endpoints::api::page_handler).fallback(endpoints::api::method_not_allowed),
        )
        .route("/api/v1/*path", any(endpoints::api::not_found))
        .route(
            "/api/search",
            get(endpoints::search::handler).post(endpoints::search::post_handler),
        )
        .route_service("/mcp", endpoints::mcp::service())
        .layer(axum::middleware::from_fn_with_state(
            budget,
            endpoints::api::rate_limit,
        ));
    Router::new()
        .route(
            "/",
            get(pages::home::handler).layer(SetResponseHeaderLayer::if_not_present(
                LINK,
                HeaderValue::from_static(endpoints::agent_discovery::DISCOVERY_LINKS),
            )),
        )
        .route("/showcase", get(pages::showcase::handler))
        .route("/ecosystem", get(pages::ecosystem::handler))
        .route("/ecosystem/apps", get(pages::ecosystem::apps_handler))
        .route(
            "/ecosystem/developer-tools",
            get(pages::ecosystem::developer_tools_handler),
        )
        .route(
            "/ecosystem/registry",
            get(pages::ecosystem::registry_handler),
        )
        .route("/privacy", get(pages::legal::privacy_handler))
        .route("/terms", get(pages::legal::terms_handler))
        .route("/about", get(pages::info::about))
        .route("/contact", get(pages::info::contact))
        .route(
            "/demos/quantity-stepper",
            get(pages::demos::quantity_stepper_handler),
        )
        .route(
            "/demos/quantity-stepper/cart",
            post(pages::demos::quantity_stepper_cart_handler),
        )
        .route("/docs", get(pages::docs::handler))
        .route("/docs/*path", get(pages::docs::path_handler))
        .route("/llms-full.txt", get(endpoints::llms::site_handler))
        .merge(endpoints::agent_discovery::router())
        .merge(agent_api)
        .route("/healthz", get(endpoints::health::healthz))
        .route("/readyz", get(endpoints::health::readyz))
        .route("/robots.txt", get(endpoints::robots::handler))
        .route("/sitemap.xml", get(endpoints::sitemap::handler))
        .route(
            "/schema/proa.config.json",
            get(endpoints::schemas::project_config),
        )
        .route(
            "/schema/proa.lock.json",
            get(endpoints::schemas::component_lock),
        )
        .route(
            "/schema/ui-component-manifest-v1.json",
            get(endpoints::schemas::ui_component_manifest),
        )
        .route("/install.sh", get(endpoints::install::shell_handler))
        .route("/install.ps1", get(endpoints::install::powershell_handler))
        // Carries its own state (release cache and HTTP client), so it arrives
        // as a router rather than as two bare handlers.
        .merge(endpoints::downloads::router())
        .merge(proa_demo::playground::marketing_router())
        .merge(proa_showcase_web::router())
        .nest_service("/static", static_service())
        .fallback(endpoints::not_found::handler)
        .layer(CompressionLayer::new().compress_when(compression_predicate()))
        .layer(TraceLayer::new_for_http())
}

/// The default predicate compresses `application/octet-stream`, which for this
/// site means re-compressing the already-compressed CLI archives the download
/// bridge streams. That costs real CPU per request on an unauthenticated route
/// and makes the payload larger, so those are excluded by content type.
fn compression_predicate() -> impl Predicate {
    DefaultPredicate::new().and(NotForContentType::const_new("application/octet-stream"))
}

fn static_service() -> Router {
    let static_dir = env::var_os("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public"));

    Router::new().nest_service(
        "/",
        ServiceBuilder::new()
            .layer(SetResponseHeaderLayer::if_not_present(
                CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ))
            .service(ServeDir::new(static_dir)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower_test::ServiceExt;

    #[test]
    fn listen_address_accepts_ipv4_and_ipv6_hosts() {
        assert_eq!(
            listen_addr("0.0.0.0", 5555).unwrap(),
            "0.0.0.0:5555".parse().unwrap()
        );
        assert_eq!(
            listen_addr("::", 5555).unwrap(),
            "[::]:5555".parse().unwrap()
        );
    }

    #[tokio::test]
    async fn root_advertises_and_serves_agent_discovery_documents() {
        let root = app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(root.status(), StatusCode::OK);
        assert_eq!(
            root.headers().get(LINK).unwrap(),
            endpoints::agent_discovery::DISCOVERY_LINKS
        );

        let metadata = app()
            .oneshot(
                Request::builder()
                    .uri("/.well-known/proa-site.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(metadata.status(), StatusCode::OK);
        assert_eq!(
            metadata
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "application/json; charset=utf-8"
        );
        let metadata = to_bytes(metadata.into_body(), usize::MAX).await.unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(&metadata).unwrap();
        assert_eq!(metadata["format"], "proa-site");

        let openapi = app()
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        let openapi = to_bytes(openapi.into_body(), usize::MAX).await.unwrap();
        let openapi: serde_json::Value = serde_json::from_slice(&openapi).unwrap();
        assert_eq!(openapi["openapi"], "3.1.0");
    }

    #[tokio::test]
    async fn twitter_card_advertises_a_fetchable_compressed_image() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("user-agent", "Twitterbot/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        let head = html.split_once("</head>").unwrap().0;
        assert!(head.contains(r#"name="twitter:card" content="summary_large_image""#));
        assert!(head.contains(r#"name="twitter:site" content="@drewhariri""#));
        assert!(head.contains(r#"property="og:site_name" content="Proa""#));
        let image = head
            .split_once(r#"name="twitter:image" content=""#)
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        let image_path = image
            .strip_prefix(utils::site_url::public_site_url())
            .expect("card image must use the deployment's absolute public URL");
        assert!(image_path.starts_with("/static/og.jpg?v="));
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(image_path)
                    .header("user-agent", "Twitterbot/1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "image/jpeg"
        );
        let image = to_bytes(response.into_body(), 500_000).await.unwrap();
        assert_eq!(image.as_ref(), include_bytes!("../public/og.jpg"));
    }

    #[tokio::test]
    async fn generated_project_schema_urls_are_served() {
        for (path, expected_id) in [
            (
                "/schema/proa.config.json",
                "https://proa.so/schema/proa.config.json",
            ),
            (
                "/schema/proa.lock.json",
                "https://proa.so/schema/proa.lock.json",
            ),
            (
                "/schema/ui-component-manifest-v1.json",
                "https://proa.so/schema/ui-component-manifest-v1.json",
            ),
        ] {
            let response = app()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response
                    .headers()
                    .get(axum::http::header::CONTENT_TYPE)
                    .unwrap(),
                "application/schema+json; charset=utf-8"
            );
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let schema: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(schema["$id"], expected_id, "{path}");
        }
    }

    /// A `.tar.gz` is already compressed. Re-compressing it burns CPU on every
    /// download and, for gzip, produces a body larger than the input, so the
    /// predicate must leave `application/octet-stream` alone while still
    /// compressing the HTML pages.
    #[tokio::test]
    async fn archives_are_not_recompressed_but_html_still_is() {
        use tower_http::compression::predicate::Predicate;

        let predicate = compression_predicate();
        // A real body, so the size floor in the default predicate is cleared
        // and the content type is what actually decides.
        let body = || Body::from(vec![b'x'; 64 * 1024]);

        let archive = axum::http::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
            .body(body())
            .unwrap();
        assert!(
            !predicate.should_compress(&archive),
            "the CLI archives must be streamed through untouched"
        );

        let page = axum::http::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(body())
            .unwrap();
        assert!(
            predicate.should_compress(&page),
            "pages must still be compressed"
        );
    }

    #[tokio::test]
    async fn installer_scripts_are_routed_with_their_own_content_types() {
        let shell = app()
            .oneshot(
                Request::builder()
                    .uri("/install.sh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(shell.status(), StatusCode::OK);
        assert_eq!(
            shell
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "text/x-shellscript; charset=utf-8"
        );

        let powershell = app()
            .oneshot(
                Request::builder()
                    .uri("/install.ps1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(powershell.status(), StatusCode::OK);
        assert_eq!(
            powershell
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    /// The malformed version is deliberate: the bridge rejects it before it
    /// would talk to GitHub, so this asserts the routes are registered without
    /// the test making a network call.
    #[tokio::test]
    async fn cli_download_routes_are_registered() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/downloads/cli/not-a-version/proa-cli.tar.gz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "application/json; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"], "not_found");
    }

    /// SEO auditors join every `<title>` in a document, inline SVG titles
    /// included, so each marketing shell must render only its head title.
    #[tokio::test]
    async fn marketing_shells_have_one_title_and_a_touch_icon() {
        for uri in [
            "/",
            "/docs",
            "/ecosystem",
            "/ecosystem/apps",
            "/ecosystem/developer-tools",
            "/ecosystem/registry",
            "/privacy",
            "/terms",
        ] {
            let response = app()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            let html = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let html = String::from_utf8(html.to_vec()).unwrap();
            assert_eq!(html.matches("<title>").count(), 1, "{uri}");
            assert!(
                html.contains(r#"<link rel="apple-touch-icon" sizes="180x180""#),
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn blog_is_not_publicly_routed_or_linked() {
        let blog = app()
            .oneshot(Request::builder().uri("/blog").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(blog.status(), StatusCode::NOT_FOUND);

        let root = app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let root = to_bytes(root.into_body(), usize::MAX).await.unwrap();
        let root = String::from_utf8(root.to_vec()).unwrap();
        assert!(!root.contains("href=\"/blog"));
    }

    #[tokio::test]
    async fn pricing_is_not_publicly_routed_or_linked() {
        let pricing = app()
            .oneshot(
                Request::builder()
                    .uri("/pricing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pricing.status(), StatusCode::NOT_FOUND);

        let root = app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let root = to_bytes(root.into_body(), usize::MAX).await.unwrap();
        let root = String::from_utf8(root.to_vec()).unwrap();
        assert!(!root.contains("href=\"/pricing"));
    }
}
