// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Exact complete product example from docs/framework/metadata.mdx.
#[rustfmt::skip]
mod product {
use std::{future::Future, sync::Arc};
use proa_core::{DataLoader, Metadata, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa_macros::html_sync;

pub struct ProductPage { pub id: String }

impl<L: DataLoader> WebRender<L> for ProductPage {
    fn render(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let product = cx.load_typed::<Product>(product_key(&self.id)).await?;
            PreparedProductPage { product }.render(cx)
        })
    }
}

// This is the page's preparation step. The document wrapper applies no defaults.
struct PreparedProductPage { product: Arc<Product> }
impl<L: DataLoader> WebRenderSync<L> for PreparedProductPage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        cx.metadata(product_metadata(&self.product))?;
        RootDocument { content: ProductBody { product: self.product } }.render(cx)
    }
}

fn product_metadata(product: &Product) -> Metadata {
    Metadata::new()
        .title(product.name.clone())
        .description(product.summary.clone())
        .canonical(product.canonical_url.clone())
        .open_graph(|og| {
            og.ty("website");
            og.images([product.social_image.clone()]);
        })
}

struct RootDocument<C> { content: C }
impl<L: DataLoader, C: WebRenderSync<L>> WebRenderSync<L> for RootDocument<C> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        WebRenderSync::render(html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    {cx.metadata.head()}
                </head>
                <body>{self.content}</body>
            </html>
        }, cx)
    }
}

struct ProductBody { product: Arc<Product> }
impl<L: DataLoader> WebRenderSync<L> for ProductBody {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        WebRenderSync::render(html_sync! {
            <main data-product>
                <h1>{self.product.name.as_str()}</h1>
                <p>{self.product.summary.as_str()}</p>
            </main>
        }, cx)
    }
}

use proa_core::{LoadError, LoadKey, LoadRequest, LoadValue};

struct Product {
    name: String,
    summary: String,
    canonical_url: String,
    social_image: String,
}
fn product_key(id: &str) -> LoadKey { LoadKey::named("product", id) }

pub struct CatalogLoader;
impl DataLoader for CatalogLoader {
    fn load(&self, request: &LoadRequest) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let key = request.key().to_string();
        async move {
            if key != "product:atlas-jacket" {
                return Err(LoadError::custom(format!("No fixture for load key: {key}")));
            }
            Ok(LoadValue::typed(Product {
                name: "Atlas Jacket".into(),
                summary: "A lightweight, waterproof jacket.".into(),
                canonical_url: "https://example.com/products/atlas-jacket".into(),
                social_image: "https://example.com/og/atlas-jacket.png".into(),
            }))
        }
    }
}

use axum::{extract::{Path, State}, response::IntoResponse, routing::get, Router};
use proa_core::DynLoaderHandle;
use proa_framework_axum::{router_from_spec, FrameworkBuilder, IslandManifest, Layout, Route, RouteResponse};
use proa_stream::DataResolver;

struct SiteLayout;
impl Layout for SiteLayout {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("Acme").title_template("%s · Acme").open_graph(|og| { og.site_name("Acme"); }))?;
        Ok(())
    }
}
struct CatalogLayout;
impl Layout for CatalogLayout {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("Catalog").title_template("%s · Acme Store").description("Acme catalog."))?;
        Ok(())
    }
}
async fn product_handler(Path(id): Path<String>, State(origin): State<Arc<CatalogLoader>>) -> impl IntoResponse {
    let loader = DataResolver::request_scope(origin);
    RouteResponse::ssr_async_with_loader(ProductPage { id }, loader)
}

pub fn app(manifest: IslandManifest, origin: Arc<CatalogLoader>) -> Router {
    let (router, setups) = router_from_spec(vec![Route::layout("/", &SiteLayout, vec![
        Route::layout("products", &CatalogLayout, vec![
            Route::endpoint(":id", get(product_handler)),
        ]),
    ])]);
    FrameworkBuilder::new(manifest)
        .with_dynamic_routes_and_setups(router.with_state(origin), setups)
        .build()
}

pub async fn product_stream_handler(
    Path(id): Path<String>,
    State(origin): State<Arc<CatalogLoader>>,
) -> Result<impl IntoResponse, axum::http::StatusCode> {
    use axum::http::StatusCode;
    if id != "atlas-jacket" { return Err(StatusCode::NOT_FOUND); }
    let loader = DataResolver::request_scope(origin);
    let product = loader.load_key(&product_key(&id)).await
        .and_then(LoadValue::downcast::<Product>)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(RouteResponse::ssr_stream_with_loader(
        proa_core::to_async(PreparedProductPage { product }), loader,
    ))
}
}
use product::*;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn documented_product_route_reuses_loaded_data_for_head_and_body() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let manifest = proa_framework_axum::IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    };
    let app = app(manifest, Arc::new(CatalogLoader));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/products/atlas-jacket")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let output = std::str::from_utf8(&bytes).unwrap();
    assert!(output.contains("<title>Atlas Jacket · Acme Store</title>"));
    assert!(output.contains("<h1>Atlas Jacket</h1>"));
    assert!(output.contains("rel=\"canonical\" href=\"https://example.com/products/atlas-jacket\""));
    assert_eq!(output.matches("property=\"og:image\"").count(), 1);
}

#[test]
fn fixture_matches_documented_product_blocks() {
    let docs = include_str!("../../../proa_marketing/src/docs/framework/metadata.mdx");
    let section = docs
        .split("## End-to-end:")
        .nth(1)
        .unwrap()
        .split("## Set layout defaults")
        .next()
        .unwrap();
    let fixture = include_str!("metadata_docs.rs");
    for block in section.split("```rust\n").skip(1) {
        assert!(
            fixture.contains(block.split("\n```").next().unwrap()),
            "update compiled fixture when examples change"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn documented_streaming_handler_decides_not_found_before_rendering() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        routing::get,
        Router,
    };
    use tower::ServiceExt;
    let manifest = proa_framework_axum::IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    };
    let routes = Router::new()
        .route("/products/:id", get(product_stream_handler))
        .with_state(Arc::new(CatalogLoader));
    let app = proa_framework_axum::FrameworkBuilder::new(manifest)
        .with_dynamic_routes(routes)
        .build();
    let not_found = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/products/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(not_found.status(), 404);
    assert!(
        !std::str::from_utf8(&to_bytes(not_found.into_body(), usize::MAX).await.unwrap())
            .unwrap()
            .contains("<title>")
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/products/atlas-jacket")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let output = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(std::str::from_utf8(&output)
        .unwrap()
        .contains("<title>Atlas Jacket</title>"));
}
