// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    convert::Infallible,
    future::{ready, Ready},
    sync::atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

#[tokio::test]
async fn dropping_a_pending_router_request_cancels_the_endpoint() {
    use super::super::metadata::TitleOnly;
    use std::sync::atomic::AtomicBool;

    static META: TitleOnly = TitleOnly::new("album");
    let entered = Arc::new(AtomicBool::new(false));
    let handler = {
        let entered = entered.clone();
        axum::routing::get(move || async move {
            assert_eq!(
                crate::resolved_metadata().unwrap().title.as_deref(),
                Some("album")
            );
            entered.store(true, Ordering::Relaxed);
            std::future::pending::<()>().await
        })
    };
    let (router, metadata) =
        crate::router_from_spec(vec![
            crate::Route::endpoint("/albums/{id}", handler).with_metadata(&META)
        ]);
    let router = router.layer(RouteContextLayer::new(metadata));
    let mut request =
        Box::pin(router.oneshot(http::Request::get("/albums/7").body(Body::empty()).unwrap()));
    assert!(futures::poll!(&mut request).is_pending());
    assert!(entered.load(Ordering::Relaxed));
    // Drop from the caller outside the request's task-local scope.
    drop(request);
}

#[test]
fn capture_ranges_require_a_complete_match_with_plain_named_segments() {
    for (pattern, path) in [
        ("/static", "/static"),
        ("/docs/{slug}.md", "/docs/guide.md"),
        ("/albums/{id}/edit", "/albums/7/view"),
        ("/albums/{id}", "/albums/"),
        ("/albums/{id}", "/albums/a%2Fb"),
        ("/files/{*rest}", "/files/a/b"),
    ] {
        assert!(
            CaptureRanges::for_path(pattern, path).is_none(),
            "{pattern} {path}"
        );
    }
    let pattern = "/{artist}/albums/{id}/tracks/{track}/";
    for path in ["/same/albums/7/tracks/same", "/same/albums/7/tracks/same/"] {
        let ranges = CaptureRanges::for_path(pattern, path).unwrap();
        let captures: Vec<_> = ranges
            .as_slice()
            .iter()
            .map(|capture| (capture.name.slice(pattern), capture.value.slice(path)))
            .collect();
        assert_eq!(
            captures,
            [("artist", "same"), ("id", "7"), ("track", "same")]
        );
    }
}

#[tokio::test]
async fn context_preserves_original_headers_across_inner_middleware() {
    async fn handler(headers: http::HeaderMap) {
        assert!(!headers.contains_key("x-test"));
        ROUTE_CONTEXT.with(|context| {
            let context = context.borrow();
            let mut values = context.current_headers().get_all("x-test").iter();
            assert_eq!(values.next().unwrap(), "first");
            let second = values.next().unwrap();
            assert_eq!(second, "second");
            assert!(second.is_sensitive());
            assert!(values.next().is_none());
        });
    }
    let router = axum::Router::new()
        .route("/albums/{id}", axum::routing::get(handler))
        .layer(axum::middleware::from_fn(
            |mut req: http::Request<Body>, next: axum::middleware::Next| async move {
                req.headers_mut().remove("x-test");
                next.run(req).await
            },
        ))
        .layer(RouteContextLayer::new(Arc::default()));
    let mut secret = http::HeaderValue::from_static("second");
    secret.set_sensitive(true);
    let request = http::Request::get("/albums/7")
        .header("x-test", "first")
        .header("x-test", secret)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        http::StatusCode::OK
    );
}

#[tokio::test]
async fn cancelling_a_metadata_only_endpoint_restores_its_outer_cache() {
    use super::super::metadata::TitleOnly;

    static OUTER: TitleOnly = TitleOnly::new("outer provider");
    static INNER: TitleOnly = TitleOnly::new("inner provider");
    let mut context = RouteContext::new(Arc::default());
    context.restore_metadata(
        Arc::from([&OUTER as MetadataRef]),
        Some(ResolvedMetadata {
            title: Some("cached outer title".to_owned()),
            ..Default::default()
        }),
    );
    let layer = EndpointContextLayer::new(Arc::from([&INNER as MetadataRef]), Arc::from("/page"));
    let mut endpoint = layer.layer(tower::service_fn(|_| {
        std::future::pending::<Result<Response, Infallible>>()
    }));
    ROUTE_CONTEXT
        .scope(RefCell::new(context), async {
            let mut future = Box::pin(endpoint.call(http::Request::new(Body::empty())));
            assert!(futures::poll!(&mut future).is_pending());
            ROUTE_CONTEXT.with(|ctx| {
                let mut ctx = ctx.borrow_mut();
                assert_eq!(
                    ctx.resolve_metadata().unwrap().title.as_deref(),
                    Some("inner provider")
                );
                ctx.set_route_params(RouteParams::from_vec(vec![RouteParam::new(
                    "keep", "value",
                )]));
            });
            drop(future);
            ROUTE_CONTEXT.with(|ctx| {
                let mut ctx = ctx.borrow_mut();
                assert_eq!(
                    ctx.resolve_metadata().unwrap().title.as_deref(),
                    Some("cached outer title")
                );
                assert_eq!(ctx.current_route_params().get("keep"), Some("value"));
            });
        })
        .await;
}

#[tokio::test]
async fn cancelling_a_params_only_endpoint_preserves_unrelated_metadata() {
    use super::super::metadata::TitleOnly;

    static META: TitleOnly = TitleOnly::new("keep");
    let layer = EndpointContextLayer::new(Arc::from([]), Arc::from("/albums/{id}"));
    let mut endpoint = layer.layer(tower::service_fn(|_| {
        std::future::pending::<Result<Response, Infallible>>()
    }));
    ROUTE_CONTEXT
        .scope(RefCell::new(RouteContext::new(Arc::default())), async {
            let mut future = Box::pin(
                endpoint.call(http::Request::get("/albums/7").body(Body::empty()).unwrap()),
            );
            assert!(futures::poll!(&mut future).is_pending());
            ROUTE_CONTEXT.with(|ctx| {
                let mut ctx = ctx.borrow_mut();
                assert_eq!(ctx.current_route_params().get("id"), Some("7"));
                ctx.swap_metadata(Arc::from([&META as MetadataRef]));
            });
            drop(future);
            ROUTE_CONTEXT.with(|ctx| {
                let mut ctx = ctx.borrow_mut();
                assert!(ctx.current_route_params().is_empty());
                assert_eq!(
                    ctx.resolve_metadata().unwrap().title.as_deref(),
                    Some("keep")
                );
            });
        })
        .await;
}

#[tokio::test]
async fn route_params_follow_axum_on_static_suffixed_and_trailing_slash_paths() {
    async fn slug() -> String {
        ROUTE_CONTEXT.with(|context| {
            context
                .borrow_mut()
                .current_route_params()
                .get("slug")
                .unwrap_or("no captures")
                .to_owned()
        })
    }
    let router = axum::Router::new()
        .route("/static", axum::routing::get(slug))
        .route("/docs/{slug}", axum::routing::get(slug))
        .route("/albums/{slug}/", axum::routing::get(slug))
        .layer(RouteContextLayer::new(Arc::default()));
    for (path, expected) in [
        ("/static", Some("no captures")),
        ("/docs/blue.md", Some("blue.md")),
        ("/other/blue.md", None),
        ("/docs/blue.md/", None),
        ("/albums/7/", Some("7")),
        ("/albums/7", None),
    ] {
        let response = router
            .clone()
            .oneshot(http::Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        if let Some(value) = expected {
            assert_eq!(response.status(), http::StatusCode::OK, "{path}");
            let body = axum::body::to_bytes(response.into_body(), 128)
                .await
                .unwrap();
            assert_eq!(body.as_ref(), value.as_bytes(), "{path}");
        } else {
            assert_eq!(response.status(), http::StatusCode::NOT_FOUND, "{path}");
        }
    }
}

#[tokio::test]
async fn named_params_are_materialized_on_first_read_inside_the_endpoint() {
    async fn handler() {
        ROUTE_CONTEXT.with(|context| {
            let mut context = context.borrow_mut();
            assert!(matches!(
                context.snapshot.current_route_params(),
                AxumRouteParams::Matched(_)
            ));
            let mut refresh = context.clone();
            assert_eq!(refresh.current_route_params().get("album"), Some("sea"));
            assert!(matches!(
                context.snapshot.current_route_params(),
                AxumRouteParams::Matched(_)
            ));
            assert_eq!(context.current_route_params().get("album"), Some("sea"));
            assert!(matches!(
                context.snapshot.current_route_params(),
                AxumRouteParams::Ready(_)
            ));
            assert_eq!(context.current_route_params().get("id"), Some("07"));
        });
    }
    let (router, metadata) = crate::router_from_spec(vec![crate::Route::endpoint(
        "/albums/{album}/{id}",
        axum::routing::get(handler),
    )]);
    let response = router
        .layer(RouteContextLayer::new(metadata))
        .oneshot(
            http::Request::get("/albums/sea/07")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::OK);
}

struct ReadyService(Arc<AtomicUsize>);
impl Clone for ReadyService {
    fn clone(&self) -> Self {
        self.0.fetch_add(1, Ordering::Relaxed);
        Self(self.0.clone())
    }
}
impl Service<axum::http::Request<Body>> for ReadyService {
    type Response = Response;
    type Error = Infallible;
    type Future = Ready<Result<Response, Infallible>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, _: axum::http::Request<Body>) -> Self::Future {
        ready(Ok(Response::new(Body::from("ok"))))
    }
}

#[tokio::test]
async fn static_context_calls_the_ready_service_without_cloning_it() {
    let clones = Arc::new(AtomicUsize::new(0));
    let layer = RouteContextLayer::new(Arc::new(Default::default()));
    let mut service = layer.layer(ReadyService(clones.clone()));
    for _ in 0..3 {
        service
            .ready()
            .await
            .unwrap()
            .call(axum::http::Request::new(Body::empty()))
            .await
            .unwrap();
    }
    assert_eq!(clones.load(Ordering::Relaxed), 0);
}
