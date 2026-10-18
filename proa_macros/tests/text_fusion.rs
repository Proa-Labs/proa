// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    render_to_string, render_to_string_async, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html, html_sync};

mod foreign_macros {
    macro_rules! text {
        ($($tokens:tt)*) => {
            "<foreign text>"
        };
    }

    pub(crate) use text;
}

#[derive(Default)]
struct CountingBuf {
    bytes: Vec<u8>,
    static_writes: usize,
    dynamic_writes: usize,
}

impl WriteBuf for CountingBuf {
    fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn reserve(&mut self, additional: usize) -> Result<(), WriteError> {
        self.bytes.reserve(additional);
        Ok(())
    }

    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.dynamic_writes += 1;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        self.static_writes += 1;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    fn to_vec(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    fn clear(&mut self) {
        self.bytes.clear();
    }
}

#[test]
fn pure_literal_inline_text_folds_into_one_static_html_write() {
    let mut out = CountingBuf::default();
    let mut cx = WebContext::with_buffer(&mut out);

    // `text` is intentionally not imported: the outer macro recognizes and
    // consumes this direct invocation as an intrinsic.
    html_sync! {
        <p class="lead">{text!("A < B & {{ready}}")}</p>
    }
    .render(&mut cx)
    .unwrap();

    assert_eq!(out.bytes, b"<p class=\"lead\">A &lt; B &amp; {ready}</p>");
    assert_eq!(out.static_writes, 1);
    assert_eq!(out.dynamic_writes, 0);
}

#[test]
fn static_property_meta_folds_into_one_escaped_static_write() {
    let mut out = CountingBuf::default();
    let mut cx = WebContext::with_buffer(&mut out);

    html_sync! {
        <meta property="og:title" content="Szkolenia & <examples> \"quoted\"" />
    }
    .render(&mut cx)
    .unwrap();

    assert_eq!(
        out.bytes,
        b"<meta property=\"og:title\" content=\"Szkolenia &amp; &lt;examples&gt; &quot;quoted&quot;\">"
    );
    assert_eq!(out.static_writes, 1);
    assert_eq!(out.dynamic_writes, 0);
}

#[test]
fn placeholder_free_literal_url_is_validated_and_folds_static() {
    let mut out = CountingBuf::default();
    let mut cx = WebContext::with_buffer(&mut out);

    html_sync! {
        <a href={::proa_macros::text!("/docs?q=rust&mode=fast")}>"Docs"</a>
    }
    .render(&mut cx)
    .unwrap();

    assert_eq!(
        out.bytes,
        b"<a href=\"/docs?q=rust&amp;mode=fast\">Docs</a>"
    );
    assert_eq!(out.static_writes, 1);
    assert_eq!(out.dynamic_writes, 0);
}

#[test]
fn foreign_namespaced_text_macro_remains_an_ordinary_macro() {
    let html = render_to_string(html_sync! {
        <p>{foreign_macros::text!("ignored {}", "value")}</p>
    });

    assert_eq!(html, "<p>&lt;foreign text&gt;</p>");
}

#[test]
fn inline_text_streams_node_and_non_url_attribute_placeholders() {
    let label = "<label&>";
    let class = Some("active & ready");
    let html = render_to_string(html_sync! {
        <p class={((text!("base {}", class)))}>{text!("Hello {}: {:.2}", label, 12.345_f64)}</p>
    });

    assert_eq!(
        html,
        "<p class=\"base active &amp; ready\">Hello &lt;label&amp;&gt;: 12.35</p>"
    );
}

#[tokio::test]
async fn async_html_recognizes_the_canonical_qualified_text_path() {
    let label = "<async>";
    let html = render_to_string_async(html! {
        <p>{proa_macros::text!("Hello {}", label)}</p>
    })
    .await;

    assert_eq!(html, "<p>Hello &lt;async&gt;</p>");
}
