// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    render_in_send_context, render_to_string, render_to_string_async, SendWebContext, WriteBuf,
};
use proa_macros::{html, html_send, html_sync};

const CONTENT: &str = "Szkolenia & <examples> \"quoted\" 'single'";
const EXPECTED: &str = "<meta property=\"og:title\" content=\"Szkolenia &amp; &lt;examples&gt; &quot;quoted&quot; &#39;single&#39;\">";

#[test]
fn property_meta_escapes_dynamic_attributes() {
    let property = "og:title";
    let content = CONTENT;
    let output = render_to_string(html_sync! {
        <meta property={property} content={content} />
    });

    assert_eq!(output, EXPECTED);
}

#[tokio::test]
async fn async_property_meta_escapes_content() {
    let content = CONTENT;
    let output = render_to_string_async(html! {
        <meta property="og:title" content={content} />
    })
    .await;

    assert_eq!(output, EXPECTED);
}

#[tokio::test]
async fn send_property_meta_escapes_content() {
    let content = CONTENT;
    let mut cx = SendWebContext::new();
    render_in_send_context(
        &mut cx,
        html_send! {
            <meta property="og:title" content={content} />
        },
    )
    .await
    .unwrap();

    assert_eq!(cx.into_output().as_slice(), EXPECTED.as_bytes());
}

#[test]
fn property_meta_allows_repeated_properties_and_other_vocabularies() {
    let output = render_to_string(html_sync! {
        <meta property="og:image" content="/first.png" />
        <meta property="og:image" content="/second.png" />
        <meta property="fb:app_id" content="123" />
    });

    assert_eq!(
        output,
        concat!(
            r#"<meta property="og:image" content="/first.png">"#,
            r#"<meta property="og:image" content="/second.png">"#,
            r#"<meta property="fb:app_id" content="123">"#,
        )
    );
}

#[test]
fn existing_meta_forms_still_render() {
    let output = render_to_string(html_sync! {
        <meta charset="utf-8" />
        <meta name="description" content="" />
        <meta http-equiv="refresh" content="30" />
        <meta itemprop="description" content="item" />
        <meta name="og:title" property="og:title" content="title" />
    });

    assert_eq!(
        output,
        concat!(
            r#"<meta charset="utf-8">"#,
            r#"<meta name="description" content="">"#,
            r#"<meta http-equiv="refresh" content="30">"#,
            r#"<meta itemprop="description" content="item">"#,
            r#"<meta name="og:title" property="og:title" content="title">"#,
        )
    );
}
