// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use axum::extract::Form;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use proa_core::{
    DataLoader, NoLoader, RenderBuffer, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;

mod assets;
pub mod components;
mod forms;
pub mod webmcp;

use components::{PizzaPage, ReceiptPage};
use forms::{pizza_order, CreatePizzaOrder};

pub fn app() -> Router {
    Router::new()
        .route("/", get(index))
        .route(CreatePizzaOrder::PATH, post(create_order))
        .route("/styles.css", get(assets::styles))
        .route("/webmcp-bridge.js", get(assets::webmcp_bridge))
        .route("/rsjs-runtime.js", get(assets::rsjs_runtime))
        .route("/rsjs-runtime-query.js", get(assets::rsjs_query_runtime))
        .route("/rsjs/island/:file", get(assets::rsjs_asset))
}

async fn index() -> Html<String> {
    render_document(PizzaPage::initial())
}

async fn create_order(Form(fields): Form<HashMap<String, String>>) -> Response {
    let submitted = serde_json::to_value(fields).expect("string form fields serialize to JSON");

    match pizza_order::validate_form(submitted) {
        Ok(values) => {
            let normalized_order = serde_json::to_string_pretty(&values)
                .expect("generated Wellformed values serialize to JSON");
            (
                StatusCode::CREATED,
                render_document(ReceiptPage { normalized_order }),
            )
                .into_response()
        }
        Err(errors) => {
            let state = pizza_order::state_with_errors(errors);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                render_document(PizzaPage::from_state(&state)),
            )
                .into_response()
        }
    }
}

fn render_document<C>(content: C) -> Html<String>
where
    C: WebRenderSync<NoLoader, RenderBuffer>,
{
    let mut cx = WebContext::new();
    Document { content }.render(&mut cx).unwrap();
    Html(cx.into_output().into_string())
}

struct Document<C> {
    content: C,
}

impl<B, C, L> WebRenderSync<L, B> for Document<C>
where
    B: WriteBuf,
    C: WebRenderSync<L, B>,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        StaticRaw("<!doctype html>").render(cx)?;
        html_sync! {
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>"Proa WebMCP Pizza"</title>
                    <meta
                        name="description"
                        content="An end-to-end Proa, Wellformed, RSJS, and WebMCP example"
                    />
                    <link rel="stylesheet" href="/styles.css" />
                    <script type="module" src="/webmcp-bridge.js"></script>
                </head>
                <body>{self.content}{RsjsRuntimeTags}</body>
            </html>
        }
        .render(cx)
    }
}

struct RsjsRuntimeTags;

impl<B, L> WebRenderSync<L, B> for RsjsRuntimeTags
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let artifacts = cx.rsjs_artifacts_snapshot();
        let plan = rsjs::manifest()
            .page_runtime_plan(artifacts.rendered_islands())
            .expect("rendered RSJS islands resolve through the linked manifest");
        plan.write_html(&mut cx.out, None)
    }
}
