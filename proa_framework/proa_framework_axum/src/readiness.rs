// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Dependency-aware readiness probes for Axum applications.
//!
//! Liveness and readiness answer different questions:
//!
//! - A liveness endpoint such as `/healthz` says that the process is running.
//!   It should stay cheap and should not depend on downstream services.
//! - A readiness endpoint such as `/readyz` says that this process can accept
//!   application traffic. Its probe may check required dependencies.
//!
//! [`Readiness`] runs a fresh probe for every request. Results are never
//! cached, so a process can recover from a transient dependency failure
//! without being restarted.

use std::{fmt::Display, future::Future, sync::Arc, time::Duration};

use axum::{
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, MethodRouter},
    Router,
};

/// Default deadline for one readiness probe.
pub const DEFAULT_READINESS_TIMEOUT: Duration = Duration::from_secs(2);

const READY_BODY: &str = "ready\n";
const NOT_READY_BODY: &str = "not ready\n";
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");
const TEXT_PLAIN_UTF_8: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");

/// A named asynchronous dependency probe for a `/readyz` route.
///
/// The probe is invoked independently for every request and may be called
/// concurrently. It therefore uses `Fn`, not `FnMut`. Compose multiple
/// required dependencies inside the probe with the concurrency or
/// short-circuiting policy appropriate for the application.
///
/// The generated handler does not spawn the probe. Dropping the request
/// future drops both the timeout wrapper and the probe future, propagating
/// cancellation to cancellation-safe dependency clients.
pub struct Readiness<P> {
    name: &'static str,
    probe: P,
    timeout: Duration,
}

impl<P> Readiness<P> {
    /// Create a readiness probe with [`DEFAULT_READINESS_TIMEOUT`].
    ///
    /// `name` appears only in internal structured logs. It is never returned
    /// in the public response.
    pub const fn new(name: &'static str, probe: P) -> Self {
        Self {
            name,
            probe,
            timeout: DEFAULT_READINESS_TIMEOUT,
        }
    }

    /// Override the per-request probe deadline.
    ///
    /// [`Duration::ZERO`] is allowed and deterministically makes a pending
    /// probe unavailable.
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Convert this probe into a GET/HEAD [`MethodRouter`].
    ///
    /// Mount the returned route at `/readyz` or at an application-specific
    /// path:
    ///
    /// ```rust
    /// use axum::Router;
    /// use proa_framework_axum::Readiness;
    ///
    /// let route = Readiness::new("database", || async {
    ///     Ok::<(), std::convert::Infallible>(())
    /// })
    /// .into_route();
    /// let app = Router::new().route("/readyz", route);
    /// # let _: Router = app;
    /// ```
    pub fn into_route<Fut, E>(self) -> MethodRouter
    where
        P: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        let Self {
            name,
            probe,
            timeout,
        } = self;
        // Axum handlers must be Clone. Capture a generic probe behind one
        // startup-time Arc rather than requiring the public probe type to be
        // Clone or boxing each request future.
        let probe = Arc::new(probe);

        get(move || {
            let probe = Arc::clone(&probe);
            async move { run_probe(name, timeout, probe).await }
        })
    }

    /// Convert this probe into a router with GET/HEAD `/readyz`.
    ///
    /// Merge the result into an application's Axum router:
    ///
    /// ```rust
    /// use axum::Router;
    /// use proa_framework_axum::Readiness;
    ///
    /// let readiness = Readiness::new("dependencies", || async {
    ///     Ok::<(), std::convert::Infallible>(())
    /// });
    /// let app = Router::new().merge(readiness.into_router());
    /// # let _: Router = app;
    /// ```
    pub fn into_router<Fut, E>(self) -> Router
    where
        P: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        Router::new().route("/readyz", self.into_route())
    }
}

async fn run_probe<P, Fut, E>(name: &'static str, timeout: Duration, probe: Arc<P>) -> Response
where
    P: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Display + Send + 'static,
{
    match tokio::time::timeout(timeout, (probe.as_ref())()).await {
        Ok(Ok(())) => readiness_response(StatusCode::OK, READY_BODY),
        Ok(Err(error)) => {
            tracing::warn!(
                probe = name,
                error = %error,
                "readiness dependency probe failed"
            );
            readiness_response(StatusCode::SERVICE_UNAVAILABLE, NOT_READY_BODY)
        }
        Err(_) => {
            tracing::warn!(
                probe = name,
                timeout_ms = timeout.as_millis(),
                "readiness dependency probe timed out"
            );
            readiness_response(StatusCode::SERVICE_UNAVAILABLE, NOT_READY_BODY)
        }
    }
}

fn readiness_response(status: StatusCode, body: &'static str) -> Response {
    (
        status,
        [(CACHE_CONTROL, NO_STORE), (CONTENT_TYPE, TEXT_PLAIN_UTF_8)],
        body,
    )
        .into_response()
}
