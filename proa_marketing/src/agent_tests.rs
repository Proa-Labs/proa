// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower_test::ServiceExt;

async fn fetch(app: &Router, path: &str, accept: &str) -> (StatusCode, HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header(header::ACCEPT, accept)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn homepage_negotiates_markdown_and_keeps_html_for_browsers() {
    let app = super::app();
    let (_, headers, html) = fetch(&app, "/", "text/html").await;
    assert!(headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    assert_eq!(headers[header::VARY], "Accept");
    let (status, headers, markdown) = fetch(&app, "/", "text/markdown").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );
    assert_eq!(headers[header::VARY], "Accept");
    assert!(markdown.starts_with("# "));
    assert!(!markdown.contains("<html"));
    assert_eq!(markdown, fetch(&app, "/index.md", "*/*").await.2);
    assert_eq!(markdown, fetch(&app, "/?mode=agent", "text/html").await.2);
    assert_eq!(
        html,
        fetch(&app, "/", "text/markdown;q=0, text/html").await.2
    );

    let data = html
        .split_once("<script type=\"application/ld+json\">")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let graph: Value = serde_json::from_str(data).unwrap();
    let graph = graph["@graph"].as_array().unwrap();
    assert!(graph
        .iter()
        .any(|entity| entity["@type"] == "Organization" && entity["contactPoint"].is_array()));
    assert!(graph
        .iter()
        .any(|entity| entity["@type"] == "SoftwareApplication" && entity["sameAs"].is_array()));
    for path in ["/about", "/contact", "/docs/api", "/llms.txt"] {
        assert!(html.contains(&format!("href=\"{path}\"")));
        assert_eq!(fetch(&app, path, "*/*").await.0, StatusCode::OK);
    }
}

#[tokio::test]
async fn discovery_links_resolve_and_do_not_advertise_hidden_docs() {
    let app = super::app();
    let origin = super::utils::site_url::public_site_url();
    for path in ["/llms.txt", "/docs/llms.txt"] {
        let (status, headers, body) = fetch(&app, path, "*/*").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "text/plain; charset=utf-8");
        assert!(body.starts_with("# "));
        assert!(body.len() < 30_000);
        for link in body
            .split("](")
            .skip(1)
            .filter_map(|part| part.split_once(')').map(|p| p.0))
            .filter_map(|url| url.strip_prefix(origin))
        {
            assert_eq!(
                fetch(&app, link, "*/*").await.0,
                StatusCode::OK,
                "{path}: {link}"
            );
        }
        for hidden in super::pages::docs::docs()
            .pages
            .iter()
            .filter(|page| page.frontmatter.hidden)
        {
            assert!(!body.contains(hidden.markdown_path));
        }
    }
    let catalog: Value =
        serde_json::from_str(&fetch(&app, "/.well-known/ard.json", "*/*").await.2).unwrap();
    assert_eq!(catalog["specVersion"], "1.0");
    for entry in catalog["entries"].as_array().unwrap() {
        assert!(entry["identifier"]
            .as_str()
            .unwrap()
            .starts_with("urn:air:proa.so:"));
        let path = entry["url"].as_str().unwrap().strip_prefix(origin).unwrap();
        assert_eq!(fetch(&app, path, "*/*").await.0, StatusCode::OK);
    }
    let skills: Value = serde_json::from_str(
        &fetch(&app, "/.well-known/agent-skills/index.json", "*/*")
            .await
            .2,
    )
    .unwrap();
    assert_eq!(
        skills["$schema"],
        "https://schemas.agentskills.io/discovery/0.2.0/schema.json"
    );
    for skill in skills["skills"].as_array().unwrap() {
        let path = skill["url"].as_str().unwrap().strip_prefix(origin).unwrap();
        let (status, _, body) = fetch(&app, path, "*/*").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(skill["type"], "skill-md");
        let digest = skill["digest"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap();
        assert_eq!(digest.len(), 64);
        let decoded = (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&digest[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(decoded, Sha256::digest(body.as_bytes()).to_vec());
        assert!(body.contains(&format!("name: {}", skill["name"].as_str().unwrap())));
    }
}

#[tokio::test]
async fn api_catalog_supports_head_discovery_and_links_to_a_working_api() {
    let app = super::app();
    let (status, headers, body) = fetch(&app, "/.well-known/api-catalog", "*/*").await;
    assert_eq!(status, StatusCode::OK);
    let link = headers[header::LINK].to_str().unwrap();
    assert!(link.contains("rel=\"api-catalog\""));
    assert!(link.contains("</.well-known/api-catalog>"));
    let catalog: Value = serde_json::from_str(&body).unwrap();
    let url = catalog["linkset"][0]["item"][0]["href"].as_str().unwrap();
    let path = url
        .strip_prefix(super::utils::site_url::public_site_url())
        .unwrap();
    assert_eq!(
        fetch(&app, path, "application/json").await.0,
        StatusCode::OK
    );

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/.well-known/api-catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::LINK], headers[header::LINK]);
    assert!(to_bytes(response.into_body(), 1024)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn public_discovery_supports_browser_reads_and_conditional_request_preflights() {
    let app = super::app();
    for path in [
        "/.well-known/ard.json",
        "/.well-known/ai-catalog.json",
        "/.well-known/api-catalog",
        "/openapi.json",
        "/.well-known/agent-skills/index.json",
        "/.well-known/agent-skills/proa-component/SKILL.md",
        "/mcp/server-card",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::ORIGIN, "https://client.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS));
        assert!(response.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS]
            .to_str()
            .unwrap()
            .split(',')
            .any(|name| name.trim().eq_ignore_ascii_case("link")));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri(path)
                    .header(header::ORIGIN, "https://client.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "if-none-match")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "OPTIONS {path}");
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert!(response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
            .to_str()
            .unwrap()
            .contains("if-none-match"));
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
            "GET,HEAD"
        );
    }
}

#[tokio::test]
async fn rest_search_pagination_retrieval_and_errors_match_the_contract() {
    let app = super::app();
    let (status, headers, body) =
        fetch(&app, "/api/v1/search?q=render&limit=2", "application/json").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(headers.contains_key("ratelimit"));
    let first: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(first["results"].as_array().unwrap().len(), 2);
    let cursor = first["next_cursor"].as_str().unwrap();
    let next: Value = serde_json::from_str(
        &fetch(
            &app,
            &format!("/api/v1/search?q=render&limit=2&cursor={cursor}"),
            "application/json",
        )
        .await
        .2,
    )
    .unwrap();
    assert_ne!(first["results"][0]["path"], next["results"][0]["path"]);
    let path = first["results"][0]["path"].as_str().unwrap();
    let page: Value = serde_json::from_str(
        &fetch(
            &app,
            &format!("/api/v1/docs/page?path={path}"),
            "application/json",
        )
        .await
        .2,
    )
    .unwrap();
    let markdown_path = first["results"][0]["markdown_url"].as_str().unwrap();
    assert_eq!(
        page["markdown"],
        fetch(&app, markdown_path, "text/markdown").await.2
    );
    for (path, expected) in [
        ("/api/v1/search", StatusCode::BAD_REQUEST),
        ("/api/v1/search?q=%20", StatusCode::BAD_REQUEST),
        ("/api/v1/search?q=render&limit=0", StatusCode::BAD_REQUEST),
        ("/api/v1/search?q=render&limit=abc", StatusCode::BAD_REQUEST),
        ("/api/v1/docs?limit=51", StatusCode::BAD_REQUEST),
        ("/api/v1/docs?cursor=invalid", StatusCode::BAD_REQUEST),
        (
            "/api/v1/docs/page?path=https://example.org/",
            StatusCode::NOT_FOUND,
        ),
        ("/api/v1/docs/page?path=/etc/passwd", StatusCode::NOT_FOUND),
        ("/api/v1/missing", StatusCode::NOT_FOUND),
    ] {
        let (status, headers, body) = fetch(&app, path, "application/json").await;
        assert_eq!(status, expected, "{path}: {body}");
        assert_eq!(headers[header::CONTENT_TYPE], "application/problem+json");
        let problem: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(problem["status"], expected.as_u16());
        assert!(problem["detail"].is_string());
    }
}

#[tokio::test]
async fn missing_pages_preserve_404_with_useful_markdown() {
    let app = super::app();
    for path in [
        "/does-not-exist",
        "/docs/does-not-exist",
        "/docs/does-not-exist.md",
        "/does-not-exist.md",
    ] {
        let (status, headers, body) = fetch(&app, path, "text/markdown").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert!(body.contains("](/llms.txt)"));
        assert_eq!(headers[header::VARY], "Accept");

        let (status, headers, _) = fetch(&app, path, "text/html").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers[header::VARY], "Accept");
        let expected_type = if path.ends_with(".md") {
            "text/markdown; charset=utf-8"
        } else {
            "text/html; charset=utf-8"
        };
        assert_eq!(headers[header::CONTENT_TYPE], expected_type);
    }
}

#[tokio::test]
async fn rest_method_errors_and_missing_routes_have_distinct_typed_responses() {
    let app = super::app();
    for (path, status) in [
        ("/api", StatusCode::METHOD_NOT_ALLOWED),
        ("/api/v1", StatusCode::METHOD_NOT_ALLOWED),
        ("/api/v1/search", StatusCode::METHOD_NOT_ALLOWED),
        ("/api/v1/docs", StatusCode::METHOD_NOT_ALLOWED),
        ("/api/v1/docs/page", StatusCode::METHOD_NOT_ALLOWED),
        ("/api/v1/missing", StatusCode::NOT_FOUND),
        ("/api/missing", StatusCode::NOT_FOUND),
    ] {
        for method in [Method::POST, Method::PUT, Method::DELETE] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{method} {path}");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "application/problem+json"
            );
            if status == StatusCode::METHOD_NOT_ALLOWED {
                assert_eq!(response.headers()[header::ALLOW], "GET,HEAD");
            }
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let problem: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(problem["status"], status.as_u16());
        }
    }
}

async fn rpc(app: &Router, method: &str, params: Value) -> Value {
    let origin = super::utils::site_url::public_site_url();
    let host = origin.split_once("://").unwrap().1;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, host)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header("mcp-protocol-version", "2025-06-18")
                .body(Body::from(
                    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("Non-JSON MCP response: {}", String::from_utf8_lossy(&bytes)))
}

#[tokio::test]
async fn mcp_initializes_lists_searches_reads_and_rejects_invalid_inputs() {
    let app = super::app();
    let initialized = rpc(&app, "initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}})).await;
    let (status, headers, card) =
        fetch(&app, "/mcp/server-card", "application/mcp-server-card+json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/mcp-server-card+json; charset=utf-8"
    );
    let card: Value = serde_json::from_str(&card).unwrap();
    for field in ["name", "version", "title", "description", "websiteUrl"] {
        assert!(card[field].is_string(), "{field}");
        assert_eq!(
            initialized["result"]["serverInfo"][field], card[field],
            "{field}: {initialized}"
        );
    }
    let tools = rpc(&app, "tools/list", json!({})).await;
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["openWorldHint"], false);
        assert_eq!(tool["outputSchema"]["type"], "object");
    }
    let results = rpc(
        &app,
        "tools/call",
        json!({"name":"search_docs","arguments":{"q":"streaming","limit":2}}),
    )
    .await;
    let doc = &results["result"]["structuredContent"]["results"][0];
    assert!(doc["path"].is_string(), "{results}");
    let page = rpc(
        &app,
        "tools/call",
        json!({"name":"get_doc","arguments":{"path":doc["path"]}}),
    )
    .await;
    assert!(
        page["result"]["structuredContent"]["markdown"]
            .as_str()
            .unwrap()
            .len()
            > 100
    );
    let resources = rpc(&app, "resources/list", json!({})).await;
    assert_eq!(
        resources["result"]["resources"].as_array().unwrap().len(),
        50
    );
    let resource = &resources["result"]["resources"][0];
    assert_eq!(resource["mimeType"], "text/markdown");
    let read = rpc(&app, "resources/read", json!({"uri":resource["uri"]})).await;
    assert_eq!(read["result"]["contents"][0]["mimeType"], "text/markdown");
    for args in [
        json!({}),
        json!({"path":"/etc/passwd"}),
        json!({"path":"https://example.com/"}),
    ] {
        let error = rpc(
            &app,
            "tools/call",
            json!({"name":"get_doc","arguments":args}),
        )
        .await;
        assert_eq!(error["result"]["isError"], true, "{error}");
    }
    assert!(rpc(&app, "does/not/exist", json!({})).await["error"].is_object());
    assert!(rpc(
        &app,
        "resources/read",
        json!({"uri":"https://example.com/docs"})
    )
    .await["error"]
        .is_object());
}

#[tokio::test]
async fn mcp_enforces_host_origin_and_body_limits() {
    let app = super::app();
    let origin = super::utils::site_url::public_site_url();
    let host = origin.split_once("://").unwrap().1;
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let oversized = " ".repeat(16 * 1024 + 1);
    for (request_host, request_origin, body, status) in [
        (
            host,
            Some("https://untrusted.example"),
            body,
            StatusCode::FORBIDDEN,
        ),
        ("untrusted.example", None, body, StatusCode::FORBIDDEN),
        (host, Some(origin), body, StatusCode::OK),
        (host, None, body, StatusCode::OK),
        (
            host,
            None,
            oversized.as_str(),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header(header::HOST, request_host)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18");
        if let Some(origin) = request_origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            status,
            "host {request_host}, origin {request_origin:?}"
        );
    }
}
