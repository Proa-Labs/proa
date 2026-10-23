// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
// The first three product blocks in docs/framework/metadata.mdx, compiled together.

#[rustfmt::skip]
mod product {
use std::future::Future;

use proa_core::{
    DataLoader, RenderOutcome, WebContext, WebRender,
    WebRenderSync, WriteError,
};
use proa_framework_axum::Metadata;
use proa_macros::html_sync;

pub struct ProductPage {
    pub id: String,
}

impl<L: DataLoader> WebRender<L> for ProductPage {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            // 1. Resolve the page's data before emitting its head.
            let product: Product =
                cx.load_json(product_key(&self.id)).await?;

            // 2. Use the loaded product to declare metadata.
            cx.metadata(
                Metadata::new()
                    .title(product.name.clone())
                    .description(product.summary.clone())
                    .canonical(product.canonical_url.clone())
                    .open_graph(|og| {
                        og.ty("website");
                        og.title(product.name.clone());
                        og.description(product.summary.clone());
                        og.url(product.canonical_url.clone());
                        og.image(product.social_image.clone());
                    }),
            );

            // 3. Emit the head, then render the same product in the body.
            WebRenderSync::render(html_sync! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8" />
                        <meta
                            name="viewport"
                            content="width=device-width, initial-scale=1"
                        />
                        {cx.metadata.head()}
                    </head>
                    <body>
                        <main>
                            <h1>{product.name.as_str()}</h1>
                            <p>{product.summary.as_str()}</p>
                        </main>
                    </body>
                </html>
            }, cx)
        })
    }
}

use bytes::Bytes;
use proa_core::{LoadError, LoadKey, LoadRequest, LoadValue};
use serde::Deserialize;

#[derive(Deserialize)]
struct Product {
    name: String,
    summary: String,
    canonical_url: String,
    social_image: String,
}

fn product_key(id: &str) -> LoadKey {
    LoadKey::named("product", id)
}

pub struct CatalogLoader;

impl DataLoader for CatalogLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let key = request.key().to_string();

        async move {
            if key != "product:atlas-jacket" {
                return Err(LoadError::custom(format!(
                    "No fixture for load key: {key}"
                )));
            }

            const PRODUCT_JSON: &[u8] = br#"{
                "name": "Atlas Jacket",
                "summary": "A lightweight, waterproof jacket.",
                "canonical_url": "https://example.com/products/atlas-jacket",
                "social_image": "https://example.com/og/atlas-jacket.png"
            }"#;

            Ok(LoadValue::bytes(Bytes::from_static(PRODUCT_JSON)))
        }
    }
}

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use proa_framework_axum::RouteResponse;
use proa_stream::DataResolver;

async fn product_handler(
    Path(id): Path<String>,
    State(origin): State<Arc<CatalogLoader>>,
) -> impl IntoResponse {
    let loader = DataResolver::request_scope(origin);

    RouteResponse::ssr_async_with_loader(ProductPage { id }, loader)
}

pub fn routes(origin: Arc<CatalogLoader>) -> Router {
    Router::new()
        .route("/products/:id", get(product_handler))
        .with_state(origin)
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
    let app = proa_framework_axum::FrameworkBuilder::new(manifest)
        .with_dynamic_routes(routes(Arc::new(CatalogLoader)))
        .build();
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
    assert!(output.contains("<title>Atlas Jacket</title>"));
    assert!(output.contains("<h1>Atlas Jacket</h1>"));
    assert!(output.contains("rel=\"canonical\" href=\"https://example.com/products/atlas-jacket\""));
    assert_eq!(output.matches("property=\"og:image\"").count(), 1);
}

#[test]
fn fixture_matches_documented_product_blocks() {
    let docs = include_str!("../../../proa_marketing/src/docs/framework/metadata.mdx");
    let fixture = include_str!("metadata_docs.rs");
    let mut blocks = docs.split("```rust\n").skip(2);
    for _ in 0..3 {
        let block = blocks.next().unwrap().split("\n```").next().unwrap();
        assert!(
            fixture.contains(block),
            "update compiled fixture when the product example changes"
        );
    }
}
