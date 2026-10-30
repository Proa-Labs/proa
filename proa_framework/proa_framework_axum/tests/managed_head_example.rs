// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The pull-request walkthrough, exercised through the real Axum router.
//!
//! The three blocks below are the PR description's Rust blocks, in order.
//! Keep them in sync with `proa_marketing/src/docs/framework/metadata.mdx`.
//! The test at the end checks layout defaults, the Page override, the
//! stylesheet slot replacement, the body output, and one product lookup per
//! request.

// 1. Configure trusted defaults once.
use axum::{extract::Path, routing::get, Router};
use proa_core::{
    ComponentRole, DataLoader, HeadConfig, HeadMedia, HeadUrlPolicy, LoadKey, OpenGraph,
    RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError,
};
use proa_framework_axum::{FrameworkBuilder, IslandManifest, RouteResponse};
use proa_macros::{html, html_sync};
use proa_stream::DataResolver;
use std::{future::Future, sync::Arc};

#[derive(PartialEq, Eq)]
enum Style {
    Theme,
}

// Call once at startup with your configured production or staging origin.
fn shop_head(public_origin: &str) -> Result<HeadConfig, WriteError> {
    HeadConfig::default()
        .with_urls(HeadUrlPolicy::new(public_origin, "/")?)
        .with_defaults(|head| {
            head.title_template("%s | ORIGIN")?;
            head.stylesheet(Style::Theme, "/static/base.css")?;
            Ok(())
        })
}

// 2. Load the product in the Page's head and body.
struct Product {
    name: String,
    summary: String,
    image_path: String,
}

fn product_key(id: u64) -> LoadKey {
    LoadKey::named("product", id.to_string())
}

struct ProductPage {
    id: u64,
}

impl<L: DataLoader> WebRender<L> for ProductPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let product = cx.load_typed::<Product>(product_key(self.id)).await?;
        cx.head_mut().title(product.name.clone())?;
        cx.head_mut().description(product.summary.clone())?;
        cx.head_mut().canonical(&format!("/products/{}", self.id))?;
        cx.head_mut().open_graph(
            OpenGraph::new()
                .kind("product")
                .images([HeadMedia::new(product.image_path.clone())
                    .dimensions(1200, 630)
                    .alt(product.name.clone())]),
        )?;
        Ok(())
    }

    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            // Same key + request-scoped resolver: reuse the head's loaded value.
            let product = cx.load_typed::<Product>(product_key(self.id)).await?;
            let body = html_sync! {
                <article>
                    <h1>{product.name.as_str()}</h1>
                    <p>{product.summary.as_str()}</p>
                </article>
            };
            WebRenderSync::render(body, cx)
        })
    }
}

// 3. Add the layout and expose `/products/:id`.
struct ShopLayout {
    page: ProductPage,
}

impl<L: DataLoader> WebRender<L> for ShopLayout {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        cx.head_mut().title("Shop")?;
        cx.head_mut().description("Browse our products.")?;
        // Replace the app's Theme slot; do not emit a second stylesheet.
        cx.head_mut().stylesheet(Style::Theme, "/static/shop.css")?;
        Ok(())
    }

    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let head = cx.head();
        html! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    {head}
                </head>
                <body>{self.page}</body>
            </html>
        }
        .render(cx)
    }
}

fn shop_router<L: DataLoader + 'static>(
    manifest: IslandManifest,
    catalog: Arc<L>,
    public_origin: &str,
) -> Result<Router, WriteError> {
    let head = shop_head(public_origin)?;
    let routes = Router::new().route(
        "/products/:id",
        get(move |Path(id): Path<u64>| {
            // Fresh cache for each request; the catalog itself can be shared.
            let loader = DataResolver::request_scope(Arc::clone(&catalog));
            async move {
                RouteResponse::ssr_stream_with_loader(
                    ShopLayout {
                        page: ProductPage { id },
                    },
                    loader,
                )
            }
        }),
    );
    Ok(FrameworkBuilder::new(manifest)
        .with_head(head)
        .with_dynamic_routes(routes)
        .build())
}

// Test support: a catalog that counts underlying lookups.
#[derive(Default)]
struct Catalog {
    lookups: std::sync::atomic::AtomicUsize,
}

impl DataLoader for Catalog {
    fn load(
        &self,
        request: &proa_core::LoadRequest,
    ) -> impl Future<Output = Result<proa_core::LoadValue, proa_core::LoadError>> + Send {
        self.lookups
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let key = request.key().clone();
        async move {
            if key == product_key(42) {
                Ok(proa_core::LoadValue::typed(Product {
                    name: "Trail Jacket".to_owned(),
                    summary: "A waterproof shell.".to_owned(),
                    image_path: "/static/trail-jacket.png".to_owned(),
                }))
            } else {
                Err(proa_core::LoadError::custom(format!(
                    "unexpected key: {key}"
                )))
            }
        }
    }
}

async fn fetch(app: &Router, uri: &str) -> String {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let response = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
    assert!(response.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn pr_example_resolves_layout_defaults_page_overrides_and_one_lookup_per_request() {
    let manifest: IslandManifest = serde_json::from_str(r#"{"islands":{}}"#).unwrap();
    let catalog = Arc::new(Catalog::default());
    let app = shop_router(
        manifest,
        Arc::clone(&catalog),
        "https://staging.example.com",
    )
    .unwrap();

    let html = fetch(&app, "/products/42").await;
    let head_end = html.find("</head>").expect("document head");
    let (head, body) = html.split_at(head_end);
    assert!(head.starts_with("<!DOCTYPE html>"), "{head}");
    assert!(head.contains(r#"<html lang="en">"#), "{head}");
    for expected in [
        "<title>Trail Jacket | ORIGIN</title>",
        r#"<meta name="description" content="A waterproof shell.">"#,
        r#"<link rel="canonical" href="https://staging.example.com/products/42">"#,
        r#"<meta property="og:title" content="Trail Jacket | ORIGIN">"#,
        r#"<meta property="og:description" content="A waterproof shell.">"#,
        r#"<meta property="og:url" content="https://staging.example.com/products/42">"#,
        r#"<meta property="og:type" content="product">"#,
        r#"<meta property="og:image" content="https://staging.example.com/static/trail-jacket.png">"#,
        r#"<meta property="og:image:alt" content="Trail Jacket">"#,
        r#"<meta property="og:image:width" content="1200">"#,
        r#"<meta property="og:image:height" content="630">"#,
        r#"<link rel="stylesheet" href="https://staging.example.com/static/shop.css">"#,
    ] {
        assert!(head.contains(expected), "missing {expected} in:\n{head}");
    }
    assert_eq!(
        head.matches("<link rel=\"stylesheet\"").count(),
        1,
        "the layout replaces the Theme slot instead of adding a second stylesheet:\n{head}"
    );
    assert!(!html.contains("base.css"), "{html}");
    assert!(
        body.contains("<article><h1>Trail Jacket</h1><p>A waterproof shell.</p></article>"),
        "{body}"
    );
    assert_eq!(
        catalog.lookups.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "head and body share one underlying lookup"
    );

    let again = fetch(&app, "/products/42").await;
    assert_eq!(again, html);
    assert_eq!(
        catalog.lookups.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "a fresh request gets a fresh resolver"
    );
}
