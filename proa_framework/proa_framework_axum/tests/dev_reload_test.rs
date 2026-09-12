// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end coverage for `proa dev` live-reload injection over real HTTP.
//!
//! The unit tests exercise the body wrapper directly. This one runs a real
//! hyper server through [`proa_framework_axum::serve_with_shutdown`], because the
//! part most likely to break in the wild is framing: the layer removes
//! `content-length` from a body it grows, and hyper has to fall back to
//! chunked transfer without truncating or hanging.
//!
//! This file holds exactly one test on purpose. It sets a process-wide
//! environment variable, and Cargo gives each integration test file its own
//! process, so nothing else can observe the mutation.

use std::time::Duration;

use axum::{routing::get, Json, Router};
use proa_framework_axum::{serve_with_shutdown, RELOAD_PORT_ENV, RELOAD_TOKEN_ENV};

const TOKEN: &str = "00112233445566778899aabbccddeeff";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proa_dev_injects_the_live_reload_client_into_html_over_real_http() {
    // Only `proa dev` sets these two. Both are required, so the test states
    // the activation contract exactly the way the CLI does.
    std::env::set_var(RELOAD_PORT_ENV, "45999");
    std::env::set_var(RELOAD_TOKEN_ENV, TOKEN);

    let app = Router::new()
        .route(
            "/",
            get(|| async {
                axum::response::Html(
                    "<!DOCTYPE html><html><head><title>site</title></head><body>hi</body></html>",
                )
            }),
        )
        .route(
            "/api",
            get(|| async { Json(serde_json::json!({"ok": true})) }),
        );

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        serve_with_shutdown(listener, app, Duration::from_secs(5), async {
            let _ = stopped.await;
        })
        .await
    });

    let client = reqwest::Client::new();
    let page = client
        .get(format!("http://{address}/"))
        .send()
        .await
        .expect("request the page");

    // A grown body cannot keep a declared length; hyper must frame it chunked.
    assert!(
        page.headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .is_none(),
        "{:?}",
        page.headers()
    );
    let html = page.text().await.expect("read the page");
    let script = html.find("<script>").expect("client injected");
    assert!(script < html.find("</head>").unwrap(), "{html}");
    assert!(html.contains("__proaLiveReload"), "{html}");
    assert!(html.contains("45999"), "{html}");
    assert!(html.contains(TOKEN), "{html}");
    assert!(html.ends_with("<body>hi</body></html>"), "{html}");

    // A port on its own must never activate dev tooling, or a variable left
    // exported in a shell profile would follow a release binary into
    // production and start rewriting every HTML response.
    std::env::remove_var(RELOAD_TOKEN_ENV);
    assert!(proa_framework_axum::dev_reload::layer_from_env().is_none());
    std::env::set_var(RELOAD_TOKEN_ENV, TOKEN);

    // Data endpoints are untouched, so an injected script can never end up
    // inside a JSON payload.
    let api = client
        .get(format!("http://{address}/api"))
        .send()
        .await
        .expect("request the API");
    assert_eq!(api.text().await.unwrap(), r#"{"ok":true}"#);

    let _ = stop.send(());
    server.await.unwrap().unwrap();
}
