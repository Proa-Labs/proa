// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    render_to_string, ContentMode, NoLoader, WebContext, WebRender, WebRenderSync, WriteError,
};
use proa_macros::{xml, xml_sync};

#[test]
fn xml_sync_static_uses_xml_escaping() {
    let output = render_to_string(xml_sync! {
        <Feed enabled>
            <Item name="a\u{00A0}b & <c>">"x\u{00A0}<&>"</Item>
            <br />
        </Feed>
    });

    assert_eq!(
        output,
        "<Feed enabled=\"\"><Item name=\"a\u{00A0}b &amp; &lt;c&gt;\">x\u{00A0}&lt;&amp;&gt;</Item><br/></Feed>"
    );
}

#[test]
fn xml_sync_dynamic_text_and_attrs_use_xml_policy() {
    let label = "A\u{00A0}&<";
    let attr = "x\u{00A0}\"&";

    let output = render_to_string(xml_sync! {
        <Feed value={attr}>{label}</Feed>
    });

    assert_eq!(
        output,
        "<Feed value=\"x\u{00A0}&quot;&amp;\">A\u{00A0}&amp;&lt;</Feed>"
    );
}

#[test]
fn xml_sync_supports_namespace_colon_names() {
    let href = "#heart";

    let output = render_to_string(xml_sync! {
        <svg:path xlink:href={href} xml:lang="en" />
    });

    assert_eq!(output, "<svg:path xlink:href=\"#heart\" xml:lang=\"en\"/>");
}

#[test]
fn xml_sync_preserves_underscores_in_namespaced_xml_attrs() {
    let output = render_to_string(xml_sync! {
        <atom:entry data_value="kept"></atom:entry>
    });

    assert_eq!(output, "<atom:entry data_value=\"kept\"></atom:entry>");
}

#[test]
fn xml_sync_does_not_infer_html_void_elements() {
    let output = render_to_string(xml_sync! {
        <path>"body"</path>
    });

    assert_eq!(output, "<path>body</path>");
}

#[test]
fn xml_sync_restores_previous_escape_dialect() {
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    let prev = cx.__proa_enter_escape_dialect(proa_core::shared::escape::EscapeDialect::Xml);

    WebRenderSync::render(
        xml_sync! {
            <Outer>{"A\u{00A0}"}</Outer>
        },
        &mut cx,
    )
    .unwrap();

    let dialect = cx.__proa_current_escape_dialect();
    proa_core::shared::escape::escape_text_for_dialect(
        cx.out,
        "\u{00A0}",
        ContentMode::Html,
        dialect,
    )
    .unwrap();
    cx.__proa_restore_escape_dialect(prev);

    assert_eq!(
        String::from_utf8(out).unwrap(),
        "<Outer>A\u{00A0}</Outer>\u{00A0}"
    );
}

#[test]
fn xml_sync_dynamic_null_rejects() {
    let bad = "a\0b";
    let node = xml_sync! {
        <Feed>{bad}</Feed>
    };
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);

    let err = WebRenderSync::render(node, &mut cx).unwrap_err();
    assert_eq!(err, WriteError::InvalidCharForDialect);
}

fn assert_async_renderable<R: WebRender<NoLoader>>(_renderable: R) {}

#[test]
fn xml_async_macro_is_renderable() {
    let label = "dynamic";

    assert_async_renderable(xml! {
        <Feed>"Static "{label}</Feed>
    });
}
