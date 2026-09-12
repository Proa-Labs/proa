// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod common;
use common::Harness;
use mcp_incident_desk::{
    auth::tokens,
    components::{markdown, IssueReport},
    loader::RequestScope,
};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn request_scopes_deduplicate_and_async_markdown_reuses_the_sync_tree() {
    let h = Harness::new().await;
    let grant = h.tokens("alice", "aster", false).await;
    let principal = tokens::authenticate(&h.state, &grant.access_token)
        .await
        .unwrap();
    for _ in 0..2 {
        let scope = RequestScope::new(&h.state.db, principal.clone())
            .await
            .unwrap();
        let projection = scope.report("AST-1042", &h.app).await.unwrap();
        assert_eq!(scope.inner.reads.load(Ordering::Relaxed), 2);
        let sync = markdown(&projection).unwrap();
        // Async composition is kept outside Send-bound MCP handlers: MdContext uses Rc.
        let report = proa_core::to_md_async(IssueReport::new(&projection));
        let mut bytes = Vec::new();
        let mut context = proa_core::md::MdContext::new(&mut bytes);
        use proa_core::MdRender;
        MdRender::render_md(report, &mut context)
            .resolve()
            .await
            .unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), sync);
    }
}
