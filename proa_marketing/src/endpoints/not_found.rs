// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use crate::utils::http::accepts_markdown;
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use proa_core::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::{content_part, html_sync, md_sync, omit};

struct NotFoundPage {
    title: &'static str,
    message: &'static str,
    docs_label: &'static str,
    docs_url: &'static str,
    home_label: &'static str,
    home_url: &'static str,
    conjunction: &'static str,
}

impl NotFoundPage {
    const fn new() -> Self {
        Self {
            title: "Page not found",
            message: "This URL does not identify a public Proa page. Visit the ",
            docs_label: "documentation",
            docs_url: routes::DOCS,
            home_label: "homepage",
            home_url: routes::HOME,
            conjunction: " or ",
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for NotFoundPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <title>
                        {content_part!("browser-title", html_sync! { {self.title} " - Proa" })}
                    </title>
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                </head>
                <body>
                    <main data-not-found-page>
                        <h1>{self.title}</h1>
                        <p>
                            {self.message}
                            <a href={content_part!("docs-url", self.docs_url)}>{self.docs_label}</a>
                            {self.conjunction}
                            <a href={content_part!("home-url", self.home_url)}>{self.home_label}</a>
                            "."
                        </p>
                    </main>
                </body>
            </html>
        }
        .render(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for NotFoundPage {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        use crate::utils::document::Url;
        omit!(
            "browser-title",
            "The browser-tab title repeats the page heading"
        );
        md_sync! { r###"
            # {self.title}

            {self.message}[{self.docs_label}]({content_part!("docs-url", Url(self.docs_url))}){self.conjunction}[{self.home_label}]({content_part!("home-url", Url(self.home_url))}).

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
        crate::utils::document::render_markdown(NotFoundPage::new()),
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
        proa_core::render_to_string(NotFoundPage::new()),
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
