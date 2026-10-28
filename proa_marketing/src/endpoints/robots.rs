// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::header;
use proa_core::{DataLoader, MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

use crate::data::routes;
use crate::utils::document::Url;
use crate::utils::site_url::public_site_url;

struct Robots<'a> {
    origin: &'a str,
}

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Robots<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            User-agent: *
            Allow: /
            Sitemap: {Url(self.origin)}{Url(routes::SITEMAP)}
            "###
        }
        .render_md(cx)
    }
}

pub async fn handler() -> impl axum::response::IntoResponse {
    let body = crate::utils::document::render_markdown(Robots {
        origin: public_site_url(),
    });
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body)
}
