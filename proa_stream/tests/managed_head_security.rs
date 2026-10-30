// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    head_fields::HeadUrlPolicy,
    managed_head::{HeadError, ManagedHead},
    *,
};
use std::sync::{Arc, Mutex};
type Output = Arc<Mutex<Vec<bytes::Bytes>>>;
fn setup(mount: &str, cdn: bool) -> (WebContext, ManagedHead, Output) {
    let output: Output = Arc::new(Mutex::new(Vec::new()));
    let sink = output.clone();
    let doc = ManagedHead::new(
        1024 * 1024,
        Arc::new(move |_, bytes| {
            sink.lock().unwrap().extend(bytes);
            Ok(())
        }),
    );
    let policy = HeadUrlPolicy::new("https://public.example", mount).unwrap();
    doc.configure_urls(if cdn {
        policy.allow_resource_origin("https://cdn.example").unwrap()
    } else {
        policy
    })
    .unwrap();
    let mut cx = WebContext::new();
    cx.head_mut().manage(doc.clone());
    (cx, doc, output)
}
fn wire(output: &Output) -> String {
    String::from_utf8(
        output
            .lock()
            .unwrap()
            .iter()
            .flat_map(|b| b.iter().copied())
            .collect(),
    )
    .unwrap()
}
fn emit(cx: &mut WebContext, doc: &ManagedHead) -> Result<(), WriteError> {
    cx.managed_seal()?;
    WebRenderSync::render(doc.outlet(), cx)?;
    doc.final_check()
}
#[derive(PartialEq, Eq)]
enum Script {
    Site,
    Second,
}
#[derive(PartialEq, Eq)]
enum OtherScript {
    Site,
}
#[test]
fn escapes_each_html_context_and_normalizes_meta_aliases() {
    let (mut cx, doc, out) = setup("/", false);
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().title("é </title><script>&\"").unwrap();
    cx.head_mut().meta("Description", "old").unwrap();
    cx.head_mut()
        .description("é \" onload=\"bad & <tag>")
        .unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    emit(&mut cx, &doc).unwrap();
    guard.finish(Ok(())).unwrap();
    let html = wire(&out);
    assert!(html.starts_with("<title>é &lt;/title&gt;&lt;script&gt;&amp;\"</title>"));
    assert_eq!(html.matches("<meta ").count(), 1);
    assert!(html.contains("content=\"é &quot; onload=&quot;bad &amp; &lt;tag&gt;\""));
    assert!(!html.contains("<script>"));
}
#[test]
fn trusted_origin_mount_and_explicit_cdn_define_url_resolution() {
    for mount in ["/", "/shop"] {
        let (mut cx, doc, out) = setup(mount, true);
        let hook = doc.begin_hook().unwrap();
        cx.head_mut()
            .canonical("/products/42?lang=en&view=full")
            .unwrap();
        cx.head_mut()
            .module(Script::Site, "/assets/site.js")
            .unwrap();
        cx.head_mut()
            .stylesheet(Script::Site, "https://cdn.example/style.css")
            .unwrap();
        let guard = hook.finish(Ok(())).unwrap();
        emit(&mut cx, &doc).unwrap();
        guard.finish(Ok(())).unwrap();
        let base = format!("https://public.example{}", mount.trim_end_matches('/'));
        let html = wire(&out);
        assert!(html.contains(&format!(
            "href=\"{base}/products/42?lang=en&amp;view=full\""
        )));
        assert!(html.contains(&format!("src=\"{base}/assets/site.js\"")));
        assert!(html.contains("href=\"https://cdn.example/style.css\""));
    }
}
#[test]
fn invalid_values_reject_atomically_without_logging_the_values() {
    let (mut cx, doc, out) = setup("/shop", false);
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().canonical("/valid").unwrap();
    cx.head_mut().module(Script::Site, "/good.js").unwrap();
    for value in [
        "javascript:alert(1)",
        "data:text/html,bad",
        "file:///secret",
        "//evil.example/a",
        "https://user:secret@evil.example/a",
        "/../outside",
        "/%2e%2e/outside",
        "\\evil.example/a",
        "\nhttps://evil.example",
        "https://evil.example/evil.js",
    ] {
        let error = cx.head_mut().module(Script::Site, value).unwrap_err();
        assert!(!format!("{error}").contains(value));
    }
    for value in ["javascript:alert(1)", "//evil.example/a", "/../outside"] {
        assert!(cx.head_mut().canonical(value).is_err());
    }
    assert!(cx.head_mut().meta("x\" content=\"bad", "ignored").is_err());
    let guard = hook.finish(Ok(())).unwrap();
    emit(&mut cx, &doc).unwrap();
    guard.finish(Ok(())).unwrap();
    let html = wire(&out);
    assert!(html.contains("/shop/valid\""));
    assert!(html.contains("/shop/good.js\""));
    assert_eq!(html.matches("<script ").count(), 1);
    assert!(!html.contains("<meta "));
}
#[test]
fn enum_types_and_resource_families_are_distinct_and_replacements_keep_order() {
    let (mut cx, doc, out) = setup("/", false);
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().module(Script::Site, "/old.js").unwrap();
    cx.head_mut().module(Script::Second, "/second.js").unwrap();
    cx.head_mut()
        .module(OtherScript::Site, "/other.js")
        .unwrap();
    cx.head_mut()
        .stylesheet(Script::Site, "/style.css")
        .unwrap();
    cx.head_mut().module(Script::Site, "/new.js").unwrap();
    cx.head_mut().remove_module(Script::Second).unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    emit(&mut cx, &doc).unwrap();
    guard.finish(Ok(())).unwrap();
    let html = wire(&out);
    assert_eq!(html.matches("<script ").count(), 2);
    assert!(!html.contains("old.js"));
    assert!(!html.contains("second.js"));
    assert!(html.find("new.js").unwrap() < html.find("other.js").unwrap());
    assert!(html.contains("style.css"));
}
#[test]
fn request_nonce_is_used_for_resources_without_leaking_between_requests() {
    for nonce in ["bm9uY2UtMQ==", "bm9uY2UtMg=="] {
        let (mut cx, doc, out) = setup("/", false);
        cx.set_csp_nonce(CspNonce::new(nonce).unwrap());
        let hook = doc.begin_hook().unwrap();
        cx.head_mut().module(Script::Site, "/site.js").unwrap();
        let guard = hook.finish(Ok(())).unwrap();
        emit(&mut cx, &doc).unwrap();
        guard.finish(Ok(())).unwrap();
        assert_eq!(wire(&out).matches("nonce=").count(), 1);
        assert!(wire(&out).contains(&format!("nonce=\"{nonce}\"")));
    }
    let (mut cx, doc, out) = setup("/", false);
    let hook = doc.begin_hook().unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    emit(&mut cx, &doc).unwrap();
    guard.finish(Ok(())).unwrap();
    assert!(wire(&out).is_empty());
}
#[test]
fn invalid_config_and_oversized_fields_are_rejected_before_mutation() {
    for origin in [
        "https://user:secret@public.example",
        "https://public.example/path",
        "https://public.example/?token=secret",
        "javascript:bad",
    ] {
        assert!(HeadUrlPolicy::new(origin, "/").is_err());
    }
    for mount in [
        "//evil",
        "/../outside",
        "/%2e%2e",
        "/a?token=secret",
        "/a\\b",
    ] {
        assert!(HeadUrlPolicy::new("https://public.example", mount).is_err());
    }
    let (mut cx, doc, out) = setup("/", false);
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().title("valid").unwrap();
    assert!(cx.head_mut().title("x".repeat(16 * 1024 + 1)).is_err());
    let guard = hook.finish(Ok(())).unwrap();
    emit(&mut cx, &doc).unwrap();
    guard.finish(Ok(())).unwrap();
    assert_eq!(wire(&out), "<title>valid</title>");
}
#[test]
fn escaped_size_limit_is_sticky_and_publishes_no_partial_head() {
    let (mut cx, doc, out) = setup("/", false);
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().title("&".repeat(16 * 1024)).unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    assert!(matches!(
        emit(&mut cx, &doc),
        Err(WriteError::HeadLifecycle(HeadError::RetainedLimit))
    ));
    assert!(out.lock().unwrap().is_empty());
    assert!(doc.final_check().is_err());
    assert!(guard.finish(Ok(())).is_err());
}
#[test]
fn unique_slot_limit_counts_removal_tombstones() {
    let (mut cx, doc, out) = setup("/", false);
    let hook = doc.begin_hook().unwrap();
    for key in 0u16..128 {
        cx.head_mut().remove_module(key).unwrap();
    }
    assert!(cx.head_mut().module(128u16, "/overflow.js").is_err());
    cx.head_mut().module(0u16, "/replacement.js").unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    emit(&mut cx, &doc).unwrap();
    guard.finish(Ok(())).unwrap();
    assert_eq!(wire(&out).matches("<script ").count(), 1);
}
#[test]
fn body_cannot_swap_nonce_after_head_values_have_been_sealed() {
    let (mut cx, doc, out) = setup("/", false);
    cx.set_csp_nonce(CspNonce::new("bm9uY2UtMQ==").unwrap());
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().module(Script::Site, "/site.js").unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    cx.managed_seal().unwrap();
    cx.set_csp_nonce(CspNonce::new("bm9uY2UtMg==").unwrap());
    assert!(matches!(
        WebRenderSync::render(doc.outlet(), &mut cx),
        Err(WriteError::HeadLifecycle(
            HeadError::NonceAfterInitialization
        ))
    ));
    assert!(out.lock().unwrap().is_empty());
    assert!(guard.finish(Ok(())).is_err());
}
#[test]
fn send_context_cannot_swap_the_response_nonce_after_sealing() {
    let (_, doc, out) = setup("/", false);
    let mut cx = SendWebContext::<NoLoader, RenderBuffer>::new();
    cx.head_mut().manage(doc.clone());
    cx.set_csp_nonce(CspNonce::new("bm9uY2UtMQ==").unwrap());
    let hook = doc.begin_hook().unwrap();
    cx.head_mut().module(Script::Site, "/site.js").unwrap();
    let guard = hook.finish(Ok(())).unwrap();
    cx.managed_seal().unwrap();
    cx.set_csp_nonce(CspNonce::new("bm9uY2UtMg==").unwrap());
    assert!(matches!(
        WebRenderSendSync::render_send_sync(doc.outlet(), &mut cx),
        Err(WriteError::HeadLifecycle(
            HeadError::NonceAfterInitialization
        ))
    ));
    assert!(out.lock().unwrap().is_empty());
    assert!(guard.finish(Ok(())).is_err());
}
