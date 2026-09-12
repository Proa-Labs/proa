// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod common;
use common::Harness;

#[tokio::test]
async fn issue_representations_negotiate_without_bypassing_auth() {
    let h = Harness::new().await;
    let browser = h.login_browser("alice", "aster").await;
    let url = format!("{}/issues/AST-1042", h.app);
    for accept in [
        "text/html",
        "text/markdown;q=0, text/html",
        "text/markdown;q=0.5, text/html",
        "text/*",
        "text/markdown, text/html",
    ] {
        let r = browser
            .get(&url)
            .header("accept", accept)
            .send()
            .await
            .unwrap();
        assert!(
            r.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html"),
            "{accept}"
        );
        assert_eq!(r.headers()["vary"], "Accept");
        assert!(r.headers()["link"]
            .to_str()
            .unwrap()
            .contains("/issues/AST-1042.md"));
    }
    let explicit = browser
        .get(format!("{url}.md"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    for accept in ["text/markdown", "text/html;q=0.4, text/markdown;q=0.9"] {
        let r = browser
            .get(&url)
            .header("accept", accept)
            .send()
            .await
            .unwrap();
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/markdown"));
        assert_eq!(r.headers()["cache-control"], "no-store");
        assert_eq!(r.text().await.unwrap(), explicit);
    }
    for suffix in [".md", ".json"] {
        assert_eq!(
            h.browser()
                .get(format!("{url}{suffix}"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        h.browser()
            .get(&url)
            .header("accept", "text/markdown")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}
