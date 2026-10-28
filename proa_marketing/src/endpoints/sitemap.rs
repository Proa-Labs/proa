// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::header;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::xml_sync;

use crate::{data::routes::STATIC_ROUTES, pages::docs, utils::site_url::public_site_url};

struct Sitemap<'a> {
    origin: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Sitemap<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let paths = STATIC_ROUTES
            .iter()
            .copied()
            .chain(proa_showcase_web::SHOWCASE_ROUTES.iter().copied())
            .chain(
                docs::docs()
                    .pages
                    .iter()
                    .filter(|page| !page.frontmatter.hidden)
                    .map(|page| page.route_path),
            );
        xml_sync! {
            <?xml version="1.0" encoding="UTF-8"?>
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                {for path in paths {
                    xml_sync! { <url><loc>{self.origin}{path}</loc></url> }
                }}
            </urlset>
        }
        .render(cx)
    }
}

pub async fn handler() -> impl axum::response::IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        proa_core::render_to_string(Sitemap {
            origin: public_site_url(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sitemap_escapes_dynamic_xml_and_includes_public_routes() {
        let xml = proa_core::render_to_string(Sitemap {
            origin: "https://example.test/?a=1&b=2",
        });
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("?a=1&amp;b=2"));
        let expected = STATIC_ROUTES.len()
            + proa_showcase_web::SHOWCASE_ROUTES.len()
            + docs::docs()
                .pages
                .iter()
                .filter(|page| !page.frontmatter.hidden)
                .count();
        assert_eq!(xml.matches("<loc>").count(), expected);
    }
}
