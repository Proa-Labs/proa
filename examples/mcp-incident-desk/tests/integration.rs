// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod common;
use common::{hidden, Harness};
use mcp_incident_desk::{
    auth::{form, oauth_form, tokens},
    components::{markdown, IssueReport},
    domain,
    loader::RequestScope,
    security::random,
};
use proa_core::{WebContext, WebRenderSync};
use rmcp::model::CallToolRequestParams;
#[test]
fn oauth_repeated_resource_preserves_audience_and_parameter_binding() {
    let parsed = oauth_form(
        b"resource=https%3A%2F%2Flocalhost%2Fmcp&resource=https%3A%2F%2Flocalhost%2Fmcp",
    )
    .unwrap();
    assert_eq!(parsed["resource"], "https://localhost/mcp");
    assert!(
        oauth_form(b"resource=https://localhost/mcp&resource=https://other.example/mcp").is_err()
    );
    for parameter in [
        "state",
        "code",
        "code_verifier",
        "client_id",
        "redirect_uri",
        "scope",
    ] {
        assert!(oauth_form(format!("{parameter}=same&{parameter}=same").as_bytes()).is_err());
    }
    assert!(form(b"resource=one&resource=one").is_err());
}
#[tokio::test]
async fn expired_forms_report_expiry_without_authorizing_a_missing_cookie() {
    let h = Harness::new().await;
    let browser = h.browser();
    let start = browser
        .get(format!("{}/auth/login", h.app))
        .send()
        .await
        .unwrap();
    let page = browser
        .get(start.headers()["location"].to_str().unwrap())
        .send()
        .await
        .unwrap();
    let flow = hidden(&page.text().await.unwrap(), "flow");
    let missing_cookie = h.browser();
    for path in ["/authorize/login", "/consent"] {
        let denied = missing_cookie
            .post(format!("{}{path}", h.issuer))
            .header("origin", &h.issuer)
            .form(&[("flow", &flow)])
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
    }
    sqlx::query("UPDATE pending SET expires=0 WHERE kind='authorize'")
        .execute(&h.state.db)
        .await
        .unwrap();
    for path in ["/authorize/login", "/consent"] {
        let expired = missing_cookie
            .post(format!("{}{path}", h.issuer))
            .header("origin", &h.issuer)
            .form(&[("flow", &flow)])
            .send()
            .await
            .unwrap();
        assert_eq!(expired.status(), reqwest::StatusCode::BAD_REQUEST);
    }
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM grants")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!((sessions, grants), (0, 0));
}
#[tokio::test]
async fn authenticated_browser_and_sdk_share_data_and_permissions() {
    let h = Harness::new().await;
    let browser = h.login_browser("alice", "aster").await;
    let page = browser
        .get(format!("{}/issues/AST-1042", h.app))
        .send()
        .await
        .unwrap();
    assert!(page.status().is_success());
    assert_eq!(page.headers()["cache-control"], "no-store");
    assert_eq!(page.headers()["referrer-policy"], "same-origin");
    let html = page.text().await.unwrap();
    assert!(html.contains("data-issue-report"));
    assert!(html.contains("Record a resolution"));
    let tokens = h.tokens("alice", "aster", true).await;
    let sdk = h.sdk(&tokens.access_token).await;
    let tools = sdk.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 6);
    let brief = sdk
        .call_tool(
            CallToolRequestParams::new("get_issue_brief").with_arguments(
                serde_json::json!({"issue_id":"AST-1042"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(brief.is_error, Some(false));
    assert!(brief.structured_content.is_none());
    let text = serde_json::to_value(&brief.content).unwrap()[0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    let plain: String = pulldown_cmark::Parser::new(&text)
        .filter_map(|e| match e {
            pulldown_cmark::Event::Text(t) | pulldown_cmark::Event::Code(t) => Some(t.to_string()),
            _ => None,
        })
        .collect();
    assert!(plain.contains("AST-1042"));
    assert!(text.contains("148"));
    let data = sdk
        .call_tool(
            CallToolRequestParams::new("get_issue_data").with_arguments(
                serde_json::json!({"issue_id":"AST-1042"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        data.structured_content.as_ref().unwrap()["issue"]["version"],
        7
    );
    let args = serde_json::json!({"issue_id":"AST-1042","expected_version":7,"note":"Verified recovery in the isolated test","idempotency_key":"test-resolution-01"});
    let result = sdk
        .call_tool(
            CallToolRequestParams::new("resolve_issue")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.structured_content.as_ref().unwrap()["version"], 8);
    let replay = sdk
        .call_tool(
            CallToolRequestParams::new("resolve_issue")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.structured_content, replay.structured_content);
    let data: serde_json::Value = browser
        .get(format!("{}/issues/AST-1042.json", h.app))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(data["issue"]["status"], "resolved");
    assert_eq!(data["activity"][0]["user_name"], "Alice");
    assert_eq!(data["activity"][0]["client_id"], "incident-demo-client");
    let viewer = h.tokens("ben", "aster", true).await;
    assert!(!viewer.scope.contains("issues:write"));
    let v = h.sdk(&viewer.access_token).await;
    let read = v
        .call_tool(
            CallToolRequestParams::new("get_issue_data").with_arguments(
                serde_json::json!({"issue_id":"AST-1042"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        read.structured_content.as_ref().unwrap()["can_resolve"],
        false
    );
    let denied=h.browser().post(h.state.config.resource()).bearer_auth(&viewer.access_token).json(&serde_json::json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"resolve_issue","arguments":args}})).send().await.unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
    let outsider = h.tokens("casey", "harbor", true).await;
    let c = h.sdk(&outsider.access_token).await;
    let denied = c
        .call_tool(
            CallToolRequestParams::new("get_issue_data").with_arguments(
                serde_json::json!({"issue_id":"AST-1042"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(denied.is_error, Some(true));
    assert!(!serde_json::to_string(&denied).unwrap().contains("Checkout"));
    let connections = browser
        .get(format!("{}/connections", h.app))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let grant = hidden(&connections, "grant");
    let csrf = hidden(&connections, "csrf");
    let r = browser
        .post(format!("{}/connections/revoke", h.app))
        .header("origin", &h.app)
        .form(&[("grant", grant), ("csrf", csrf)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::SEE_OTHER);
    assert!(tokens::authenticate(&h.state, &tokens.access_token)
        .await
        .is_err());
    let reopened = mcp_incident_desk::app::AppState::open((*h.state.config).clone())
        .await
        .unwrap();
    assert!(tokens::authenticate(&reopened, &tokens.access_token)
        .await
        .is_err());
    let status: String = sqlx::query_scalar(
        "SELECT status FROM issues WHERE workspace_id='aster' AND id='AST-1042'",
    )
    .fetch_one(&reopened.db)
    .await
    .unwrap();
    assert_eq!(status, "resolved");
    let _ = sdk.cancel().await;
    let _ = v.cancel().await;
    let _ = c.cancel().await;
}
#[tokio::test]
async fn oauth_crypto_rotation_and_request_boundaries() {
    let h = Harness::new().await;
    let (code, verifier) = h.code("alice", "aster", true).await;
    assert!(!h.exchange(&code, &random()).await.status().is_success());
    let (a, b) = tokio::join!(h.exchange(&code, &verifier), h.exchange(&code, &verifier));
    assert_ne!(a.status().is_success(), b.status().is_success());
    let t: mcp_incident_desk::auth::tokens::TokenResponse = if a.status().is_success() {
        a.json().await.unwrap()
    } else {
        b.json().await.unwrap()
    };
    assert!(tokens::authenticate(&h.state, &t.access_token)
        .await
        .is_ok());
    let mut corrupt = t.access_token.clone().into_bytes();
    let i = corrupt.len() - 12;
    corrupt[i] = if corrupt[i] == b'A' { b'B' } else { b'A' };
    assert!(
        tokens::authenticate(&h.state, std::str::from_utf8(&corrupt).unwrap())
            .await
            .is_err()
    );
    assert!(tokens::decode(&h.state, &t.access_token, "wrong-audience", false).is_err());
    assert!(tokens::decode(&h.state, &t.access_token, "incident-web", true).is_err());
    let refreshed = h
        .browser()
        .post(format!("{}/token", h.issuer))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "incident-demo-client"),
            ("refresh_token", t.refresh_token.as_str()),
            ("resource", h.state.config.resource().as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert!(refreshed.status().is_success());
    let next: mcp_incident_desk::auth::tokens::TokenResponse = refreshed.json().await.unwrap();
    assert_ne!(t.refresh_token, next.refresh_token);
    let replay = h
        .browser()
        .post(format!("{}/token", h.issuer))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "incident-demo-client"),
            ("refresh_token", t.refresh_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert!(!replay.status().is_success());
    assert!(tokens::authenticate(&h.state, &next.access_token)
        .await
        .is_err());
    let page = h
        .browser()
        .get(format!("{}/issues/AST-1042.json", h.app))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), reqwest::StatusCode::UNAUTHORIZED);
    for origin in ["https://attacker.invalid", "null"] {
        for path in ["/authorize/login", "/consent"] {
            let wrong_origin = h
                .browser()
                .post(format!("{}{path}", h.issuer))
                .header("origin", origin)
                .form(&[("user", "alice"), ("password", common::PASSWORD)])
                .send()
                .await
                .unwrap();
            assert_eq!(wrong_origin.status(), reqwest::StatusCode::FORBIDDEN);
        }
    }
    assert!(form(b"client_id=one&client_id=two").is_err());
    assert!(!mcp_incident_desk::auth::provider::valid_redirect(
        "https://valid.test/callback#fragment"
    ));
    assert!(!mcp_incident_desk::auth::provider::valid_redirect(
        "http://remote.test/callback"
    ));
    let browser = h.login_browser("ben", "aster").await;
    let page = browser
        .get(format!("{}/issues/AST-1042", h.app))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!page.contains("Record a resolution"));
    let denied = browser
        .post(format!("{}/issues/AST-1042/resolve", h.app))
        .header("origin", &h.app)
        .form(&[
            ("csrf", "forged"),
            ("expected_version", "7"),
            ("note", "forged"),
            ("idempotency_key", "forged-key"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn loader_isolation_escaping_and_transactional_conflicts() {
    let h = Harness::new().await;
    let t = h.tokens("alice", "aster", true).await;
    let p = tokens::authenticate(&h.state, &t.access_token)
        .await
        .unwrap();
    sqlx::query("UPDATE issues SET title=? WHERE workspace_id='aster' AND id='AST-1042'")
        .bind("<script>alert(1)</script> [fake](javascript:bad)")
        .execute(&h.state.db)
        .await
        .unwrap();
    let scope = RequestScope::new(&h.state.db, p.clone()).await.unwrap();
    let d = scope.report("AST-1042", &h.app).await.unwrap();
    assert_eq!(
        scope.inner.reads.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    let md = markdown(&d).unwrap();
    assert!(!md.contains("[fake](javascript:bad)"));
    assert!(pulldown_cmark::Parser::new(&md).any(|event| matches!(
        event,
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::List(Some(1)))
    )));
    let mut cx = WebContext::new();
    IssueReport::new(&d).render(&mut cx).unwrap();
    let html = cx.into_output().into_string();
    assert!(!html.contains("<script>alert"));
    assert!(html.contains("&lt;script"));
    let input = domain::ResolveInput {
        issue_id: "AST-1043".into(),
        expected_version: 7,
        note: "First verified fix".into(),
        idempotency_key: "concurrent-one".into(),
    };
    let mut other = input.clone();
    other.idempotency_key = "concurrent-two".into();
    let (a, b) = tokio::join!(
        domain::resolve(&h.state.db, &p, input.clone()),
        domain::resolve(&h.state.db, &p, other)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activity WHERE workspace_id='aster' AND issue_id='AST-1043'",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let outsider = h.tokens("casey", "harbor", false).await;
    let p = tokens::authenticate(&h.state, &outsider.access_token)
        .await
        .unwrap();
    let scope = RequestScope::new(&h.state.db, p).await.unwrap();
    assert!(scope.report("AST-1042", &h.app).await.is_err());
}
