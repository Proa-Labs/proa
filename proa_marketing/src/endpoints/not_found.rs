// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::utils::http::accepts_markdown;
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync, omit};

struct NotFoundPage;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for NotFoundPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <title>"Page not found - Proa"</title>
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                </head>
                <body>
                    <main data-not-found-page>
                        <h1>"Page not found"</h1>
                        {content_part!("recovery", html_sync! { <p>
                            "This URL does not identify a public Proa page. Visit the "
                            <a href="/docs">"documentation"</a>
                            " or "
                            <a href="/">"homepage"</a>
                            "."
                        </p> })}
                    </main>
                </body>
            </html>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for NotFoundPage {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        omit!(
            "Page not found - Proa",
            "The browser-tab title repeats the page heading"
        );
        md_sync! { r###"
            # Page not found

            {content_part!("recovery", md_sync! { r##"This URL does not identify a public Proa page. Visit the [documentation](/docs) or [homepage](/)."## })}

            Find machine-readable resources in the [agent index](/llms.txt), or browse the [sitemap](/sitemap.xml).
            "###
        }.render_md(cx)
    }
}

pub fn markdown() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            (header::VARY, "Accept"),
        ],
        crate::utils::document::render_markdown(NotFoundPage),
    )
        .into_response()
}

pub fn html() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::VARY, "Accept"),
        ],
        proa_core::render_to_string(NotFoundPage),
    )
        .into_response()
}

pub async fn handler(uri: axum::http::Uri, headers: HeaderMap) -> Response {
    if uri.path().starts_with("/api/") {
        super::api::ApiError::not_found().into_response()
    } else if accepts_markdown(&headers) || uri.path().ends_with(".md") {
        markdown()
    } else {
        html()
    }
}
