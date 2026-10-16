// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A separate process isolates the dev environment configuration from root tests.
use std::{convert::Infallible, time::Duration};

use axum::{
    body::Body,
    http::{header, Response},
    routing::get,
    Router,
};
use futures::StreamExt;
use proa_framework_axum::{
    serve_with_shutdown, ShutdownOutcome, RELOAD_PORT_ENV, RELOAD_TOKEN_ENV,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mounted_event_stream_does_not_delay_graceful_server_shutdown() {
    let upstream = Router::new().route(
        "/__proa/dev-events",
        get(|| async {
            let initial = futures::stream::iter([Ok::<_, Infallible>("data: ready\n\n")]);
            let stream = initial.chain(futures::stream::pending());
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let upstream = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    std::env::set_var(RELOAD_PORT_ENV, port.to_string());
    std::env::set_var(RELOAD_TOKEN_ENV, "0123456789abcdef0123456789abcdef");
    std::env::set_var("PROA_BASE_PATH", "/apps/demo");
    std::env::set_var("PROA_HOST", "127.0.0.1");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_with_shutdown(
        listener,
        Router::new(),
        Duration::from_secs(5),
        async {
            let _ = stopped.await;
        },
    ));
    // Like a prefix-stripping proxy, send the local route to the application.
    let mut events = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!(
            "http://{address}/__proa/dev-events?token=0123456789abcdef0123456789abcdef"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(events.status(), 200);
    assert_eq!(events.headers()["x-accel-buffering"], "no");
    let initial = tokio::time::timeout(Duration::from_secs(2), events.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(initial, "data: ready\n\n");
    shutdown.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("reload relay must close before the five-second drain deadline")
        .unwrap()
        .unwrap();
    assert_eq!(result, ShutdownOutcome::Drained);
    assert!(tokio::time::timeout(Duration::from_secs(2), events.chunk())
        .await
        .unwrap()
        .unwrap()
        .is_none());
    upstream.abort();
}
