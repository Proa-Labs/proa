// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::to_bytes,
    http::{header, HeaderValue, Response, StatusCode},
};
use std::{convert::Infallible, pin::Pin};

use super::*;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn layer() -> DevReloadLayer {
    DevReloadLayer::new(3001, TOKEN)
}

fn html(body: &'static str) -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .unwrap()
}

/// A body that yields exactly the frames it is given, so a marker can be
/// deliberately split across a chunk boundary the way streaming SSR does.
fn chunked(chunks: Vec<&'static str>) -> Body {
    Body::from_stream(tokio_stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<_, Infallible>(Bytes::from_static(chunk.as_bytes()))),
    ))
}

fn streamed(chunks: Vec<&'static str>) -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .body(chunked(chunks))
        .unwrap()
}

async fn body_string(response: Response<Body>) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn the_client_lands_immediately_before_the_head_close() {
    let response = inject(
        html("<html><head><title>a</title></head><body>b</body></html>"),
        &layer(),
    );
    let body = body_string(response).await;

    let script = body.find("<script>").expect("script injected");
    let head_close = body.find("</head>").expect("head preserved");
    assert!(script < head_close, "{body}");
    assert!(body.contains("__proaLiveReload"), "{body}");
    // The client builds its endpoint from `location` plus these two, so the
    // page works over localhost, 127.0.0.1, or a LAN address alike.
    assert!(body.contains("3001"), "{body}");
    assert!(body.contains("/__proa/dev-events"), "{body}");
    assert!(body.ends_with("</body></html>"), "{body}");
}

#[tokio::test]
async fn a_declared_length_is_dropped_because_the_body_grew() {
    let response = inject(html("<html><head></head><body></body></html>"), &layer());
    assert!(response.headers().get(header::CONTENT_LENGTH).is_none());
}

#[tokio::test]
async fn a_head_close_split_across_frames_is_still_found() {
    // The marker straddles three chunks, which a naive per-chunk search misses
    // and then silently appends the client at the end of the document.
    let response = inject(
        streamed(vec![
            "<html><head><title>a</title><",
            "/he",
            "ad><body>b</body></html>",
        ]),
        &layer(),
    );
    let body = body_string(response).await;

    assert!(body.contains("</head>"), "{body}");
    let script = body.find("<script>").expect("script injected");
    assert!(script < body.find("</head>").unwrap(), "{body}");
    assert_eq!(body.matches("<script").count(), 1, "{body}");
}

#[tokio::test]
async fn an_uppercase_head_close_is_matched_too() {
    let response = inject(html("<HTML><HEAD></HEAD><BODY></BODY></HTML>"), &layer());
    let body = body_string(response).await;
    assert!(
        body.find("<script>").unwrap() < body.find("</HEAD>").unwrap(),
        "{body}"
    );
}

#[tokio::test]
async fn a_document_without_a_head_close_gets_the_client_appended() {
    for document in [
        "<!DOCTYPE html><html><body>no head</body></html>",
        "\n  <html><body>leading space</body></html>",
        "<HTML><BODY>shouting</BODY></HTML>",
    ] {
        let body = body_string(inject(html(document), &layer())).await;
        // Leading bytes are only inspected, never rewritten.
        assert!(body.starts_with(document), "{body}");
        assert!(body.contains("__proaLiveReload"), "{body}");
    }
}

#[tokio::test]
async fn an_html_fragment_is_returned_byte_for_byte() {
    // Islands and action handlers answer with partial HTML. Gluing several
    // kilobytes of client onto those would corrupt every one of them.
    for fragment in [
        "<div>fragment</div>",
        "<li>row</li>",
        "<template id=\"proa-tpl-1\"></template>",
    ] {
        let body = body_string(inject(html(fragment), &layer())).await;
        assert_eq!(body, fragment, "fragment was modified");
    }
}

#[tokio::test]
async fn an_empty_body_still_receives_the_client() {
    let response = inject(html(""), &layer());
    assert!(body_string(response).await.contains("__proaLiveReload"));
}

#[tokio::test]
async fn streamed_chunks_are_forwarded_without_being_buffered_to_the_end() {
    // Everything before the marker must be emitted as it arrives, or Proa's
    // streaming SSR would lose the time to first byte it exists for.
    let mut body = InjectBody::new(
        chunked(vec!["<html><head>", "<title>a</title>", "</head>"]),
        Bytes::from_static(b"<!--client-->"),
    );
    let mut pinned = Pin::new(&mut body);

    let first = std::future::poll_fn(|cx| pinned.as_mut().poll_frame(cx))
        .await
        .unwrap()
        .unwrap();
    // The whole chunk goes out except the last five bytes, which are all that
    // a `</head` split across this boundary could still need.
    assert_eq!(first.into_data().unwrap(), Bytes::from_static(b"<html><"));
}

#[tokio::test]
async fn non_html_responses_are_left_alone() {
    for content_type in [
        "application/json",
        "text/plain",
        "text/html-ish",
        "image/svg+xml",
    ] {
        let response = Response::builder()
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from("<head></head>"))
            .unwrap();
        let injected = inject(response, &layer());
        assert_eq!(
            body_string(injected).await,
            "<head></head>",
            "{content_type} was modified"
        );
    }
}

#[tokio::test]
async fn a_response_without_a_content_type_is_left_alone() {
    let response = Response::builder()
        .body(Body::from("<head></head>"))
        .unwrap();
    assert_eq!(
        body_string(inject(response, &layer())).await,
        "<head></head>"
    );
}

#[tokio::test]
async fn compressed_html_is_left_alone_rather_than_corrupted() {
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .header(header::CONTENT_ENCODING, "gzip")
        .body(Body::from("<head></head>"))
        .unwrap();
    assert_eq!(
        body_string(inject(response, &layer())).await,
        "<head></head>"
    );
}

#[tokio::test]
async fn an_identity_encoding_is_still_injected() {
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .header(header::CONTENT_ENCODING, "identity")
        .body(Body::from("<head></head>"))
        .unwrap();
    assert!(body_string(inject(response, &layer()))
        .await
        .contains("<script>"));
}

#[tokio::test]
async fn an_error_page_is_injected_so_the_tab_still_reloads_when_it_is_fixed() {
    let response = Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "text/html")
        .body(Body::from("<html><head></head><body>500</body></html>"))
        .unwrap();
    assert!(body_string(inject(response, &layer()))
        .await
        .contains("__proaLiveReload"));
}

#[tokio::test]
async fn a_content_security_policy_nonce_is_reused_so_the_client_is_not_blocked() {
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self'; script-src 'nonce-YWJjZGVm' 'strict-dynamic'",
        )
        .body(Body::from("<html><head></head></html>"))
        .unwrap();
    let body = body_string(inject(response, &layer())).await;
    assert!(body.contains("<script nonce=\"YWJjZGVm\">"), "{body}");
}

#[tokio::test]
async fn a_strict_policy_is_widened_just_enough_to_reach_the_event_stream() {
    // `connect-src` falls back to `default-src`, so a site with
    // `default-src 'self'` blocks the stream on another port and live reload
    // never connects at all.
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self'; object-src 'none'",
        )
        .body(Body::from("<html><head></head></html>"))
        .unwrap();
    let injected = inject(response, &layer());
    let policy = injected
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // The app's own sources survive, and only the reload port is added.
    assert!(policy.contains("default-src 'self'"), "{policy}");
    assert!(policy.contains("object-src 'none'"), "{policy}");
    assert!(
        policy.contains("connect-src 'self' http://*:3001"),
        "{policy}"
    );
    assert!(body_string(injected).await.contains("__proaLiveReload"));
}

#[test]
fn an_existing_connect_src_is_extended_rather_than_replaced() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; connect-src 'self' https://api.example.com"),
    );
    let policy = allow_connect_source(&headers, "http://*:3001").unwrap();
    assert!(
        policy.contains("connect-src 'self' https://api.example.com http://*:3001"),
        "{policy}"
    );
    // Exactly one connect-src, or the first one wins and the widening is lost.
    assert_eq!(policy.matches("connect-src").count(), 1, "{policy}");

    // Nothing to widen when the response sets no policy, or when the policy
    // never restricts connections.
    assert_eq!(
        allow_connect_source(&HeaderMap::new(), "http://*:3001"),
        None
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    assert_eq!(allow_connect_source(&headers, "http://*:3001"), None);
}

#[test]
fn the_nonce_comes_from_the_directive_that_governs_scripts() {
    let mut headers = HeaderMap::new();
    // A policy may carry a different nonce for styles. Borrowing that one
    // produces a tag the browser refuses, which is the exact failure this
    // reuse exists to avoid.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("style-src 'nonce-c3R5bGU='; script-src 'nonce-c2NyaXB0'"),
    );
    assert_eq!(script_nonce(&headers).as_deref(), Some("c2NyaXB0"));

    // With no script-src, scripts are governed by default-src.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("style-src 'nonce-c3R5bGU='; default-src 'nonce-ZGVmYXVsdA=='"),
    );
    assert_eq!(script_nonce(&headers).as_deref(), Some("ZGVmYXVsdA=="));

    // A style nonce alone governs nothing this layer injects.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("style-src 'nonce-c3R5bGU='"),
    );
    assert_eq!(script_nonce(&headers), None);
}

#[test]
fn a_document_is_not_called_a_fragment_before_its_opening_is_readable() {
    // A first frame shorter than the prefix it is starting to spell must not
    // settle the question, or a streamed document whose opening arrives one
    // byte at a time silently loses live reload.
    assert_eq!(document_verdict(b"", false), None);
    assert_eq!(document_verdict(b"<", false), None);
    assert_eq!(document_verdict(b"<htm", false), None);
    assert_eq!(document_verdict(b"<!doc", false), None);
    assert_eq!(document_verdict(b"\n  <ht", false), None);

    assert_eq!(document_verdict(b"<html>", false), Some(true));
    assert_eq!(document_verdict(b"<!DOCTYPE html>", false), Some(true));
    // Anything that cannot become a prefix is settled immediately.
    assert_eq!(document_verdict(b"<div", false), Some(false));
    assert_eq!(document_verdict(b"<li>row</li>", false), Some(false));
    // Once the body ends, a partial prefix is all there will ever be.
    assert_eq!(document_verdict(b"<htm", true), Some(false));
}

#[tokio::test]
async fn a_document_streamed_one_byte_at_a_time_still_gets_the_client() {
    // Every frame here is shorter than `<html`, so a decision taken from the
    // first frame alone would be a permanent, wrong "fragment".
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .body(chunked(vec![
            "<", "h", "t", "m", "l", ">", "no head", "</html>",
        ]))
        .unwrap();
    let body = body_string(inject(response, &layer())).await;

    assert!(body.starts_with("<html>no head"), "{body}");
    assert!(body.contains("__proaLiveReload"), "{body}");
}

#[test]
fn a_nonce_with_unexpected_characters_is_refused_so_it_cannot_break_out_of_the_tag() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("script-src 'nonce-abc\"><img src=x>'"),
    );
    assert_eq!(script_nonce(&headers), None);

    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("script-src 'self'"),
    );
    assert_eq!(script_nonce(&headers), None);
}

#[test]
fn only_a_plain_port_and_a_hex_token_turn_injection_on() {
    assert_eq!(parse_reload_port("3001"), Some(3001));
    assert_eq!(parse_reload_port(" 3001 "), Some(3001));
    // A deployed process must not be talked into loading dev tooling by a
    // stray or hostile variable.
    assert_eq!(parse_reload_port("0"), None);
    assert_eq!(parse_reload_port(""), None);
    assert_eq!(parse_reload_port("evil.example.com"), None);
    assert_eq!(parse_reload_port("3001/../x"), None);
    assert_eq!(parse_reload_port("70000"), None);

    assert_eq!(parse_reload_token(TOKEN), Some(TOKEN));
    assert_eq!(parse_reload_token(&format!(" {TOKEN} ")), Some(TOKEN));
    // The token is spliced into a string literal in the injected script, so
    // anything outside lowercase hex is refused rather than escaped.
    assert_eq!(parse_reload_token(""), None);
    assert_eq!(parse_reload_token("0123456789ABCDEF0123456789ABCDEF"), None);
    assert_eq!(parse_reload_token("0123456789abcdef0123456789abcde"), None);
    assert_eq!(parse_reload_token("\";alert(1);//abcdef0123456789ab"), None);
}

#[tokio::test]
async fn the_token_reaches_the_page_so_the_stream_will_admit_it() {
    let body = body_string(inject(html("<html><head></head></html>"), &layer())).await;
    assert!(body.contains(TOKEN), "{body}");
    assert!(body.contains("?token="), "{body}");
}

#[tokio::test]
async fn a_framed_response_is_left_alone_so_it_does_not_spend_a_connection() {
    use tower::{Layer as _, ServiceExt as _};

    let service = layer().layer(tower::service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from("<html><head></head></html>"))
                .unwrap(),
        )
    }));

    // Each event stream costs one of the browser's six connections per
    // origin, and an embedded frame cannot usefully reload on its own.
    let framed = service
        .clone()
        .oneshot(
            Request::builder()
                .header("sec-fetch-dest", "iframe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_string(framed).await, "<html><head></head></html>");

    let navigated = service
        .oneshot(
            Request::builder()
                .header("sec-fetch-dest", "document")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(body_string(navigated).await.contains("__proaLiveReload"));
}

#[test]
fn the_event_path_matches_the_one_the_cli_serves() {
    // proa_cli::lifecycle::reload owns the other copy of this literal.
    assert_eq!(DEV_EVENTS_PATH, "/__proa/dev-events");
    assert_eq!(RELOAD_PORT_ENV, "PROA_DEV_RELOAD_PORT");
    assert_eq!(RELOAD_TOKEN_ENV, "PROA_DEV_RELOAD_TOKEN");
    assert!(DevReloadLayer::new(4321, TOKEN).script.contains("4321"));
}

#[test]
fn the_client_script_has_no_placeholders_left_and_cannot_close_its_own_tag() {
    let script = DevReloadLayer::new(3001, TOKEN).script;
    assert!(!script.contains("__PROA_DEV_RELOAD_PORT__"), "{script}");
    assert!(!script.contains("__PROA_DEV_EVENTS_PATH__"), "{script}");
    // An inline script ends at the first `</script`, wherever it appears.
    assert!(
        !script.to_ascii_lowercase().contains("</script"),
        "{script}"
    );
}
