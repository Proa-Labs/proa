// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{routing::get, Router};
use proa_core::{DataLoader, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError};
use proa_framework_axum::{
    router_from_spec, serve_with_shutdown, IslandManifest, Route, RouteContextLayer, RouteResponse,
    ShutdownOutcome,
};
use proa_macros::{html, html_sync};
use proa_stream::Suspense;
use tokio::sync::{oneshot, Notify};

#[derive(Clone)]
struct SlowRequestState {
    started: Arc<Notify>,
    finish: Arc<Notify>,
}

struct NeverBoundary {
    started: Arc<Notify>,
    dropped: Arc<Notify>,
}

struct DropNotification(Arc<Notify>);

impl Drop for DropNotification {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

impl<B, L> WebRender<L, B> for NeverBoundary
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        _cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _drop_notification = DropNotification(self.dropped);
            self.started.notify_one();
            std::future::pending::<Result<(), WriteError>>().await
        })
    }
}

fn empty_manifest() -> Arc<IslandManifest> {
    Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_deadline_starts_after_signal_and_drains_an_ordinary_request() {
    let state = SlowRequestState {
        started: Arc::new(Notify::new()),
        finish: Arc::new(Notify::new()),
    };
    let handler_state = state.clone();
    let app = Router::new().route(
        "/slow",
        get(move || {
            let state = handler_state.clone();
            async move {
                state.started.notify_one();
                state.finish.notified().await;
                "finished"
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let drain_timeout = Duration::from_millis(100);
    let server = tokio::spawn(serve_with_shutdown(
        listener,
        app,
        drain_timeout,
        async move {
            let _ = shutdown_rx.await;
        },
    ));

    let request = tokio::spawn(async move {
        reqwest::get(format!("http://{address}/slow"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(1), state.started.notified())
        .await
        .expect("slow request should start");

    // If the drain deadline started with the server, this request would
    // already be force-cancelled before the signal below.
    tokio::time::sleep(drain_timeout + Duration::from_millis(30)).await;
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_millis(50), async {
        while let Ok(stream) = tokio::net::TcpStream::connect(address).await {
            drop(stream);
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the listener should stop admitting connections after shutdown");
    state.finish.notify_one();

    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), request)
            .await
            .expect("ordinary request should drain")
            .unwrap(),
        "finished"
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("server should finish after the request")
            .unwrap()
            .unwrap(),
        ShutdownOutcome::Drained
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_deadline_cancels_a_stalled_streaming_boundary() {
    let boundary_started = Arc::new(Notify::new());
    let boundary_dropped = Arc::new(Notify::new());
    let route_started = Arc::clone(&boundary_started);
    let route_dropped = Arc::clone(&boundary_dropped);
    let routes = vec![Route::endpoint(
        "/stalled",
        get(move || {
            let started = Arc::clone(&route_started);
            let dropped = Arc::clone(&route_dropped);
            async move {
                RouteResponse::ssr_stream(html! {
                    <main>
                        {Suspense::new(
                            NeverBoundary { started, dropped },
                            html_sync! { <p>"waiting"</p> },
                        )}
                    </main>
                })
            }
        }),
    )];
    let (app, computed_metadata) = router_from_spec(routes);
    let app = app.layer(RouteContextLayer::new(empty_manifest(), computed_metadata));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let drain_timeout = Duration::from_millis(50);
    let server = tokio::spawn(serve_with_shutdown(
        listener,
        app,
        drain_timeout,
        async move {
            let _ = shutdown_rx.await;
        },
    ));

    let mut response = tokio::time::timeout(
        Duration::from_secs(1),
        reqwest::get(format!("http://{address}/stalled")),
    )
    .await
    .expect("the streamed response should start")
    .unwrap();
    assert!(response.status().is_success());
    tokio::time::timeout(Duration::from_secs(1), boundary_started.notified())
        .await
        .expect("the streamed boundary should be pending");
    let first_chunk = tokio::time::timeout(Duration::from_secs(1), response.chunk())
        .await
        .expect("the streamed shell should arrive")
        .unwrap();
    assert!(first_chunk.is_some());

    let signalled_at = Instant::now();
    shutdown_tx.send(()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("bounded shutdown should not wait forever")
            .unwrap()
            .unwrap(),
        ShutdownOutcome::DeadlineExceeded
    );
    assert!(
        signalled_at.elapsed() >= drain_timeout,
        "the active stream must receive its full drain window"
    );
    tokio::time::timeout(Duration::from_secs(1), boundary_dropped.notified())
        .await
        .expect("aborting the HTTP body should cancel the pending boundary");
}
