// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::header;

use crate::data::routes::STATIC_ROUTES;
use crate::pages::docs;
use crate::utils::site_url::public_site_url;

// proa-perf-ok(scope): sitemap XML is emitted once per request outside Proa HTML rendering
pub async fn handler() -> impl axum::response::IntoResponse {
    let site_url = public_site_url();
    let mut body = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );

    for path in STATIC_ROUTES {
        push_url(&mut body, site_url, path);
    }

    for path in proa_showcase_web::SHOWCASE_ROUTES {
        push_url(&mut body, site_url, path);
    }

    for page in docs::docs().pages {
        if page.frontmatter.hidden {
            continue;
        }
        push_url(&mut body, site_url, page.route_path);
    }

    body.push_str("</urlset>\n");
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
}

// proa-perf-ok(scope): sitemap XML is emitted once per request outside Proa HTML rendering
fn push_url(body: &mut String, site_url: &str, path: &str) {
    body.push_str("  <url><loc>");
    body.push_str(site_url);
    body.push_str(path);
    body.push_str("</loc></url>\n");
}
