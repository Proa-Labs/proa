// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::{header::ACCEPT, HeaderMap, HeaderName, HeaderValue};

pub(crate) fn insert_estimated_token_headers(
    headers: &mut HeaderMap,
    markdown: &str,
    original: &str,
) {
    headers.insert(
        HeaderName::from_static("x-markdown-tokens"),
        token_count_header_value(markdown),
    );
    headers.insert(
        HeaderName::from_static("x-original-tokens"),
        token_count_header_value(original),
    );
}

fn token_count_header_value(source: &str) -> HeaderValue {
    HeaderValue::from_str(&estimated_token_count(source).to_string())
        .expect("numeric token estimate should be a valid header value")
}

fn estimated_token_count(source: &str) -> usize {
    let non_whitespace = source.chars().filter(|ch| !ch.is_whitespace()).count();
    non_whitespace.saturating_add(3) / 4
}

pub(crate) fn accepts_markdown(headers: &HeaderMap) -> bool {
    headers.get_all(ACCEPT).iter().any(|value| {
        value
            .to_str()
            .is_ok_and(|accept| accept.split(',').any(accept_item_allows_markdown))
    })
}

fn accept_item_allows_markdown(item: &str) -> bool {
    let mut parts = item.split(';').map(str::trim);
    let Some(media_type) = parts.next() else {
        return false;
    };

    if !media_type.eq_ignore_ascii_case("text/markdown") {
        return false;
    }

    parts.all(|part| {
        let Some(q) = part.strip_prefix("q=") else {
            return true;
        };

        q.trim()
            .parse::<f32>()
            .map(|quality| quality > 0.0)
            .unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use super::{accepts_markdown, insert_estimated_token_headers};
    use axum::http::{header::ACCEPT, HeaderMap, HeaderValue};

    #[test]
    fn markdown_negotiation_honors_media_type_and_quality() {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("text/html, text/markdown; q=0.8"),
        );
        assert!(accepts_markdown(&headers));

        headers.insert(ACCEPT, HeaderValue::from_static("text/markdown; q=0"));
        assert!(!accepts_markdown(&headers));
    }

    #[test]
    fn token_headers_are_derived_from_each_representation() {
        let mut headers = HeaderMap::new();
        insert_estimated_token_headers(&mut headers, "1234 5678", "123456789012");

        assert_eq!(headers["x-markdown-tokens"], "2");
        assert_eq!(headers["x-original-tokens"], "3");
    }
}
