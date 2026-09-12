// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod common;
use common::Harness;
use mcp_incident_desk::{auth::tokens, domain};
use rmcp::model::CallToolRequestParams;
use serde_json::{json, Value};

fn input() -> Value {
    json!({
        "title": "Pool <retry> & saturation",
        "project_id": "checkout",
        "release_id": "checkout-2026.09.08.3",
        "runbook_id": "pool",
        "severity": "high",
        "affected_users": 42,
        "event_count": 105,
        "event_samples": ["PoolWait: 2100ms", "<script>alert('observed telemetry')</script>"],
        "note": "Opened from the authenticated MCP client.",
        "idempotency_key": "create-regression-001"
    })
}

fn call(name: &'static str, args: Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())
}

#[tokio::test]
async fn mcp_creation_is_atomic_idempotent_and_visible_in_html_and_markdown() {
    let h = Harness::new().await;
    let token = h.tokens("alice", "aster", true).await;
    let sdk = h.sdk(&token.access_token).await;
    let request = input();
    let (a, b) = tokio::join!(
        sdk.call_tool(call("create_issue", request.clone())),
        sdk.call_tool(call("create_issue", request.clone())),
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.is_error, Some(false));
    assert_eq!(b.is_error, Some(false));
    assert_eq!(a.structured_content, b.structured_content);
    let result = a.structured_content.unwrap();
    let id = result["issue_id"].as_str().unwrap();
    assert_eq!(result["version"], 1);
    let data = sdk
        .call_tool(call("get_issue_data", json!({"issue_id":id})))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(data["issue"]["title"], request["title"]);
    assert_eq!(data["issue"]["workspace_id"], "aster");
    assert_eq!(data["issue"]["owner_id"], "alice");
    assert_eq!(data["issue"]["status"], "open");
    assert_eq!(data["events"], request["event_samples"]);
    assert_eq!(data["activity"].as_array().unwrap().len(), 1);
    assert_eq!(data["activity"][0]["id"], result["audit_id"]);
    assert_eq!(data["activity"][0]["user_name"], "Alice");
    assert_eq!(data["activity"][0]["client_id"], "incident-demo-client");
    let brief = sdk
        .call_tool(call("get_issue_brief", json!({"issue_id":id})))
        .await
        .unwrap();
    let content = serde_json::to_value(brief.content).unwrap();
    let markdown = content[0]["text"].as_str().unwrap();
    let plain: String = pulldown_cmark::Parser::new(markdown)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                Some(text.to_string())
            }
            _ => None,
        })
        .collect();
    assert!(plain.contains(request["title"].as_str().unwrap()));
    let browser = h.login_browser("alice", "aster").await;
    let html = browser
        .get(format!("{}/issues/{id}", h.app))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("Pool &lt;retry&gt; &amp; saturation"));
    assert!(!html.contains("<script>"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM issues")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(count, 13);
    let mut changed = request;
    changed["title"] = json!("Different payload with the same key");
    assert_eq!(
        sdk.call_tool(call("create_issue", changed))
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    let _ = sdk.cancel().await;
}

#[tokio::test]
async fn creation_enforces_workspace_scope_bounds_and_current_grants() {
    let h = Harness::new().await;
    let alice = h.tokens("alice", "aster", true).await;
    let viewer = h.tokens("ben", "aster", true).await;
    let readonly = h.tokens("alice", "aster", false).await;
    let outsider = h.tokens("casey", "harbor", true).await;
    sqlx::query("INSERT INTO projects VALUES('aster','aster-only','Private project')")
        .execute(&h.state.db)
        .await
        .unwrap();
    let mut private = input();
    private["project_id"] = json!("aster-only");
    let p = tokens::authenticate(&h.state, &outsider.access_token)
        .await
        .unwrap();
    assert!(
        domain::create(&h.state.db, &p, serde_json::from_value(private).unwrap())
            .await
            .is_err()
    );
    for access in [&viewer.access_token, &readonly.access_token] {
        let response = h.browser().post(h.state.config.resource()).bearer_auth(access)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_issue","arguments":input()}}))
            .send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        let principal = tokens::authenticate(&h.state, access).await.unwrap();
        assert!(domain::create(
            &h.state.db,
            &principal,
            serde_json::from_value(input()).unwrap()
        )
        .await
        .is_err());
    }
    let principal = tokens::authenticate(&h.state, &alice.access_token)
        .await
        .unwrap();
    for (field, value) in [
        ("title", json!(" ")),
        ("title", json!("x".repeat(201))),
        ("event_samples", json!(vec!["event"; 11])),
        ("event_samples", json!(["x".repeat(2001)])),
        ("note", json!("")),
        ("idempotency_key", json!("short")),
        ("release_id", json!("missing-release")),
        ("runbook_id", json!("missing-runbook")),
    ] {
        let mut bad = input();
        bad[field] = value;
        assert!(domain::create(
            &h.state.db,
            &principal,
            serde_json::from_value(bad).unwrap()
        )
        .await
        .is_err());
    }
    for (field, value) in [
        ("severity", json!("invalid")),
        ("affected_users", json!(-1)),
        ("workspace_id", json!("harbor")),
    ] {
        let mut bad = input();
        bad[field] = value;
        assert!(serde_json::from_value::<domain::CreateIssueInput>(bad).is_err());
    }
    sqlx::query("UPDATE grants SET revoked=1 WHERE id=?")
        .bind(&principal.credential_id)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert!(domain::create(
        &h.state.db,
        &principal,
        serde_json::from_value(input()).unwrap()
    )
    .await
    .is_err());
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM issues),(SELECT count(*) FROM activity),(SELECT count(*) FROM idempotency)").fetch_one(&h.state.db).await.unwrap();
    assert_eq!(counts, (12, 0, 0));
}
