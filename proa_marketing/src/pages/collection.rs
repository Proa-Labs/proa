// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    http::{header::CONTENT_TYPE, header::LINK, header::VARY, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use proa_docs::{DocPage, Docs, DocsResponse};

use crate::{
    components::docs::DocsPageView,
    layout::{render_docs_page, PageMeta},
    utils::http::{accepts_markdown, insert_estimated_token_headers},
};

pub(crate) fn response_for_collection_path(
    docs: &'static Docs,
    path: &str,
    label: &'static str,
    request_headers: &HeaderMap,
) -> Response {
    if let Some(response) = proa_docs::render_static(docs, path) {
        let mut response = docs_response_to_axum(response);
        if let Some(page) = page_for_markdown_path(docs, path) {
            insert_page_token_headers(&mut response, page);
        }
        return response;
    }

    let Some(page) = docs.page_for_path(path) else {
        if accepts_markdown(request_headers) || path.ends_with(".md") {
            return crate::endpoints::not_found::markdown();
        }
        return crate::endpoints::not_found::html();
    };

    if accepts_markdown(request_headers) {
        return negotiated_markdown_response(page);
    }

    let mut response = render_docs_page(
        PageMeta {
            title: page.title,
            description: page.description.unwrap_or(docs.config.site_name),
            path: page.route_path,
        },
        DocsPageView { docs, page, label },
    )
    .into_response();

    response
        .headers_mut()
        .insert(VARY, HeaderValue::from_static("Accept"));
    insert_alternate_markdown_link(&mut response, page);
    insert_page_token_headers(&mut response, page);
    response
}

/// Advertise the Markdown representation of an HTML docs page.
///
/// Style guide 11.2: the site already emits `rel="service-desc"` for OpenAPI,
/// so an agent can discover the API but not the readable source of the page it
/// just fetched. `rel="alternate"` closes that gap without a second request.
fn insert_alternate_markdown_link(response: &mut Response, page: &'static DocPage) {
    let value = format!(
        "<{}>; rel=\"alternate\"; type=\"text/markdown\"",
        page.markdown_path
    );
    if let Ok(value) = HeaderValue::from_str(&value) {
        response.headers_mut().append(LINK, value);
    }
}

fn page_for_markdown_path(docs: &'static Docs, path: &str) -> Option<&'static DocPage> {
    docs.pages.iter().find(|page| page.markdown_path == path)
}

fn docs_response_to_axum(response: DocsResponse) -> Response {
    (
        StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK),
        [(CONTENT_TYPE, response.content_type)],
        response.body.into_owned(),
    )
        .into_response()
}

fn negotiated_markdown_response(page: &'static DocPage) -> Response {
    let mut response = (
        StatusCode::OK,
        [
            (CONTENT_TYPE, "text/markdown; charset=utf-8"),
            (VARY, "Accept"),
        ],
        page.markdown,
    )
        .into_response();

    insert_page_token_headers(&mut response, page);
    response
}

fn insert_page_token_headers(response: &mut Response, page: &DocPage) {
    insert_estimated_token_headers(response.headers_mut(), page.markdown, page.article_html);
}

#[cfg(test)]
mod tests {
    use super::response_for_collection_path;
    use crate::pages::docs::docs;
    use axum::http::{header::ACCEPT, header::CONTENT_TYPE, HeaderMap, HeaderValue};

    #[test]
    fn page_representations_include_negotiation_and_token_metadata() {
        let html = response_for_collection_path(docs(), "/docs", "Docs", &HeaderMap::new());
        assert_eq!(html.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(html.headers()["vary"], "Accept");
        assert!(html.headers().contains_key("x-markdown-tokens"));
        assert!(html.headers().contains_key("x-original-tokens"));

        let mut request_headers = HeaderMap::new();
        request_headers.insert(ACCEPT, HeaderValue::from_static("text/markdown"));
        let markdown = response_for_collection_path(docs(), "/docs", "Docs", &request_headers);
        assert_eq!(
            markdown.headers()[CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert!(markdown.headers().contains_key("x-markdown-tokens"));
        assert!(markdown.headers().contains_key("x-original-tokens"));

        let page = docs().find_page("").expect("docs index");
        let direct_markdown =
            response_for_collection_path(docs(), page.markdown_path, "Docs", &HeaderMap::new());
        assert_eq!(
            direct_markdown.headers()[CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert!(direct_markdown.headers().contains_key("x-markdown-tokens"));
        assert!(direct_markdown.headers().contains_key("x-original-tokens"));
    }
}
