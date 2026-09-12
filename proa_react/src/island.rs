// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::string::String;

use arrayvec::ArrayString;
use proa_core::{ctx::Ctx, text::UnsafeRaw, WebRenderSync, WriteError};
use proa_macros::html_sync;
use serde::Serialize;

/// A React island that implements `Render`.
pub struct ReactIsland<F> {
    island_name: String,
    stable_id: String,
    props_json: String,
    inner_html: F,
    error_config: Option<crate::builder::ErrorBoundaryConfig>,
}

impl<F> WebRenderSync for ReactIsland<F>
where
    F: FnOnce(&mut Ctx) -> Result<(), WriteError>,
{
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        render_island_impl(
            cx,
            &self.island_name,
            &self.stable_id,
            &self.props_json,
            self.inner_html,
            self.error_config.as_ref(),
            None,
            false,
        )
    }
}

/// Internal helper for rendering islands with error boundary support.
#[allow(clippy::too_many_arguments)]
pub(crate) fn react_island_render_with_error<F>(
    cx: &mut Ctx,
    island_name: &str,
    stable_id: &str,
    props_json: &str,
    inner_html: F,
    error_config: Option<&crate::builder::ErrorBoundaryConfig>,
    style: Option<&str>,
    permanent: bool,
) -> Result<(), WriteError>
where
    F: FnOnce(&mut Ctx) -> Result<(), WriteError>,
{
    render_island_impl(
        cx,
        island_name,
        stable_id,
        props_json,
        inner_html,
        error_config,
        style,
        permanent,
    )
}

/// Creates a React island that can be rendered later.
pub fn react_island<F>(
    island_name: impl Into<String>,
    stable_id: impl Into<String>,
    props_json: impl Into<String>,
    inner_html: F,
) -> ReactIsland<F>
where
    F: FnOnce(&mut Ctx) -> Result<(), WriteError>,
{
    ReactIsland {
        island_name: island_name.into(),
        stable_id: stable_id.into(),
        props_json: props_json.into(),
        inner_html,
        error_config: None,
    }
}

#[allow(clippy::too_many_arguments)]
// proa-perf-ok(scope): this low-level island emitter escapes every dynamic
// attribute, masks script-breaking JSON, and writes directly to avoid copies
fn render_island_impl<F>(
    cx: &mut Ctx,
    island_name: &str,
    stable_id: &str,
    props_json: &str,
    inner_html: F,
    error_config: Option<&crate::builder::ErrorBoundaryConfig>,
    style: Option<&str>,
    permanent: bool,
) -> Result<(), WriteError>
where
    F: FnOnce(&mut Ctx) -> Result<(), WriteError>,
{
    let mut props_id_inline: ArrayString<128> = ArrayString::new();
    let props_script_id: Cow<'_, str> = match props_id_inline.try_push_str(stable_id) {
        Ok(_) => {
            let _ = props_id_inline.try_push_str("__props");
            Cow::Borrowed(props_id_inline.as_str())
        }
        Err(_) => {
            let mut owned = String::with_capacity(stable_id.len() + "__props".len());
            owned.push_str(stable_id);
            owned.push_str("__props");
            Cow::Owned(owned)
        }
    };

    // Build opening div tag with dynamic attributes
    let mut tag_html = String::with_capacity(256);
    tag_html.push_str("<div id=\"");
    push_attr_escaped(&mut tag_html, stable_id);
    tag_html.push_str("\" data-island=\"");
    push_attr_escaped(&mut tag_html, island_name);
    tag_html.push_str("\" data-props-id=\"");
    push_attr_escaped(&mut tag_html, props_script_id.as_ref());
    tag_html.push('"');

    // Add data-turbo-permanent for islands that survive Turbo Drive navigations
    if permanent {
        tag_html.push_str(" data-turbo-permanent");
    }

    // Add inline style for size hints / layout shift prevention
    if let Some(s) = style {
        tag_html.push_str(" style=\"");
        push_attr_escaped(&mut tag_html, s);
        tag_html.push('"');
    }

    // Add error boundary attributes if configured
    if let Some(config) = error_config {
        tag_html.push_str(" data-error-retry-count=\"");
        tag_html.push_str(&config.retry_count.to_string());
        tag_html.push_str("\" data-error-retry-delay=\"");
        tag_html.push_str(&config.retry_delay_ms.to_string());
        tag_html.push_str("\" data-error-report=\"");
        tag_html.push_str(if config.report_errors {
            "true"
        } else {
            "false"
        });
        tag_html.push('"');

        if let Some(fallback) = &config.error_fallback {
            tag_html.push_str(" data-error-fallback=\"");
            push_attr_escaped(&mut tag_html, fallback);
            tag_html.push('"');
        }
    }

    tag_html.push('>');

    // Write opening tag
    proa_core::WriteBuf::extend(&mut cx.out, tag_html.as_bytes())?;

    // Render inner content
    inner_html(cx)?;

    // Close div tag
    proa_core::WriteBuf::extend(&mut cx.out, b"</div>")?;

    // `props_json` is already valid JSON from `serde_json::to_string`. Do NOT
    // HTML-escape it — `<script type="application/json">` content is raw text,
    // and `element.textContent` does not decode HTML entities, so entities
    // would corrupt the JSON. We mask three byte sequences that the HTML
    // tokenizer (or legacy JS parsers) would otherwise pick up:
    //
    // 1. `</` → `<\/` (valid JSON escape for `/`) prevents the HTML parser
    //    from matching `</script>` inside the JSON payload.
    // 2. `U+2028` (LINE SEPARATOR) → `\u2028` and
    // 3. `U+2029` (PARAGRAPH SEPARATOR) → `\u2029`. These two code points
    //    are legal JSON string contents but, prior to ES2019, acted as line
    //    terminators inside inline `<script>` blocks on many JS parsers,
    //    allowing breakout from a JSON string literal. Masking them to their
    //    6-char `\uXXXX` escapes is a no-op for `element.textContent` (the
    //    hydration payload decodes identically) and closes the legacy-parser
    //    breakout. See CVE review Gap M.
    let needs_mask = props_json.contains("</")
        || props_json.contains('\u{2028}')
        || props_json.contains('\u{2029}');
    let safe_props_json: Cow<'_, str> = if needs_mask {
        let mut out = String::with_capacity(props_json.len());
        for ch in props_json.chars() {
            match ch {
                '\u{2028}' => out.push_str("\\u2028"),
                '\u{2029}' => out.push_str("\\u2029"),
                _ => out.push(ch),
            }
        }
        // `</` masking runs after char-by-char transcode so it sees the final
        // byte layout. Both passes are idempotent and do not create new
        // hazards for each other.
        if out.contains("</") {
            out = out.replace("</", "<\\/");
        }
        Cow::Owned(out)
    } else {
        Cow::Borrowed(props_json)
    };
    let nonce = cx.csp_nonce_cloned();
    html_sync! {
        <script
            type="application/json"
            id={props_script_id.as_ref()}
            nonce={proa_core::csp_nonce_attr(nonce.as_ref())}
        >
            {UnsafeRaw(safe_props_json.as_ref())}
        </script>
    }
    .render(cx)
}

/// HTML-escape a string for safe embedding in an HTML attribute value.
fn push_attr_escaped(buf: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '"' => buf.push_str("&quot;"),
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            _ => buf.push(ch),
        }
    }
}

/// Convenience helper that serializes props with `serde_json` and returns a `ReactIsland`.
pub fn react_island_with_props<F, P>(
    island_name: impl Into<String>,
    stable_id: impl Into<String>,
    props: &P,
    inner_html: F,
) -> Result<ReactIsland<F>, serde_json::Error>
where
    F: FnOnce(&mut Ctx) -> Result<(), WriteError>,
    P: Serialize,
{
    let json = serde_json::to_string(props)?;
    Ok(react_island(island_name, stable_id, json, inner_html))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::text::text;
    use proa_core::WebRenderSync;

    #[test]
    fn island_renders_expected_markup() {
        let mut cx = Ctx::new();
        react_island("Counter", "counter-1", r#"{"count":1}"#, |cx| {
            text("Count: 1").render(cx)
        })
        .render(&mut cx)
        .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(html.contains(r#"data-island="Counter""#));
        assert!(html.contains(r#"<script type="application/json" id="counter-1__props">"#));
    }

    #[test]
    fn island_props_script_uses_request_nonce() {
        let nonce = proa_core::CspNonce::new("propsNonce123").unwrap();
        let mut cx = Ctx::new().with_csp_nonce(nonce);
        react_island("Counter", "counter-1", r#"{"count":1}"#, |cx| {
            text("Count: 1").render(cx)
        })
        .render(&mut cx)
        .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(
            html.contains(
                r#"<script type="application/json" id="counter-1__props" nonce="propsNonce123">"#
            ),
            "{html}"
        );
    }

    #[test]
    fn island_with_props_helper_serializes() {
        let mut cx = Ctx::new();
        #[derive(Serialize)]
        struct Props<'a> {
            name: &'a str,
        }

        react_island_with_props("Greeting", "greet-1", &Props { name: "Ada" }, |cx| {
            text("Hello").render(cx)
        })
        .unwrap()
        .render(&mut cx)
        .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(html.contains(r#"{"name":"Ada"}"#));
    }

    /// CVE review Gap M — `</` inside JSON prop values must be masked so the
    /// HTML parser cannot match `</script>` inside the hydration payload.
    #[test]
    fn island_json_bootstrap_masks_closing_script_tag() {
        let mut cx = Ctx::new();
        react_island(
            "Comment",
            "c-1",
            r#"{"body":"hello </script><img src=x onerror=alert(1)>"}"#,
            |cx| text("x").render(cx),
        )
        .render(&mut cx)
        .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        // Narrow to the JSON block inside the props script to avoid false
        // matches on the outer div's `data-props-id` attribute.
        let script_open = r#"<script type="application/json" id="c-1__props">"#;
        let start = html.find(script_open).unwrap() + script_open.len();
        let close = html[start..].find("</script>").unwrap();
        let payload = &html[start..start + close];
        assert!(
            !payload.contains("</"),
            "unmasked </ in JSON payload: {payload}"
        );
        assert!(payload.contains(r#"<\/script"#));
    }

    /// CVE review Gap M — U+2028 / U+2029 line separators inside JSON prop
    /// values must be escaped to their 6-char `\uXXXX` form so legacy JS
    /// parsers cannot treat them as line terminators inside inline scripts.
    #[test]
    fn island_json_bootstrap_masks_line_separators() {
        let mut cx = Ctx::new();
        // `serde_json` encodes U+2028 / U+2029 as literal bytes (NOT as
        // \u2028) in its default output, which is where the legacy-parser
        // risk comes from. Feed the function a hand-rolled JSON literal with
        // the raw code points to exercise the mask.
        let payload = "{\"note\":\"line\u{2028}break\u{2029}end\"}";
        react_island("N", "n-1", payload, |cx| text("x").render(cx))
            .render(&mut cx)
            .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        let script_open = r#"<script type="application/json" id="n-1__props">"#;
        let start = html.find(script_open).unwrap() + script_open.len();
        let close = html[start..].find("</script>").unwrap();
        let json_block = &html[start..start + close];
        assert!(!json_block.contains('\u{2028}'), "raw U+2028 in payload");
        assert!(!json_block.contains('\u{2029}'), "raw U+2029 in payload");
        assert!(json_block.contains(r"\u2028"));
        assert!(json_block.contains(r"\u2029"));
    }

    /// Masking must be idempotent-friendly: when a payload needs BOTH
    /// `</` masking and line-separator escape, the output contains neither
    /// hazard and deserializes back to the original via a JSON parser.
    #[test]
    fn island_json_bootstrap_masks_combined() {
        let mut cx = Ctx::new();
        let payload = "{\"html\":\"</script>\",\"ls\":\"\u{2028}\"}";
        react_island("C", "c-2", payload, |cx| text("x").render(cx))
            .render(&mut cx)
            .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        let script_open = r#"<script type="application/json" id="c-2__props">"#;
        let start = html.find(script_open).unwrap() + script_open.len();
        let tail = &html[start..];
        let close = tail.find("</script>").unwrap();
        let payload = &tail[..close];
        assert!(!payload.contains("</"), "unmasked </ in payload: {payload}");
        assert!(!payload.contains('\u{2028}'), "raw U+2028 in payload");
        assert!(payload.contains(r"<\/script"));
        assert!(payload.contains(r"\u2028"));
    }
}
