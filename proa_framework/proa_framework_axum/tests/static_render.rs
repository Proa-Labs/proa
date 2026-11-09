// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The Axum `RouteResponse` wrapper forwards the runtime-free static renderers.
//! Plain `#[test]` on purpose: no tokio runtime is involved.

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_framework_axum::RouteResponse;
use proa_macros::html_sync;

struct Plain;

impl<L: DataLoader> WebRenderSync<L> for Plain {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let document = html_sync! {
            <!DOCTYPE html>
            <html>
                <head>
                    <title>"Static"</title>
                </head>
                <body>"Rendered through the Axum wrapper"</body>
            </html>
        };
        WebRenderSync::render(document, cx)
    }
}

#[test]
fn axum_route_response_renders_without_a_server_or_runtime() {
    let html = RouteResponse::ssr(Plain)
        .render_to_string()
        .expect("render through the wrapper");
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("<title>Static</title>"), "{html}");
    assert!(html.contains("Rendered through the Axum wrapper"), "{html}");

    let bytes = RouteResponse::ssr(Plain)
        .render_to_bytes()
        .expect("render bytes through the wrapper");
    assert_eq!(bytes.as_ref(), html.as_bytes());
}
