// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use proa_core::{
    render_in_context_async, CacheHint, CacheMode, DataLoader, LoadError, LoadHints, LoadKey,
    LoadRequest, LoadValue, RenderMode, RenderOutcome, StandardWebContext, WebContext, WebRender,
    WriteError,
};
use proa_macros::html;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Product {
    title: String,
    price_cents: u32,
}

struct TestLoader;

impl DataLoader for TestLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let key = request.key().clone();
        async move {
            if key == LoadKey::named("product", "sku_1") {
                Ok(LoadValue::bytes(Bytes::from_static(
                    br#"{"title":"Trail Shoe","price_cents":12900}"#,
                )))
            } else {
                Err(LoadError::custom(format!("unexpected key: {key}")))
            }
        }
    }
}

struct ProductPage;

impl<L: DataLoader> WebRender<L> for ProductPage {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let product: Product = cx.load_json(LoadKey::named("product", "sku_1")).await?;
            let price_cents = product.price_cents.to_string();
            html! {
                <section data-product>
                    <h1>{product.title.as_str()}</h1>
                    <span>{price_cents.as_str()}</span>
                </section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[tokio::test]
async fn load_without_loader_returns_no_loader_error() {
    let mut out = Vec::new();
    let cx = WebContext::with_buffer(&mut out);
    let err = cx
        .load_json::<Product>(LoadKey::named("product", "missing"))
        .await
        .unwrap_err();
    assert_eq!(err, LoadError::NoLoader);
}

#[tokio::test]
async fn render_in_context_async_can_use_loader_and_render_mode() {
    let mut cx: StandardWebContext<TestLoader> =
        StandardWebContext::with_loader(Arc::new(TestLoader))
            .with_render_mode(RenderMode::ChunkedInOrder);

    render_in_context_async(&mut cx, ProductPage).await.unwrap();

    assert_eq!(cx.render_mode(), RenderMode::ChunkedInOrder);
    assert_eq!(
        String::from_utf8(cx.into_output().into_vec()).unwrap(),
        "<section data-product><h1>Trail Shoe</h1><span>12900</span></section>"
    );
}

struct HintCapturingLoader {
    seen: Arc<Mutex<Vec<LoadRequest>>>,
}

impl DataLoader for HintCapturingLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let seen = Arc::clone(&self.seen);
        let request = request.clone();
        async move {
            seen.lock().expect("seen mutex poisoned").push(request);
            Ok(LoadValue::bytes(Bytes::from_static(
                br#"{"title":"Trail Shoe","price_cents":12900}"#,
            )))
        }
    }
}

#[tokio::test]
async fn load_with_passes_advisory_cache_hints_to_loader() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut out = Vec::new();
    let cx = WebContext::with_loader_buffer(
        &mut out,
        Arc::new(HintCapturingLoader {
            seen: Arc::clone(&seen),
        }),
    );

    let _: Product = cx
        .load_json_with(
            LoadKey::named("product", "sku_1"),
            LoadHints::new()
                .with_cache(CacheHint::ttl(Duration::from_secs(30)).with_mode(CacheMode::Refresh)),
        )
        .await
        .unwrap();

    let seen = seen.lock().expect("seen mutex poisoned");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].key(), &LoadKey::named("product", "sku_1"));
    let cache = seen[0].hints().cache().expect("expected cache hint");
    assert_eq!(cache.ttl_hint(), Some(Duration::from_secs(30)));
    assert_eq!(cache.mode(), CacheMode::Refresh);
}

#[derive(Debug, PartialEq, Eq)]
struct ProductBundle {
    title: String,
}

struct TypedLoader;

impl DataLoader for TypedLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let key = request.key().clone();
        async move {
            if key == LoadKey::named("product_bundle", "sku_1") {
                Ok(LoadValue::typed(ProductBundle {
                    title: "Trail Shoe".to_string(),
                }))
            } else {
                Err(LoadError::custom(format!("unexpected key: {key}")))
            }
        }
    }
}

#[tokio::test]
async fn load_typed_returns_arc_to_materialized_value() {
    let mut out = Vec::new();
    let cx = WebContext::with_loader_buffer(&mut out, Arc::new(TypedLoader));

    let bundle = cx
        .load_typed::<ProductBundle>(LoadKey::named("product_bundle", "sku_1"))
        .await
        .unwrap();

    assert_eq!(bundle.title, "Trail Shoe");
}

#[tokio::test]
async fn load_typed_reports_type_mismatch() {
    let mut out = Vec::new();
    let cx = WebContext::with_loader_buffer(&mut out, Arc::new(TypedLoader));

    let err = cx
        .load_typed::<Product>(LoadKey::named("product_bundle", "sku_1"))
        .await
        .unwrap_err();

    assert!(matches!(err, LoadError::TypeMismatch { .. }));
}
