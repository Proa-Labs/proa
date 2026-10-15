// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared allocation-free HTML URL policy for template compilers and renderers.
//! Kept independent of rendering traits so compiler crates do not rebuild when
//! the runtime renderer changes.

#![no_std]
#![forbid(unsafe_code)]

/// Check whether a URL value starts with a dangerous scheme that could lead to
/// script execution (XSS).
///
/// Returns `true` for `javascript:` and `vbscript:` schemes (case-insensitive),
/// plus executable `data:` MIME types. These values are never legitimate in
/// ordinary server-rendered URL attributes.
///
/// ## Why this exists
///
/// The `javascript:` pseudo-scheme (RFC 4266 informational, but implemented by
/// all browsers) allows arbitrary script execution when navigated to. An
/// attacker who controls an `href` value can inject `javascript:alert(...)` to
/// achieve Cross-Site Scripting (XSS). `vbscript:` is the legacy IE equivalent.
///
/// OWASP XSS Prevention Cheat Sheet, Rule #7 — "Avoid JavaScript URLs":
/// <https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html>
///
/// React adopted the same block in v16.9 (facebook/react#16382).
///
/// ## Normalization
///
/// The URL spec (WHATWG §4.3) directs browsers to strip ASCII tab, LF, and CR
/// from URL inputs before parsing. An attacker can exploit this by embedding
/// control characters inside the scheme prefix — e.g. `java\tscript:alert(1)`
/// or `\njavascript:alert(1)` — to defeat naive prefix matching. We therefore
/// skip ASCII whitespace (space, tab, LF, CR) AND NUL throughout the scheme
/// prefix during comparison, not just at the leading edge.
///
/// The HTML5 parser ALSO decodes numeric character references
/// (`&#NNN;` and `&#xHH;`) inside attribute values before URL parsing. An
/// attacker can therefore smuggle `&#106;avascript:alert(1)` or
/// `&#0000000106;avascript:alert(1)` past a naive string-prefix check. This
/// is exactly how CVE-2026-39315 bypassed the Unhead `hasDangerousProtocol()`
/// function. We perform one pass of numeric-entity decoding with no digit
/// cap and treat a non-ASCII decoded code point as a non-match (it cannot
/// spell an ASCII scheme or MIME type). The same normalization is applied to
/// the `data:` scheme, its MIME type, and the MIME boundary.
///
/// References:
/// - WHATWG URL §4.3 — ASCII tab/newline stripping
/// - HTML5 §13.2.2.5 — attribute value processing
/// - HTML5 §13.2.5.72 — numeric character reference decoding
/// - CVE-2026-39315 — Unhead HTML-entity padded scheme bypass
/// - OWASP XSS Filter Evasion Cheat Sheet — control-char bypass techniques
///
/// Marked `#[inline]` so the release build can fold the function into
/// `elem::write_attr`, which calls it on every URL-bearing attribute in
/// the hot render path.
#[inline]
pub fn has_dangerous_url_scheme(url: &str) -> bool {
    let bytes = url.as_bytes();
    // Cheap early-out: the vast majority of URL values start with `h`
    // (http/https), `/`, `#`, `.`, `?`, `m` (mailto), `t` (tel), etc. The
    // slow path only needs to run when the first non-control byte could
    // plausibly be the start of `javascript:`, `vbscript:`, or
    // `data:text/html` — i.e. the ASCII letters j/J, v/V, d/D, OR an
    // HTML entity reference `&#`. This skips the full scheme walk for
    // ~99% of real-world attributes without changing the guarantee for
    // adversarial inputs.
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b <= 0x20 {
            i += 1;
            continue;
        }
        if !matches!(b, b'j' | b'J' | b'v' | b'V' | b'd' | b'D' | b'&') {
            return false;
        }
        break;
    }
    prefix_matches_ignoring_controls(bytes, b"javascript:")
        || prefix_matches_ignoring_controls(bytes, b"vbscript:")
        || is_dangerous_data_url(bytes)
}

/// Detect `data:` URLs whose MIME type is HTML-executable. These can
/// host phishing pages on top-level navigation and execute scripts in
/// older browsers / specific contexts (`<iframe srcdoc>`-adjacent
/// renderings). Raster image / audio / video / font data URLs are not blocked
/// because they have legitimate inline-content uses. SVG is blocked as an
/// executable XML container even when the particular payload looks inert.
///
/// Specifically blocks:
///   - `data:text/html` (and variants with charset / base64 params)
///   - `data:application/xhtml+xml`
///   - `data:application/xml` (rare but XML can host scripts via
///     SVG / XSL)
///   - `data:image/svg+xml`
///
/// The prior allowlist only blocked `javascript:` and `vbscript:`, leaving
/// `data:text/html,
/// <script>...</script>` as a viable XSS path.
fn is_dangerous_data_url(bytes: &[u8]) -> bool {
    let mut cursor = NormalizedAsciiCursor::new(bytes);
    if !cursor.consume_ascii_case_insensitive(b"data:") {
        return false;
    }

    let dangerous_mimes: &[&[u8]] = &[
        b"text/html",
        b"application/xhtml+xml",
        b"application/xhtml",
        b"application/xml",
        b"image/svg+xml", // svg can carry <script>; ban inline for v1
    ];
    for mime in dangerous_mimes {
        let mut candidate = cursor;
        let matches_mime = candidate.consume_ascii_case_insensitive(mime);
        let boundary = candidate.next();
        if matches_mime
            && (boundary.is_none()
                || boundary == Some(u32::from(b';'))
                || boundary == Some(u32::from(b',')))
        {
            return true;
        }
    }
    false
}

/// Allocation-free cursor over the browser-relevant normalized ASCII stream.
///
/// Raw and numeric-entity-decoded C0 controls are skipped. Numeric entities
/// are decoded once, including the HTML-permitted missing semicolon form. A
/// decoded non-ASCII scalar is returned as-is so an ASCII policy match fails
/// closed. Keeping the source index in the cursor lets `data:` MIME probes fork
/// after the scheme without reparsing it or maintaining a second, divergent
/// normalization loop.
#[derive(Clone, Copy)]
struct NormalizedAsciiCursor<'a> {
    input: &'a [u8],
    index: usize,
}

impl<'a> NormalizedAsciiCursor<'a> {
    #[inline(always)]
    const fn new(input: &'a [u8]) -> Self {
        Self { input, index: 0 }
    }

    #[inline(always)]
    fn next(&mut self) -> Option<u32> {
        while self.index < self.input.len() {
            let b = self.input[self.index];
            if b <= 0x20 {
                self.index += 1;
                continue;
            }

            if b == b'&' && self.input.get(self.index + 1) == Some(&b'#') {
                if let Some((code_point, consumed)) =
                    decode_numeric_entity(&self.input[self.index..])
                {
                    self.index += consumed;
                    if code_point <= 0x20 {
                        continue;
                    }
                    return Some(code_point);
                }
            }

            self.index += 1;
            return Some(u32::from(b));
        }
        None
    }

    #[inline(always)]
    fn consume_ascii_case_insensitive(&mut self, expected: &[u8]) -> bool {
        expected.iter().all(|expected| {
            self.next().is_some_and(|actual| {
                actual <= u32::from(u8::MAX) && (actual as u8).eq_ignore_ascii_case(expected)
            })
        })
    }
}

/// Case-insensitive prefix match that skips ASCII control characters (space,
/// tab, LF, CR, NUL) both at the leading edge and interleaved within the
/// scheme prefix, AND decodes numeric HTML entities (`&#NNN;`, `&#xHH;`)
/// before comparison. Matches the browser's URL-parse behaviour, which
/// strips these bytes and decodes these entities before scheme recognition.
#[inline(always)]
fn prefix_matches_ignoring_controls(input: &[u8], expected: &[u8]) -> bool {
    NormalizedAsciiCursor::new(input).consume_ascii_case_insensitive(expected)
}

/// Decode a numeric HTML entity from the start of `input`. Input must begin
/// with `&#`. Returns `(code_point, bytes_consumed)` on success.
///
/// HTML5 accepts both `&#NNN;` (decimal) and `&#xHH;` / `&#XHH;` (hex) with
/// an arbitrary number of digits and an optional trailing semicolon. Per
/// HTML5 §13.2.5.80 (numeric character reference end state):
///
/// - Value `0x00` becomes `U+FFFD` (null-character-reference parse error).
/// - Value `> 0x10FFFF` becomes `U+FFFD`.
/// - Surrogates (`0xD800..=0xDFFF`) become `U+FFFD`.
/// - Non-whitespace C0 controls (`0x01..=0x08`, `0x0B`, `0x0E..=0x1F`) are
///   kept as-is (parse error only).
/// - ASCII whitespace controls (`0x09`, `0x0A`, `0x0D`, `0x20`) are kept
///   as-is — these are later stripped by the URL parser (WHATWG URL §4.3)
///   which is why the caller must re-check for `<= 0x20` after decoding.
/// - C1 range `0x80..=0x9F` would normally be remapped via a Windows-1252
///   table, but we do not apply that remap: all values in this range are
///   `> 0x7F` and the caller's non-ASCII gate rejects them fail-closed,
///   matching real-browser behavior because no Windows-1252 remap target
///   is an ASCII letter anyway.
///
/// Saturating arithmetic prevents adversarial padding from overflowing
/// `u32` into a collision on a small ASCII target.
#[inline]
fn decode_numeric_entity(input: &[u8]) -> Option<(u32, usize)> {
    // Caller guarantees input[0..2] == b"&#".
    if input.len() < 3 {
        return None;
    }
    let (radix, start) = if input[2] == b'x' || input[2] == b'X' {
        (16u32, 3usize)
    } else {
        (10u32, 2usize)
    };
    let mut value: u32 = 0;
    let mut i = start;
    let mut digit_count = 0usize;
    while i < input.len() {
        let b = input[i];
        let digit = match (radix, b) {
            (10, b'0'..=b'9') => (b - b'0') as u32,
            (16, b'0'..=b'9') => (b - b'0') as u32,
            (16, b'a'..=b'f') => (b - b'a' + 10) as u32,
            (16, b'A'..=b'F') => (b - b'A' + 10) as u32,
            _ => break,
        };
        value = value.saturating_mul(radix).saturating_add(digit);
        if value > 0x10FFFF {
            value = 0xFFFD;
        }
        i += 1;
        digit_count += 1;
    }
    if digit_count == 0 {
        return None;
    }
    // Trailing `;` is optional per HTML5 (a parse error but still decoded).
    if i < input.len() && input[i] == b';' {
        i += 1;
    }
    // HTML5 §13.2.5.80 replacements.
    if value == 0 || (0xD800..=0xDFFF).contains(&value) {
        value = 0xFFFD;
    }
    Some((value, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_url_scheme_detection() {
        // javascript: variants
        assert!(has_dangerous_url_scheme("javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("JavaScript:alert(1)"));
        assert!(has_dangerous_url_scheme("JAVASCRIPT:alert(1)"));
        assert!(has_dangerous_url_scheme("jAvAsCrIpT:void(0)"));

        // vbscript: variants
        assert!(has_dangerous_url_scheme("vbscript:MsgBox"));
        assert!(has_dangerous_url_scheme("VBScript:foo"));

        // leading whitespace bypass attempts
        assert!(has_dangerous_url_scheme("  javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("\tjavascript:alert(1)"));
        assert!(has_dangerous_url_scheme("\njavascript:alert(1)"));
        assert!(has_dangerous_url_scheme(" \t\n\rjavascript:alert(1)"));

        // Embedded control-character bypass attempts (browsers strip these
        // per WHATWG URL §4.3 before scheme parsing).
        assert!(has_dangerous_url_scheme("java\tscript:alert(1)"));
        assert!(has_dangerous_url_scheme("java\nscript:alert(1)"));
        assert!(has_dangerous_url_scheme("java\rscript:alert(1)"));
        assert!(has_dangerous_url_scheme("j a v a s c r i p t:alert(1)"));
        assert!(has_dangerous_url_scheme("\0javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("java\0script:alert(1)"));
        assert!(has_dangerous_url_scheme("vb\tscript:MsgBox"));

        // HTML numeric entity padding — CVE-2026-39315 class. HTML5 decodes
        // numeric entities with no digit cap before URL parsing.
        assert!(has_dangerous_url_scheme("&#106;avascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#0000000106;avascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x6a;avascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#X6A;avascript:alert(1)"));
        // Missing trailing semicolon — still decoded per HTML5 §13.2.5.72.
        assert!(has_dangerous_url_scheme("&#106avascript:alert(1)"));
        // Every scheme char encoded.
        assert!(has_dangerous_url_scheme(
            "&#106;&#97;&#118;&#97;&#115;&#99;&#114;&#105;&#112;&#116;:alert(1)"
        ));
        // Mixed entities + control chars.
        assert!(has_dangerous_url_scheme("  &#106;\tavascript:\n\talert(1)"));
        // Overflow-pad attempt — saturating arithmetic clamps to U+FFFD
        // which is non-ASCII, so the check fails-closed on bogus padding.
        assert!(!has_dangerous_url_scheme(
            "&#99999999999999999999999999;avascript:alert(1)"
        ));
        // Non-ASCII code point cannot match ASCII expected char.
        assert!(!has_dangerous_url_scheme("&#x1F600;avascript:alert(1)"));
        // Malformed entity (no digits) — treated as a literal `&`.
        assert!(!has_dangerous_url_scheme("&#;javascript:alert(1)"));
        // `&amp;#106;avascript:` — because `&` comes first and doesn't match
        // `j`, this fails. Confirms that escaping `&` to `&amp;` in attribute
        // values is still the primary defense.
        assert!(!has_dangerous_url_scheme("&amp;#106;avascript:alert(1)"));

        // Entity-decoded ASCII controls (tab/LF/CR/space/NUL) — browsers
        // strip these AFTER entity decoding per WHATWG URL §4.3. Found in
        // code review; must be caught identically to raw control bytes.
        assert!(has_dangerous_url_scheme("&#9;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#10;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#13;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#013;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#32;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x9;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x0A;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("&#x0D;javascript:alert(1)"));
        assert!(has_dangerous_url_scheme("java&#9;script:alert(1)"));
        assert!(has_dangerous_url_scheme("java&#10;script:alert(1)"));
        assert!(has_dangerous_url_scheme("java&#13;script:alert(1)"));
        assert!(has_dangerous_url_scheme("&#106;&#9;&#97;vascript:alert(1)"));
        assert!(has_dangerous_url_scheme("vb&#9;script:MsgBox"));
        // Raw control + entity combo.
        assert!(has_dangerous_url_scheme("\t&#106;avascript:alert(1)"));
        // Decoded NUL — HTML5 replaces &#0; with U+FFFD which is non-ASCII,
        // so the check fails-closed. Browser behavior matches (URL starts
        // with replacement char, not `javascript:`).
        assert!(!has_dangerous_url_scheme("&#0;javascript:alert(1)"));

        // safe URLs
        assert!(!has_dangerous_url_scheme("https://example.com"));
        assert!(!has_dangerous_url_scheme("http://example.com"));
        assert!(!has_dangerous_url_scheme("/path/to/page"));
        assert!(!has_dangerous_url_scheme("#anchor"));
        assert!(!has_dangerous_url_scheme("mailto:user@example.com"));
        assert!(!has_dangerous_url_scheme("tel:+1234567890"));
        assert!(!has_dangerous_url_scheme(""));
        assert!(!has_dangerous_url_scheme("   "));

        // Substrings that should NOT match
        assert!(!has_dangerous_url_scheme("not-javascript:foo"));
        assert!(!has_dangerous_url_scheme(
            "https://example.com/javascript:foo"
        ));
    }

    /// Cover executable `data:` MIME types in the dangerous-scheme allowlist.
    /// Image / audio / video /
    /// font data URLs remain accepted (legitimate inline-content use).
    #[test]
    fn dangerous_data_url_detection() {
        const BLOCK: &[&str] = &[
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java\tscript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "data:text/html;base64,PHNjcmlwdD4=",
            "data:text/html;charset=utf-8,<b>x</b>",
            "data:application/xhtml+xml,<html/>",
            "data:application/xhtml,<html/>",
            "data:application/xml,<root/>",
            "data:image/svg+xml,<svg/>",
            " Data:\tText/HTML;base64,WA==",
            "d\0a\tt\ra:\ntext/html,x",
            "&#100;ata:text/html,x",
            "&#100ata:text/html,x",
            "data&#58;text/html,x",
            "d&#9;ata:text/html,x",
            "data:&#116;ext/html,x",
            "data:&#9;text/html,x",
            "data:t&#9;ext/html,x",
        ];
        const ALLOW: &[&str] = &[
            "https://example.com/x",
            "/relative",
            "#anchor",
            "mailto:a@b.c",
            "tel:+123",
            "data:text/plain,hello",
            "data:image/png;base64,iVBORw0K",
            "data:image/jpeg;base64,/9j/",
            "data:image/webp;base64,UklGR",
            "data:image/gif;base64,R0lGOD",
            "data:audio/mp3;base64,SUQzAw",
            "data:video/mp4;base64,AAAAGGZ",
            "data:font/woff;base64,d09GMg",
            "data:,",
            "data:base64,",
            "data:text/htmlx,x",
            "data:application/xml-dtd,x",
            "data:image/svg+xmlx,x",
            "&amp;#100;ata:text/html,x",
        ];

        for url in BLOCK {
            assert!(
                has_dangerous_url_scheme(url),
                "dangerous URL was allowed: {url:?}"
            );
        }
        for url in ALLOW {
            assert!(
                !has_dangerous_url_scheme(url),
                "safe URL was blocked: {url:?}"
            );
        }
    }
}
