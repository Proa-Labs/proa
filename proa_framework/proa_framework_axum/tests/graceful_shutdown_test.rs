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

#[tokio::test]
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
    let drain_timeout = Duration::from_secs(30);
    let io_timeout = Duration::from_secs(5);
    let server = serve_with_shutdown(listener, app, drain_timeout, async move {
        let _ = shutdown_rx.await;
    });
    tokio::pin!(server);

    let request = tokio::spawn(async move {
        reqwest::get(format!("http://{address}/slow"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });
    tokio::time::timeout(io_timeout, async {
        tokio::select! {
            () = state.started.notified() => {}
            result = &mut server => panic!("server stopped before shutdown: {result:?}"),
        }
    })
    .await
    .expect("slow request should start");

    // Advance only the server's deadline checks with virtual time. Real HTTP
    // setup and draining run with the clock resumed so idle I/O cannot
    // automatically advance a paused clock past the drain deadline.
    tokio::time::pause();
    tokio::time::advance(drain_timeout + Duration::from_secs(1)).await;
    assert!(
        futures::poll!(&mut server).is_pending(),
        "the drain deadline must not stop the server before shutdown"
    );
    shutdown_tx.send(()).unwrap();
    // Poll the ready shutdown signal through listener closure before probing
    // the socket. This avoids racing a spawned server against a 50 ms timer.
    assert!(
        futures::poll!(&mut server).is_pending(),
        "the in-flight request must receive a new drain window after shutdown"
    );
    tokio::time::resume();
    assert!(
        tokio::time::timeout(io_timeout, tokio::net::TcpStream::connect(address))
            .await
            .expect("the listener closure should be observable")
            .is_err(),
        "the listener must stop admitting connections while the request drains"
    );
    state.finish.notify_one();

    let (outcome, response) =
        tokio::time::timeout(io_timeout, async { tokio::join!(&mut server, request) })
            .await
            .expect("server and ordinary request should drain");
    assert_eq!(response.unwrap(), "finished");
    assert_eq!(outcome.unwrap(), ShutdownOutcome::Drained);
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
