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

use crate::data::site;

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
    assert_eq!(markdown, fetch(&app, "/llms.txt", "*/*").await.2);
    assert_eq!(
        markdown,
        fetch(&app, "/.well-known/llms.txt", "*/*").await.2
    );
    assert_eq!(markdown, fetch(&app, "/?mode=agent", "text/html").await.2);
    // Every homepage alias retains the tool entry points from the footer;
    // they must not return to the main landing-page content.
    let (before_footer, footer) = html.split_once("<footer").unwrap();
    assert!(!before_footer.contains("data-footer-links"));
    assert_eq!(footer.matches("data-footer-links").count(), 3);
    for path in [
        "/docs/llms.txt",
        "/docs/framework/llms.txt",
        "/docs/rsjs/llms.txt",
        "/ecosystem/llms.txt",
        "/docs/api",
        "/openapi.json",
        "/mcp/server-card",
        "/auth.md",
        "/.well-known/agent-skills/index.json",
    ] {
        assert!(markdown.contains(&format!("]({path})")), "{path}");
        assert!(footer.contains(&format!("href=\"{path}\"")), "{path}");
        assert_eq!(fetch(&app, path, "*/*").await.0, StatusCode::OK);
    }
    // Crawlers may inspect only the first few distinct documentation links.
    // Put the API guide before the machine-readable subsection indexes.
    let mut seen = std::collections::HashSet::new();
    let first_docs: Vec<_> = html
        .split("href=\"")
        .skip(1)
        .filter_map(|part| part.split_once('"').map(|(href, _)| href))
        .filter(|href| *href == "/docs" || href.starts_with("/docs/"))
        .filter(|href| seen.insert(*href))
        .take(3)
        .collect();
    assert!(
        first_docs.contains(&crate::data::routes::API_GUIDE),
        "{first_docs:?}"
    );
    for url in [
        site::LINKEDIN_URL,
        site::PYTHON_SDK_URL,
        site::TYPESCRIPT_SDK_URL,
        site::MCP_REGISTRY_URL,
        site::MCP_GLAMA_URL,
    ] {
        assert!(footer.contains(&format!("href=\"{url}\"")));
        assert!(markdown.contains(&format!("]({url})")));
    }
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
    let body = html.split_once("<body").unwrap().1;
    let videos: Vec<_> = graph
        .iter()
        .filter(|entity| entity["@type"] == "VideoObject")
        .collect();
    assert_eq!(videos.len(), 2);
    for video in videos {
        let url = video["url"].as_str().unwrap();
        assert!(body.contains(&format!("href=\"{url}\"")));
        assert!(markdown.contains(&format!("]({url})")));
        assert!(video["thumbnailUrl"].is_string());
        assert!(video["embedUrl"].is_string());
    }
    let charts = graph
        .iter()
        .find(|entity| entity["@type"] == "ItemList")
        .unwrap();
    let images = charts["itemListElement"].as_array().unwrap();
    assert_eq!(charts["numberOfItems"], images.len());
    assert_eq!(
        images.len(),
        body.matches("src=\"/static/images/benchmarks/").count()
    );
    for image in images {
        assert_eq!(image["@type"], "ImageObject");
        let path = image["contentUrl"]
            .as_str()
            .unwrap()
            .strip_prefix(crate::utils::site_url::public_site_url())
            .unwrap();
        assert!(body.contains(&format!("src=\"{path}\"")));
        assert!(markdown.contains(&format!("]({path})")));
    }
    assert!(graph
        .iter()
        .any(|entity| entity["@type"] == "Organization" && entity["contactPoint"].is_array()));
    assert!(graph
        .iter()
        .any(|entity| entity["@type"] == "SoftwareApplication" && entity["sameAs"].is_array()));
    let source = graph
        .iter()
        .find(|entity| entity["@type"] == "SoftwareSourceCode")
        .unwrap();
    assert_eq!(source["codeRepository"], site::GITHUB_URL);
    assert_eq!(source["license"], site::LICENSE_URL);
    let repository_link = format!("href=\"{}\"", site::GITHUB_URL);
    assert!(before_footer.contains(&repository_link));
    assert!(footer.contains(&repository_link));
    let organization = graph
        .iter()
        .find(|entity| entity["@type"] == "Organization")
        .unwrap();
    assert_eq!(organization["name"], site::ORGANIZATION);
    assert!(organization["sameAs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|profile| profile == site::LINKEDIN_URL));
    assert_eq!(
        organization["contactPoint"][0]["email"],
        site::PRIVACY_EMAIL
    );
    assert_eq!(organization["contactPoint"][1]["email"], site::LEGAL_EMAIL);
    let faq = graph
        .iter()
        .find(|entity| entity["@type"] == "FAQPage")
        .unwrap();
    let questions = faq["mainEntity"].as_array().unwrap();
    assert_eq!(questions.len(), crate::pages::home::faq::FAQS.len());
    let markdown_text: String = pulldown_cmark::Parser::new(&markdown)
        .filter_map(|event| {
            if let pulldown_cmark::Event::Text(text) = event {
                Some(text.into_string())
            } else {
                None
            }
        })
        .collect();
    for question in questions {
        assert!(html.contains(question["name"].as_str().unwrap()));
        assert!(markdown_text.contains(question["acceptedAnswer"]["text"].as_str().unwrap()));
    }
    for path in ["/about", "/contact", "/docs/api", "/llms.txt"] {
        assert!(html.contains(&format!("href=\"{path}\"")));
        assert_eq!(fetch(&app, path, "*/*").await.0, StatusCode::OK);
    }
}

#[tokio::test]
async fn tool_openapi_only_advertises_typed_json_reads() {
    let app = super::app();
    let spec: Value =
        serde_json::from_str(&fetch(&app, "/openapi.json", "application/json").await.2).unwrap();
    let paths = spec["paths"].as_object().unwrap();
    assert_eq!(paths.len(), 4);
    let mut ids = std::collections::HashSet::new();
    for (path, item) in paths {
        for (method, operation) in item.as_object().unwrap() {
            assert!(method == "get" || (path == "/api/v1/search" && method == "post"));
            assert!(ids.insert(operation["operationId"].as_str().unwrap()));
            assert!(
                operation["responses"]["200"]["content"]["application/json"]["schema"].is_object()
            );
        }
    }
    assert_eq!(ids.len(), 5);
    let post = &paths["/api/v1/search"]["post"];
    let input_ref = post["requestBody"]["content"]["application/json"]["schema"]["$ref"]
        .as_str()
        .unwrap()
        .strip_prefix('#')
        .unwrap();
    let input = spec.pointer(input_ref).unwrap();
    assert_eq!(input["type"], "object");
    assert_eq!(input["required"], json!(["q"]));
    assert_eq!(input["additionalProperties"], false);
    assert_eq!(input["properties"]["q"]["maxLength"], 256);
    assert_eq!(input["properties"]["limit"]["maximum"], 50);
    let key = post["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["name"] == "Idempotency-Key")
        .unwrap();
    assert_eq!(key["in"], "header");
    assert_eq!(key["required"], false);
    assert!(key["schema"]["pattern"].is_string());
    for status in ["413", "415", "422", "503"] {
        assert!(
            post["responses"][status]["content"]["application/problem+json"]["schema"].is_object()
        );
    }
    assert!(paths.contains_key("/api/v1/search"));
    assert!(paths.contains_key("/api/v1/docs/page"));
    assert_eq!(
        fetch(&app, "/openapi/site.json", "*/*").await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn documentation_exposes_its_own_article_and_navigation_metadata() {
    let app = super::app();
    for (path, kind) in [
        ("/docs/api", "APIReference"),
        ("/docs/rsjs/signals", "TechArticle"),
    ] {
        let (status, _, html) = fetch(&app, path, "text/html").await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let json = html
            .split_once("<script type=\"application/ld+json\">")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        let data: Value = serde_json::from_str(json).unwrap();
        let page = crate::endpoints::api::page(path).unwrap();
        assert_eq!(data["@graph"][0]["@type"], kind);
        assert_eq!(data["@graph"][0]["headline"], page.title);
        assert!(data["@graph"][0]["url"].as_str().unwrap().ends_with(path));
        let crumbs = data["@graph"][1]["itemListElement"].as_array().unwrap();
        assert_eq!(crumbs.last().unwrap()["name"], page.title);
        for (position, crumb) in crumbs.iter().enumerate() {
            assert_eq!(crumb["position"], position + 1);
        }
    }
}

#[tokio::test]
async fn section_indexes_only_link_their_public_documentation() {
    let app = super::app();
    for prefix in ["/docs/framework", "/docs/rsjs"] {
        let (status, _, index) = fetch(&app, &format!("{prefix}/llms.txt"), "text/plain").await;
        assert_eq!(status, StatusCode::OK);
        let expected = crate::pages::docs::docs().pages.iter().filter(|page| {
            !page.frontmatter.hidden
                && (page.route_path == prefix
                    || page
                        .route_path
                        .strip_prefix(prefix)
                        .is_some_and(|tail| tail.starts_with('/')))
        });
        let mut count = 0;
        for page in expected {
            assert!(index.contains(page.markdown_path));
            count += 1;
        }
        assert!(count > 1);
        assert_eq!(index.matches("\n- [").count(), count);
    }
}

#[tokio::test]
async fn info_pages_share_html_and_markdown_routes() {
    let app = super::app();
    for (path, markdown_path) in [("/about", "/about.md"), ("/contact", "/contact.md")] {
        let (status, headers, html) = fetch(&app, path, "text/html").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert_eq!(headers[header::VARY], "Accept");
        assert!(html.contains("data-info-page"));

        let (status, headers, markdown) = fetch(&app, path, "text/markdown").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert_eq!(headers[header::VARY], "Accept");
        assert!(markdown.starts_with("# "));
        assert!(!markdown.contains("<html"));
        assert_eq!(markdown, fetch(&app, markdown_path, "*/*").await.2);
        assert_eq!(
            html,
            fetch(&app, path, "text/markdown;q=0, text/html").await.2
        );
        let links: &[&str] = if path == "/about" {
            &[site::GITHUB_URL]
        } else {
            &[
                site::ISSUES_URL,
                site::DISCORD_URL,
                site::PRIVACY_MAILTO,
                site::LEGAL_MAILTO,
            ]
        };
        for href in links {
            assert!(html.contains(&format!("href=\"{href}\"")));
            assert!(markdown.contains(&format!("]({href})")));
        }
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
            .filter_map(|url| {
                url.strip_prefix(origin)
                    .or_else(|| url.starts_with('/').then_some(url))
            })
            .map(|path| path.split('#').next().unwrap())
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
async fn generated_contracts_keep_operation_ids_errors_and_resolvable_schemas() {
    fn check_refs(value: &Value, root: &Value) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                    let pointer = reference.strip_prefix('#').expect("local schema reference");
                    assert!(
                        root.pointer(pointer).is_some(),
                        "unresolved reference: {reference}"
                    );
                }
                for child in object.values() {
                    check_refs(child, root);
                }
            }
            Value::Array(array) => {
                for child in array {
                    check_refs(child, root);
                }
            }
            _ => {}
        }
    }

    let app = super::app();
    let spec: Value = serde_json::from_str(
        &fetch(&app, "/openapi/site.json", "application/json")
            .await
            .2,
    )
    .unwrap();
    assert_eq!(spec["openapi"], "3.1.0");
    assert!(spec["externalDocs"]["description"].is_string());
    for path in spec["paths"].as_object().unwrap().values() {
        for operation in path.as_object().unwrap().values() {
            if let Some(responses) = operation["responses"].as_object() {
                for response in responses.values() {
                    if response.get("$ref").is_none() {
                        assert!(
                            response["description"].is_string(),
                            "OpenAPI 3.1 requires response descriptions"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(
        spec["paths"]["/api/search"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["type"],
        "string"
    );
    check_refs(&spec, &spec);
    for (path, method, operation_id) in [
        ("/api/v1/search", "get", "searchDocs"),
        ("/api/v1/search", "post", "searchDocsPost"),
        ("/api/v1/docs", "get", "listDocs"),
        ("/api/v1/docs/page", "get", "getDoc"),
        ("/api/search", "get", "searchProaDocumentation"),
        ("/api/search", "post", "searchProaDocumentationPost"),
    ] {
        let operation = &spec["paths"][path][method];
        assert_eq!(operation["operationId"], operation_id);
        assert!(operation["description"].is_string());
        for status in ["200", "400", "404", "405", "429"] {
            assert!(operation["responses"][status]["headers"]["RateLimit"].is_object());
        }
        assert!(operation["responses"]["429"]["headers"]["Retry-After"].is_object());
        assert!(operation["responses"]["429"]["content"]["application/problem+json"].is_object());
    }
    // Axum's legacy extractor errors differ from the typed v1 problem response.
    for (path, content_type) in [
        ("/api/v1/search", "application/problem+json"),
        ("/api/search", "text/plain"),
    ] {
        assert!(
            spec["paths"][path]["get"]["responses"]["400"]["content"][content_type].is_object()
        );
    }
    let tools = rpc(&app, "tools/list", json!({})).await;
    for tool in tools["result"]["tools"].as_array().unwrap() {
        let input = &tool["inputSchema"];
        assert_eq!(input["additionalProperties"], false);
        check_refs(input, input);
        check_refs(&tool["outputSchema"], &tool["outputSchema"]);
        if tool["name"] == "search_docs" {
            let required = |schema: &Value| {
                schema["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|field| field.as_str().unwrap().to_owned())
                    .collect::<std::collections::BTreeSet<_>>()
            };
            assert_eq!(
                required(&tool["outputSchema"]),
                required(&spec["components"]["schemas"]["DocumentList"])
            );
        }
    }
}

#[tokio::test]
async fn retired_documents_are_readable_but_not_searchable_over_http_or_mcp() {
    let app = super::app();
    let retired: Vec<_> = super::pages::docs::docs()
        .pages
        .iter()
        .filter(|page| !page.frontmatter.hidden && page.frontmatter.search == Some(false))
        .collect();
    assert!(
        !retired.is_empty(),
        "exercise the compiled retired-guide fixtures"
    );
    for page in retired {
        let mut query = reqwest::Url::parse("http://localhost/").unwrap();
        query
            .query_pairs_mut()
            .append_pair("q", page.title)
            .append_pair("limit", "50");
        let response: Value = serde_json::from_str(
            &fetch(
                &app,
                &format!("/api/v1/search?{}", query.query().unwrap()),
                "application/json",
            )
            .await
            .2,
        )
        .unwrap();
        assert!(response["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|doc| doc["path"] != page.route_path));
        let tools = rpc(
            &app,
            "tools/call",
            json!({"name":"search_docs", "arguments":{"q":page.title,"limit":50}}),
        )
        .await;
        assert!(tools["result"]["structuredContent"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|doc| doc["path"] != page.route_path));
        let legacy: Value = serde_json::from_str(
            &fetch(
                &app,
                &format!("/api/search?{}", query.query().unwrap()),
                "application/json",
            )
            .await
            .2,
        )
        .unwrap();
        assert!(legacy["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|doc| doc["url"] != page.route_path));
        // Search exclusion is not access control: an existing link still works.
        assert_eq!(
            fetch(&app, page.markdown_path, "text/markdown").await.0,
            StatusCode::OK
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
async fn json_search_replays_successes_and_rejects_key_reuse() {
    let app = super::app();
    let get = fetch(&app, "/api/v1/search?q=render&limit=2", "application/json")
        .await
        .2;
    for (payload, key, replayed) in [
        (r#"{"q":"render","limit":2}"#, None, None),
        (
            r#"{"q":"render","limit":2}"#,
            Some("\"http-retry\""),
            Some("false"),
        ),
        (
            r#"{ "limit": 2, "q": "render" }"#,
            Some("\"http-retry\""),
            Some("true"),
        ),
    ] {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/search")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(payload)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("idempotency-replayed")
                .map(|value| value.to_str().unwrap()),
            replayed
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(response.headers().contains_key("ratelimit"));
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(body.as_ref(), get.as_bytes());
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/search")
                .header(header::CONTENT_TYPE, "application/json")
                .header("Idempotency-Key", "\"http-retry\"")
                .body(Body::from(r#"{"q":"render","limit":3}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let problem: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(problem["code"], "idempotency_key_reused");
}

#[tokio::test]
async fn json_search_reports_typed_input_errors_and_does_not_cache_failures() {
    let app = super::app();
    for (payload, media_type, key, status) in [
        (
            r#"{"q":"render"}"#.to_owned(),
            "text/plain",
            "\"input-tests\"",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "{".to_owned(),
            "application/json",
            "\"input-tests\"",
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"q":"render","extra":true}"#.to_owned(),
            "application/json",
            "\"input-tests\"",
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"q":"render","limit":0}"#.to_owned(),
            "application/json",
            "\"input-tests\"",
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"q":"render","cursor":"invalid"}"#.to_owned(),
            "application/json",
            "\"input-tests\"",
            StatusCode::BAD_REQUEST,
        ),
        (
            " ".repeat(16 * 1024 + 1),
            "application/json",
            "\"input-tests\"",
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            r#"{"q":"render"}"#.to_owned(),
            "application/json",
            "unquoted",
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"q":"render"}"#.to_owned(),
            "application/json",
            "\"input-tests\"",
            StatusCode::OK,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/search")
                    .header(header::CONTENT_TYPE, media_type)
                    .header("Idempotency-Key", key)
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == StatusCode::OK {
            assert_eq!(response.headers()["idempotency-replayed"], "false");
        } else {
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "application/problem+json"
            );
            assert!(!response.headers().contains_key("idempotency-replayed"));
            let problem: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            assert_eq!(problem["status"], status.as_u16());
        }
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
            if path == "/api/v1/search" && method == Method::POST {
                continue;
            }
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
                let allowed = response.headers()[header::ALLOW].to_str().unwrap();
                let methods: std::collections::HashSet<_> = allowed.split(',').collect();
                let expected: std::collections::HashSet<_> = if path == "/api/v1/search" {
                    ["GET", "HEAD", "POST"].into_iter().collect()
                } else {
                    ["GET", "HEAD"].into_iter().collect()
                };
                assert_eq!(methods, expected);
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
    let registry_manifest: Value = serde_json::from_str(include_str!("../server.json")).unwrap();
    assert_eq!(
        registry_manifest["version"], initialized["result"]["serverInfo"]["version"],
        "the published MCP manifest must describe the running server version"
    );
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
