// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Exercise automatic build identity over real HTTP across separate processes.
//!
//! Build/run with PROA_PAGE_CACHE_PROBE_BUILD=a and save stdout as first.json.
//! Run the same build with `same-build first.json` to check a process restart.
//! Build/run with PROA_PAGE_CACHE_PROBE_BUILD=b and `new-build first.json` to
//! prove the changed executable rejects the old ETag even for identical HTML.
//! This is a validation fixture, not a deployment or cache-purge requirement.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use axum::{routing::get, Router};
use proa_framework_axum::{FrameworkBuilder, IslandManifest, PageCacheLayer, Route, RouteResponse};
use reqwest::{header, StatusCode};
use serde_json::json;

// Referencing this in the output changes the executable without changing the
// rendered page. That isolates build fingerprinting from content hashing.
const BUILD: &str = match option_env!("PROA_PAGE_CACHE_PROBE_BUILD") {
    Some(build) => build,
    None => "local",
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next();
    let previous = args
        .next()
        .map(
            |path| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
                Ok(serde_json::from_slice(&std::fs::read(path)?)?)
            },
        )
        .transpose()?;
    assert!(matches!(
        mode.as_deref(),
        None | Some("same-build" | "new-build")
    ));
    assert_eq!(mode.is_some(), previous.is_some());

    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let route = Route::page(
        "/promo/summer",
        "Promotion",
        get(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { RouteResponse::ssr(proa_core::text::text("identical page across builds")) }
        }),
    )
    .with_page_cache(PageCacheLayer::for_current_build()?);
    let app = FrameworkBuilder::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    })
    .with_dynamic_routes(Router::new().merge(route.into_router()))
    .build();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/promo/summer", listener.local_addr()?);
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    // Each process binds a different port. A fixed Host makes the cache-key
    // input identical, as it would be at the production domain after deploy.
    let request = || client.get(&url).header(header::HOST, "page-cache.test");
    let mut first_request = request();
    if let Some(previous) = &previous {
        first_request =
            first_request.header(header::IF_NONE_MATCH, previous["etag"].as_str().unwrap());
    }
    let first = first_request.send().await?;
    let first_status = first.status();
    let etag = first.headers()[header::ETAG].to_str()?.to_owned();
    assert_eq!(first.headers()["x-proa-page-cache"], "MISS");
    assert_eq!(first.headers()[header::CACHE_CONTROL], "public, no-cache");
    let first_body = first.text().await?;

    match mode.as_deref() {
        Some("same-build") => {
            assert_eq!(first_status, StatusCode::NOT_MODIFIED);
            assert_eq!(etag, previous.as_ref().unwrap()["etag"].as_str().unwrap());
            assert!(first_body.is_empty());
        }
        Some("new-build") => {
            assert_eq!(first_status, StatusCode::OK);
            assert_ne!(etag, previous.as_ref().unwrap()["etag"].as_str().unwrap());
            assert_eq!(
                first_body,
                previous.as_ref().unwrap()["body"].as_str().unwrap()
            );
        }
        None => assert_eq!(first_status, StatusCode::OK),
        _ => unreachable!(),
    }

    let hit = request().send().await?;
    assert_eq!(hit.status(), StatusCode::OK);
    assert_eq!(hit.headers()["x-proa-page-cache"], "HIT");
    let hit_body = hit.text().await?;
    let conditional = request()
        .header(header::IF_NONE_MATCH, &etag)
        .send()
        .await?;
    assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(conditional.headers()["x-proa-page-cache"], "HIT");
    assert!(conditional.bytes().await?.is_empty());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "cache hits must skip the page handler"
    );

    println!(
        "{}",
        json!({
            "build": BUILD,
            "mode": mode.as_deref().unwrap_or("initial"),
            "first_status": first_status.as_u16(),
            "first_cache": "MISS",
            "repeat_cache": "HIT",
            "conditional_status": 304,
            "handler_calls": calls.load(Ordering::SeqCst),
            "etag": etag,
            "body": hit_body,
        })
    );
    let _ = shutdown.send(());
    server.await??;
    Ok(())
}
