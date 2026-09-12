// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the dynamic URL-attribute scheme guard.
//!
//! A static `href="javascript:..."` is a compile error (see the trybuild UI
//! tests). These tests cover the runtime half: a *dynamic* URL value such as
//! `href={user_input}` must be blocked too, routed through `render_url_attr`
//! by the `html_sync!` / `html!` codegen. In debug builds the guard trips a
//! `debug_assert!`; in release it neutralizes the attribute to an empty value.

use proa_core::{Ctx, WebRenderSync};
use proa_macros::{html_sync, text};

fn render(f: impl FnOnce(&mut Ctx)) -> String {
    let mut cx = Ctx::new();
    f(&mut cx);
    String::from_utf8(cx.into_output().into_vec()).unwrap()
}

#[test]
fn dynamic_safe_href_renders() {
    let url = "https://example.com/profile";
    let html = render(|cx| {
        html_sync! {
            <a href={url}>"link"</a>
        }
        .render(cx)
        .unwrap();
    });
    assert!(
        html.contains(r#"href="https://example.com/profile""#),
        "{html}"
    );
}

#[test]
fn dynamic_safe_src_data_image_renders() {
    // Non-executable data: URIs are legitimate inline images.
    let url = "data:image/png;base64,iVBORw0KGgo=";
    let html = render(|cx| {
        html_sync! {
            <img src={url} alt="" />
        }
        .render(cx)
        .unwrap();
    });
    assert!(html.contains("data:image/png;base64"), "{html}");
}

#[test]
fn static_safe_src_data_image_renders() {
    let html = render(|cx| {
        html_sync! {
            <img src="data:image/png;base64,iVBORw0KGgo=" alt="" />
        }
        .render(cx)
        .unwrap();
    });
    assert!(html.contains("data:image/png;base64"), "{html}");
}

// A non-URL attribute must NOT be scheme-checked: a `title` that happens to
// contain the text "javascript:" is harmless and must render (escaped), not
// panic. This proves the guard is scoped to URL-bearing attribute names.
#[test]
fn dynamic_non_url_attribute_not_guarded() {
    let value = "javascript:not-a-url-here";
    let html = render(|cx| {
        html_sync! {
            <span title={value}>"x"</span>
        }
        .render(cx)
        .unwrap();
    });
    assert!(
        html.contains(r#"title="javascript:not-a-url-here""#),
        "{html}"
    );
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn dynamic_javascript_href_panics_in_debug() {
    let evil = "javascript:alert(document.cookie)";
    let _ = render(|cx| {
        html_sync! {
            <a href={evil}>"x"</a>
        }
        .render(cx)
        .unwrap();
    });
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn dynamic_javascript_src_panics_in_debug() {
    let evil = "javascript:alert(1)";
    let _ = render(|cx| {
        html_sync! {
            <img src={evil} alt="" />
        }
        .render(cx)
        .unwrap();
    });
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn stored_text_composite_url_panics_in_debug() {
    let value = text!("{}{}", "java", "script:alert(1)");
    let _ = render(|cx| {
        html_sync! {
            <a href={value}>"x"</a>
        }
        .render(cx)
        .unwrap();
    });
}

#[cfg(not(debug_assertions))]
#[test]
fn dynamic_dangerous_url_neutralized_in_release() {
    let evil = "javascript:alert(document.cookie)";
    let html = render(|cx| {
        html_sync! {
            <a href={evil}>"x"</a>
        }
        .render(cx)
        .unwrap();
    });
    assert!(!html.contains("javascript:"), "{html}");
    assert!(html.contains(r#"href="""#), "{html}");
}

#[cfg(not(debug_assertions))]
#[test]
fn stored_text_composite_url_is_neutralized_in_release() {
    let value = text!("{}{}", "java", "script:alert(1)");
    let html = render(|cx| {
        html_sync! {
            <a href={value}>"x"</a>
        }
        .render(cx)
        .unwrap();
    });

    assert_eq!(html, r#"<a href="">x</a>"#);
}
