// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Quote `input` as a JavaScript double-quoted string literal, escaping the
/// characters that could break out of the literal or out of an inline
/// `<script type="module">` element. The result is emitted via `UnsafeRaw`
/// in `bootstrap.rs`, so the script-context masking below is load-bearing:
///
/// * `<` is escaped to `\x3C`. `\x3C` is valid JavaScript and decodes back to
///   `<` for the JS engine, but it prevents the HTML tokenizer from ever
///   seeing the literal byte sequences `</script>` (which would close the
///   script early) or `<!--` (which starts an HTML-style comment that legacy
///   parsers honor inside scripts). Masking `<` alone neutralizes both.
/// * `U+2028` (LINE SEPARATOR) and `U+2029` (PARAGRAPH SEPARATOR) are escaped
///   to `\u2028` / `\u2029`. Prior to ES2019 these acted as line terminators
///   inside string literals on many JS parsers, allowing breakout from the
///   quoted string. The `\uXXXX` form decodes to the same code point.
///
/// This mirrors the canonical script-context masking in
/// `proa_react/src/island.rs` (see the "CVE review Gap M" comment block).
pub(crate) fn quote_js(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\x3C"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_js_escapes_control_characters() {
        assert_eq!(quote_js("line\nbreak"), r#""line\nbreak""#);
        assert_eq!(quote_js("quote\""), r#""quote\"""#);
        assert_eq!(quote_js(r#"back\slash"#), r#""back\\slash""#);
    }

    #[test]
    fn quote_js_masks_closing_script_tag() {
        // `</script>` must not survive verbatim or the inline <script> closes
        // early. The `<` is escaped to `\x3C`, so the byte sequence `</` (and
        // therefore `</script>`) never reaches the HTML tokenizer.
        let quoted = quote_js("</script><img src=x onerror=alert(1)>");
        assert!(
            !quoted.contains("</"),
            "unmasked `</` in quoted JS: {quoted}"
        );
        assert!(!quoted.contains("</script>"), "unmasked closing script tag");
        assert!(quoted.contains("\\x3C/script>"));
    }

    #[test]
    fn quote_js_masks_html_comment_open() {
        // `<!--` starts an HTML comment that legacy parsers honor inside a
        // <script>. Masking `<` to `\x3C` neutralizes it.
        let quoted = quote_js("before <!-- after");
        assert!(!quoted.contains("<!--"), "unmasked `<!--` in quoted JS");
        assert!(quoted.contains("\\x3C!-- after"));
    }

    #[test]
    fn quote_js_escapes_line_separators() {
        // U+2028 / U+2029 are legal inside a JS string post-ES2019 but break
        // out of the literal on legacy parsers. They must become \uXXXX.
        let quoted = quote_js("line\u{2028}para\u{2029}end");
        assert!(!quoted.contains('\u{2028}'), "raw U+2028 in quoted JS");
        assert!(!quoted.contains('\u{2029}'), "raw U+2029 in quoted JS");
        assert_eq!(quoted, "\"line\\u2028para\\u2029end\"");
    }
}
