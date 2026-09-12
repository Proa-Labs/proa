// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::header;

use crate::utils::site_url::public_site_url;

pub async fn handler() -> impl axum::response::IntoResponse {
    let site_url = public_site_url();
    let body = format!(
        "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
        site_url
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body)
}
