// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Signal-aware Axum serving with a bounded graceful-shutdown window.

use std::future::Future;
use std::io;
use std::time::Duration;

use axum::{body::Body, Router};
use hyper::body::Incoming;
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::{conn::auto::Builder, graceful::GracefulShutdown},
    service::TowerToHyperService,
};
use tokio::{net::TcpListener, task::JoinSet};
use tower::ServiceExt as _;

/// The generated production scaffold's graceful-drain deadline.
pub const DEFAULT_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// How a signal-aware server finished draining its accepted connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// Every accepted connection finished within the configured deadline.
    Drained,
    /// The deadline elapsed and the remaining connection tasks were aborted.
    DeadlineExceeded,
}

/// Serve an Axum router until Ctrl-C or SIGTERM, then drain accepted
/// connections for at most `drain_timeout`.
///
/// The deadline starts when the signal arrives, not when the server starts.
/// Once signalled, the listener is dropped so no new connections are accepted.
/// Ordinary in-flight requests may finish. If the deadline expires, remaining
/// connection tasks are aborted, which drops their response bodies and
/// propagates cancellation into Proa streaming responses.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    drain_timeout: Duration,
) -> io::Result<ShutdownOutcome> {
    serve_with_shutdown(listener, app, drain_timeout, shutdown_signal()).await
}

/// Like [`serve`], with an injectable shutdown future for supervisors and
/// deterministic integration tests.
///
/// Application entry points normally use [`serve`]. This variant is useful
/// when shutdown comes from an existing cancellation primitive instead of an
/// operating-system signal.
pub async fn serve_with_shutdown<F>(
    listener: TcpListener,
    app: Router,
    drain_timeout: Duration,
    shutdown: F,
) -> io::Result<ShutdownOutcome>
where
    F: Future<Output = ()> + Send,
{
    // `proa dev` sets `PROA_DEV_RELOAD_PORT` on the server processes it
    // spawns, which is the only way this layer is ever built. A production
    // start pays one environment read here and nothing after it.
    let reload = crate::dev_reload::layer_from_env();
    let app = match &reload {
        Some(layer) => {
            tracing::info!("live reload client injection enabled for this dev server");
            layer.clone().wrap(app)
        }
        None => app,
    };
    let graceful = GracefulShutdown::new();
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            () = &mut shutdown => break,
            completed = connections.join_next(), if !connections.is_empty() => {
                report_connection_task(completed);
            }
            accepted = listener.accept() => {
                let (stream, remote_addr) = accepted?;
                let app = app.clone().map_request(|request: http::Request<Incoming>| {
                    request.map(Body::new)
                });
                let watcher = graceful.watcher();

                connections.spawn(async move {
                    let io = TokioIo::new(stream);
                    let service = TowerToHyperService::new(app);
                    let builder = Builder::new(TokioExecutor::new());
                    let connection = builder.serve_connection_with_upgrades(io, service);

                    if let Err(error) = watcher.watch(connection).await {
                        tracing::debug!(?error, %remote_addr, "HTTP connection ended with an error");
                    }
                });
            }
        }
    }

    // Closing the listener is the admission-control boundary. Everything in
    // `connections` was accepted before the shutdown signal.
    drop(listener);
    if let Some(reload) = reload {
        reload.shutdown();
    }
    tracing::info!(
        accepted_connections = graceful.count(),
        ?drain_timeout,
        "shutdown signal received; draining accepted connections"
    );

    match tokio::time::timeout(drain_timeout, graceful.shutdown()).await {
        Ok(()) => {
            while let Some(completed) = connections.join_next().await {
                report_connection_task(Some(completed));
            }
            tracing::info!("graceful shutdown drain completed");
            Ok(ShutdownOutcome::Drained)
        }
        Err(_) => {
            let remaining_connections = connections.len();
            tracing::warn!(
                remaining_connections,
                ?drain_timeout,
                "graceful shutdown deadline elapsed; aborting remaining connections"
            );
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Ok(ShutdownOutcome::DeadlineExceeded)
        }
    }
}

fn report_connection_task(completed: Option<Result<(), tokio::task::JoinError>>) {
    if let Some(Err(error)) = completed {
        tracing::error!(?error, "HTTP connection task failed");
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
