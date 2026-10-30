// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression coverage through the normal route and middleware stack.
#[cfg(test)]
mod tests {
    use axum::{body::Body, routing::get, Router};
    use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use proa_core::{
        ComponentRole, HeadConfig, HeadOperation, RenderMode, RenderOutcome, SendHeadOperation,
        SendWebContext, WebContext, WebRender, WebRenderSend, WebRenderSync, WriteError,
    };
    use proa_framework_axum::{RouteContextLayer, RouteResponse};
    use proa_macros::{html, html_send, html_sync};
    use std::{future::Future, sync::Arc};
    use tower::ServiceExt;

    const CACHE_HEADERS: [&str; 5] = [
        "cache-control",
        "cdn-cache-control",
        "cloudflare-cdn-cache-control",
        "vercel-cdn-cache-control",
        "surrogate-control",
    ];

    struct PolicyPage(HeaderMap);
    impl WebRender for PolicyPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render_head(&self, cx: &mut WebContext) -> impl HeadOperation {
            async move {
                for (name, value) in &self.0 {
                    assert!(cx.response_mut().append_header(name.clone(), value.clone()));
                }
                Ok(())
            }
        }
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            html! {
                <html>
                    <head>{cx.head()}</head>
                    <body></body>
                </html>
            }
            .render(cx)
        }
    }
    impl WebRenderSend for PolicyPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render_head_send(&self, cx: &mut SendWebContext) -> impl SendHeadOperation {
            async move {
                for (name, value) in &self.0 {
                    assert!(cx.response_mut().append_header(name.clone(), value.clone()));
                }
                Ok(())
            }
        }
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            html_send! { <html><head>{cx.head()}</head><body></body></html> }.render_send(cx)
        }
    }

    fn layer() -> RouteContextLayer {
        RouteContextLayer::new(
            Arc::new(serde_json::from_str(r#"{"islands":{}}"#).unwrap()),
            Arc::new(Default::default()),
        )
        .with_head(HeadConfig::default())
    }

    fn policy_app(send: bool, policies: HeaderMap, middleware: HeaderMap) -> Router {
        Router::new()
            .route(
                "/",
                get(move || {
                    let page = PolicyPage(policies.clone());
                    async move {
                        if send {
                            RouteResponse::ssr_stream_send(page)
                        } else {
                            RouteResponse::ssr_stream(page)
                        }
                    }
                }),
            )
            .layer(axum::middleware::from_fn(
                move |request, next: axum::middleware::Next| {
                    let middleware = middleware.clone();
                    async move {
                        let mut response = next.run(request).await;
                        response.headers_mut().extend(middleware);
                        response
                    }
                },
            ))
            .layer(layer())
    }

    fn request(method: Method) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri("/")
            .body(Body::empty())
            .unwrap()
    }

    fn public_policies() -> HeaderMap {
        CACHE_HEADERS
            .map(|name| {
                (
                    HeaderName::from_static(name),
                    HeaderValue::from_static("public, max-age=86400"),
                )
            })
            .into_iter()
            .collect()
    }

    #[tokio::test]
    async fn middleware_bypass_constrains_every_deferred_cache_header() {
        for send in [false, true] {
            for name in CACHE_HEADERS {
                for restriction in [
                    "private",
                    "no-store",
                    "no-cache",
                    r#"PrIvAtE = "Set-Cookie""#,
                ] {
                    let mut middleware = HeaderMap::new();
                    // Read all values, including a restriction after a permissive one.
                    middleware.append(name, HeaderValue::from_static("max-age=60"));
                    middleware.append(name, HeaderValue::from_static(restriction));
                    let response = policy_app(send, public_policies(), middleware)
                        .oneshot(request(Method::GET))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    for target in CACHE_HEADERS {
                        assert_eq!(
                            response.headers()[target],
                            "private, no-store",
                            "{name}: {restriction} must constrain {target}; send={send}"
                        );
                    }
                    response.into_body().collect().await.unwrap();
                }
            }
        }
    }

    #[tokio::test]
    async fn public_head_policies_survive_when_middleware_does_not_restrict_caching() {
        for send in [false, true] {
            let response = policy_app(send, public_policies(), HeaderMap::new())
                .oneshot(request(Method::GET))
                .await
                .unwrap();
            for name in CACHE_HEADERS {
                assert_eq!(response.headers()[name], "public, max-age=86400");
            }
            response.into_body().collect().await.unwrap();
        }
    }

    #[tokio::test]
    async fn head_through_context_layer_preserves_prepared_headers_and_suppresses_body() {
        for send in [false, true] {
            let response = policy_app(send, public_policies(), HeaderMap::new())
                .oneshot(request(Method::HEAD))
                .await
                .unwrap();
            assert_eq!(
                response.headers()["surrogate-control"],
                "public, max-age=86400"
            );
            assert!(response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty());
        }
    }

    struct Fails;
    impl WebRender for Fails {
        fn render(
            self,
            _: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::<std::future::Ready<_>>::Done(Err(WriteError::render("body error")))
        }
    }

    #[derive(Clone)]
    struct Fallback {
        invalid_head: bool,
    }
    impl WebRenderSync for Fallback {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            if self.invalid_head {
                // Intentionally ignore the result: lifecycle failures must remain sticky.
                let _ = cx.head_mut().title("illegal fallback metadata");
            }
            let fallback = html_sync! {
                <p data-error-fallback></p>
            };
            WebRenderSync::render(fallback, cx)
        }
    }

    struct RecoveryPage(Fallback);
    impl WebRender for RecoveryPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            let body = proa_core::ErrorBoundary::new(proa_stream::Suspense::new(Fails, ()), self.0);
            html! {
                <html>
                    <head>{cx.head()}</head>
                    <body>{body}</body>
                </html>
            }
            .render(cx)
        }
    }

    #[tokio::test]
    async fn deferred_error_fallbacks_cannot_hide_head_lifecycle_errors() {
        for mode in [RenderMode::ChunkedInOrder, RenderMode::ChunkedOutOfOrder] {
            for invalid_head in [false, true] {
                let app = Router::new()
                    .route(
                        "/",
                        get(move || async move {
                            RouteResponse::ssr_stream(RecoveryPage(Fallback { invalid_head }))
                                .with_render_mode(mode)
                        }),
                    )
                    .layer(layer());
                let response = app.oneshot(request(Method::GET)).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let collected = response.into_body().collect().await;
                assert_eq!(collected.is_err(), invalid_head, "mode={mode:?}");
            }
        }
    }
}
