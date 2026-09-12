// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod common;
use common::Harness;
use mcp_incident_desk::{
    auth::tokens,
    components::{markdown, IssueReport},
    loader::RequestScope,
};
use proa_core::{WebContext, WebRenderSync};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

#[tokio::test]
async fn brief_preserves_evidence_and_composes_safe_structured_blocks() {
    let h = Harness::new().await;
    let grant = h.tokens("alice", "aster", true).await;
    let p = tokens::authenticate(&h.state, &grant.access_token)
        .await
        .unwrap();
    let scope = RequestScope::new(&h.state.db, p).await.unwrap();
    let mut d = scope.report("AST-1042", &h.app).await.unwrap();
    d.issue.title = "Connection <lost> [not a link](javascript:bad)".into();
    d.events =
        vec!["Error: `lease` unavailable\n```\n# forged heading\n<script>alert(1)</script>".into()];
    d.runbook_steps = vec![
        "Check pool <capacity>".into(),
        "Verify **literal** sample".into(),
    ];
    let md = markdown(&d).unwrap();
    let mut headings = Vec::new();
    let mut heading = None;
    let mut code = String::new();
    let mut in_code = false;
    let mut links = Vec::new();
    let mut ordered_lists = 0;
    for event in Parser::new(&md) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => headings.push(heading.take().unwrap()),
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::List(Some(1))) => ordered_lists += 1,
            Event::Start(Tag::Link { dest_url, .. }) => links.push(dest_url.to_string()),
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = &mut heading {
                    h.push_str(&t);
                }
                if in_code {
                    code.push_str(&t);
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => panic!("untrusted HTML escaped its context"),
            _ => {}
        }
    }
    assert_eq!(headings[0], d.issue.title);
    assert!(!headings.iter().any(|h| h == "forged heading"));
    assert_eq!(code.trim_end(), d.events[0]);
    assert_eq!(links, [d.url.as_str()]);
    assert_eq!(ordered_lists, 1);
    assert!(md.contains("expected_version"));
    assert!(
        !md.contains("\n\n\n"),
        "component boundaries added excessive whitespace"
    );
    let mut cx = WebContext::new();
    IssueReport::new(&d).render(&mut cx).unwrap();
    let html = cx.into_output().into_string();
    assert!(!html.contains("<script>"));
    assert_eq!(html.matches("data-activity-log").count(), 1);
    assert!(html.contains("data-badge"));
    assert!(html.contains("status-open"));
    d.can_resolve = false;
    let readonly = markdown(&d).unwrap();
    assert!(!readonly.contains("expected_version"));
    d.issue.status = "resolved".into();
    let mut cx = WebContext::new();
    IssueReport::new(&d).render(&mut cx).unwrap();
    assert!(cx.into_output().into_string().contains("status-resolved"));
    assert!(!markdown(&d).unwrap().contains("expected_version"));
}

#[tokio::test]
async fn screens_use_registry_components_and_keep_form_and_navigation_contracts() {
    let h = Harness::new().await;
    let browser = h.login_browser("alice", "aster").await;
    let listing = browser
        .get(format!("{}/issues?query=Checkout&status=open", h.app))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    for hook in [
        "data-table",
        "data-table-header",
        "data-table-body",
        "data-table-row",
        "data-badge",
        "data-button",
    ] {
        assert!(listing.contains(hook), "missing registry component {hook}");
    }
    assert!(listing.contains("value=\"Checkout\""));
    assert!(listing.contains("aria-current=\"page\""));
    assert!(listing.contains("href=\"#main-content\""));
    assert!(listing.contains("id=\"main-content\""));
    assert!(listing.contains("scope=\"col\""));
    let detail = browser
        .get(format!("{}/issues/AST-1042", h.app))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    for hook in [
        "data-card",
        "data-textarea",
        "data-button",
        "name=\"csrf\"",
        "name=\"expected_version\"",
        "name=\"idempotency_key\"",
        "for=\"resolution-note\"",
        "id=\"resolution-note\"",
        "maxlength=\"2000\"",
    ] {
        assert!(detail.contains(hook), "missing form contract {hook}");
    }
    assert_eq!(detail.matches("data-activity-log").count(), 1);
    for asset in ["/static/styles.css", "/static/incident.css"] {
        assert!(detail.contains(asset));
    }
}
