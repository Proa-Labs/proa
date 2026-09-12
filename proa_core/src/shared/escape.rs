// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::mode::ContentMode;
use crate::shared::write_buf::{WriteBuf, WriteError};
use memchr::{memchr, memchr2, memchr3};

pub mod policy;

pub use policy::{EscapeDialect, EscapePolicy, HtmlPolicy, NullBytePolicy, XmlPolicy};

mod sealed {
    pub trait Sealed {}
}

#[cfg(debug_assertions)]
trait EscapeSink {
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError>;
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError>;
}

#[cfg(debug_assertions)]
struct WriteBufEscapeSink<'a, B: WriteBuf>(&'a mut B);

#[cfg(debug_assertions)]
impl<B: WriteBuf> EscapeSink for WriteBufEscapeSink<'_, B> {
    #[inline]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.0.extend(bytes)
    }

    #[inline]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        self.0.extend_static(bytes)
    }
}

/// Write a byte segment to `buf`, applying the active dialect policy for NBSP
/// and NULL bytes.
#[inline(always)]
fn emit_with_policy<S, P>(buf: &mut S, segment: &[u8]) -> Result<(), WriteError>
where
    S: ?Sized + EscapeSinkLike,
    P: EscapePolicy,
{
    let mut start = 0;
    while start < segment.len() {
        let needle_hit = if P::ESCAPE_NBSP {
            memchr2(0x00, 0xC2, &segment[start..])
        } else {
            memchr(0x00, &segment[start..])
        };

        let Some(pos) = needle_hit else {
            buf.extend(&segment[start..])?;
            break;
        };

        let abs = start + pos;
        if segment[abs] == 0x00 {
            if abs > start {
                buf.extend(&segment[start..abs])?;
            }
            match P::NULL_BYTE {
                NullBytePolicy::ReplaceWithReplacementChar => {
                    buf.extend_static("\u{FFFD}".as_bytes())?;
                }
                NullBytePolicy::Reject => {
                    return Err(WriteError::InvalidCharForDialect);
                }
            }
            start = abs + 1;
        } else {
            debug_assert!(P::ESCAPE_NBSP);
            if abs + 1 < segment.len() && segment[abs + 1] == 0xA0 {
                if abs > start {
                    buf.extend(&segment[start..abs])?;
                }
                buf.extend_static(b"&nbsp;")?;
                start = abs + 2;
            } else {
                buf.extend(&segment[start..abs + 1])?;
                start = abs + 1;
            }
        }
    }
    Ok(())
}

trait EscapeSinkLike {
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError>;
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError>;
}

impl<B: WriteBuf> EscapeSinkLike for B {
    #[inline(always)]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        WriteBuf::extend(self, bytes)
    }

    #[inline(always)]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        WriteBuf::extend_static(self, bytes)
    }
}

#[cfg(debug_assertions)]
impl EscapeSinkLike for dyn EscapeSink + '_ {
    #[inline]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        EscapeSink::extend(self, bytes)
    }

    #[inline]
    fn extend_static(&mut self, bytes: &'static [u8]) -> Result<(), WriteError> {
        EscapeSink::extend_static(self, bytes)
    }
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text<B: WriteBuf>(buf: &mut B, s: &str, mode: ContentMode) -> Result<(), WriteError> {
    escape_text_in::<HtmlPolicy, B>(buf, s, mode)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text_for_dialect<B: WriteBuf>(
    buf: &mut B,
    s: &str,
    mode: ContentMode,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    match dialect {
        EscapeDialect::Html => escape_text_in::<HtmlPolicy, B>(buf, s, mode),
        EscapeDialect::Xml => escape_text_in::<XmlPolicy, B>(buf, s, mode),
    }
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
    mode: ContentMode,
) -> Result<(), WriteError> {
    match mode {
        ContentMode::Html => escape_text_normal_in::<P, B>(buf, s),
        ContentMode::Script | ContentMode::Style => {
            if !P::HAS_RAW_TEXT_ELEMENTS {
                return Err(WriteError::RawTextUnsupportedForDialect);
            }
            match mode {
                ContentMode::Script => escape_text_with_sentinel_in::<P, B>(buf, s, b"script"),
                ContentMode::Style => escape_text_with_sentinel_in::<P, B>(buf, s, b"style"),
                ContentMode::Html => unreachable!(),
            }
        }
    }
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_text_xml<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_text_normal_in::<XmlPolicy, B>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_attr_value_in::<HtmlPolicy, B>(buf, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value_for_dialect<B: WriteBuf>(
    buf: &mut B,
    s: &str,
    dialect: EscapeDialect,
) -> Result<(), WriteError> {
    match dialect {
        EscapeDialect::Html => escape_attr_value_in::<HtmlPolicy, B>(buf, s),
        EscapeDialect::Xml => escape_attr_value_in::<XmlPolicy, B>(buf, s),
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn escape_attr_value_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
) -> Result<(), WriteError> {
    escape_attr_value_inner::<_, P>(buf, s)
}

#[cfg(debug_assertions)]
#[inline]
pub fn escape_attr_value_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
) -> Result<(), WriteError> {
    let mut sink = WriteBufEscapeSink(buf);
    escape_attr_value_dyn::<P>(&mut sink, s)
}

#[cfg_attr(not(debug_assertions), inline(always))]
#[cfg_attr(debug_assertions, inline)]
pub fn escape_attr_value_xml<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_attr_value_in::<XmlPolicy, B>(buf, s)
}

#[cfg(debug_assertions)]
#[inline(never)]
fn escape_attr_value_dyn<P: EscapePolicy>(
    buf: &mut dyn EscapeSink,
    s: &str,
) -> Result<(), WriteError> {
    escape_attr_value_inner::<_, P>(buf, s)
}

#[inline(always)]
fn escape_attr_value_inner<S, P>(buf: &mut S, s: &str) -> Result<(), WriteError>
where
    S: ?Sized + EscapeSinkLike,
    P: EscapePolicy,
{
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &bytes[i..];

        let off = match memchr3(b'&', b'<', b'>', rest) {
            Some(pos) => match memchr2(b'"', b'\'', &rest[..pos]) {
                Some(quote_pos) => quote_pos,
                None => pos,
            },
            None => match memchr2(b'"', b'\'', rest) {
                Some(pos) => pos,
                None => {
                    emit_with_policy::<_, P>(buf, rest)?;
                    break;
                }
            },
        };

        if off > 0 {
            emit_with_policy::<_, P>(buf, &rest[..off])?;
            i += off;
        }

        match bytes[i] {
            b'&' => buf.extend_static(b"&amp;")?,
            b'<' => buf.extend_static(b"&lt;")?,
            b'>' => buf.extend_static(b"&gt;")?,
            b'"' => buf.extend_static(b"&quot;")?,
            b'\'' => buf.extend_static(b"&#39;")?,
            _ => unreachable!(),
        }
        i += 1;
    }
    Ok(())
}

#[cfg(test)]
fn escape_text_normal<B: WriteBuf>(buf: &mut B, s: &str) -> Result<(), WriteError> {
    escape_text_normal_in::<HtmlPolicy, B>(buf, s)
}

#[cfg(not(debug_assertions))]
#[inline(always)]
fn escape_text_normal_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
) -> Result<(), WriteError> {
    escape_text_normal_inner::<_, P>(buf, s)
}

#[cfg(debug_assertions)]
#[inline]
fn escape_text_normal_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
) -> Result<(), WriteError> {
    let mut sink = WriteBufEscapeSink(buf);
    escape_text_normal_dyn::<P>(&mut sink, s)
}

#[cfg(debug_assertions)]
#[inline(never)]
fn escape_text_normal_dyn<P: EscapePolicy>(
    buf: &mut dyn EscapeSink,
    s: &str,
) -> Result<(), WriteError> {
    escape_text_normal_inner::<_, P>(buf, s)
}

#[inline(always)]
fn escape_text_normal_inner<S, P>(buf: &mut S, s: &str) -> Result<(), WriteError>
where
    S: ?Sized + EscapeSinkLike,
    P: EscapePolicy,
{
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &bytes[i..];

        let Some(off) = memchr3(b'&', b'<', b'>', rest) else {
            emit_with_policy::<_, P>(buf, rest)?;
            break;
        };

        if off > 0 {
            emit_with_policy::<_, P>(buf, &rest[..off])?;
            i += off;
        }

        match bytes[i] {
            b'&' => buf.extend_static(b"&amp;")?,
            b'<' => buf.extend_static(b"&lt;")?,
            b'>' => buf.extend_static(b"&gt;")?,
            _ => unreachable!(),
        }
        i += 1;
    }
    Ok(())
}

/// Escape text inside a raw-text element (`<script>` / `<style>`).
///
/// `tag` is the lowercase element name (`b"script"` or `b"style"`). The
/// function masks three byte sequences that could transition the HTML
/// tokenizer out of the raw-text state or back into tag mode, covering the
/// Go `html/template` CVE series (CVE-2023-24538, CVE-2023-39318,
/// CVE-2023-39319) of script-context escape bypasses:
///
/// - `</tag` followed by a tag-ending char (`>`, whitespace, `/`) — closes
///   the raw-text element (HTML5 §13.2.5.17). Masked to `<\/tag…`.
/// - `<tag` followed by a tag-ending char — transitions into
///   script-data-escaped / script-data-double-escaped (HTML5 §13.2.5.27).
///   Masked to `<\tag…`.
/// - `<!--` — opens the script-data-escaped state (HTML5 §13.2.5.18).
///   Masked to `<\!--`.
///
/// `-->` does NOT need its own mask: every bare `>` byte in the input is
/// escaped to `&gt;`, so the byte sequence `-->` can never appear in the
/// output. `>` escape + `<!--` mask together make the `-->` tokenizer
/// transition unreachable (see HTML5 §13.2.5.22 — you only observe `-->`
/// once script-data-escaped state is entered, which requires an unmasked
/// `<!--`).
///
/// All masks insert a single `\` byte. In a JS string literal, `\x` where
/// `x` is not a recognized escape is interpreted as `x` — so the decoded
/// string value is unchanged. Outside a string, interpolation inside
/// `<script>` already triggers the Gap B compile-time warning, so the
/// developer has been steered away from that path entirely.
///
/// CVE review Gap P. Complements Gap B (compile-time warning).
#[cfg(not(debug_assertions))]
#[inline(always)]
fn escape_text_with_sentinel_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
    tag: &[u8],
) -> Result<(), WriteError> {
    escape_text_with_sentinel_inner::<_, P>(buf, s, tag)
}

#[cfg(debug_assertions)]
#[inline]
fn escape_text_with_sentinel_in<P: EscapePolicy, B: WriteBuf>(
    buf: &mut B,
    s: &str,
    tag: &[u8],
) -> Result<(), WriteError> {
    let mut sink = WriteBufEscapeSink(buf);
    escape_text_with_sentinel_dyn::<P>(&mut sink, s, tag)
}

#[cfg(debug_assertions)]
#[inline(never)]
fn escape_text_with_sentinel_dyn<P: EscapePolicy>(
    buf: &mut dyn EscapeSink,
    s: &str,
    tag: &[u8],
) -> Result<(), WriteError> {
    escape_text_with_sentinel_inner::<_, P>(buf, s, tag)
}

#[inline(always)]
fn escape_text_with_sentinel_inner<S, P>(buf: &mut S, s: &str, tag: &[u8]) -> Result<(), WriteError>
where
    S: ?Sized + EscapeSinkLike,
    P: EscapePolicy,
{
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut flush_start = 0;

    while i < bytes.len() {
        let rest = &bytes[i..];

        // Fast path: scan ahead for the next byte that requires handling.
        // Every `&`, `<`, `>` is escaped to its HTML entity, and `<` can
        // additionally trigger a sentinel mask. Bytes not in that set pass
        // through unchanged and get flushed when the next trigger is found.
        let Some(off) = memchr3(b'&', b'<', b'>', rest) else {
            // No more trigger bytes — fall through to the final flush.
            break;
        };
        i += off;

        // i now points at a trigger byte. Check sentinel masks first.
        if bytes[i] == b'<' {
            // `</tag` closing tag (case-insensitive), with tag-end lookahead.
            if starts_with_close_tag(&bytes[i..], tag) {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static(b"<\\/")?;
                let tag_end = i + 2 + tag.len();
                buf.extend(&bytes[i + 2..tag_end])?;
                i = tag_end;
                flush_start = i;
                continue;
            }
            // `<tag` opening tag (case-insensitive), with tag-end lookahead.
            if starts_with_open_tag(&bytes[i..], tag) {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static(b"<\\")?;
                let tag_end = i + 1 + tag.len();
                buf.extend(&bytes[i + 1..tag_end])?;
                i = tag_end;
                flush_start = i;
                continue;
            }
            // `<!--` HTML comment start.
            if bytes[i..].starts_with(b"<!--") {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static(b"<\\!--")?;
                i += 4;
                flush_start = i;
                continue;
            }
        }

        // Regular HTML escape for `&`, `<`, `>`.
        match bytes[i] {
            b'&' => {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static(b"&amp;")?;
            }
            b'<' => {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static(b"&lt;")?;
            }
            b'>' => {
                flush_impl::<_, P>(buf, bytes, flush_start, i)?;
                buf.extend_static(b"&gt;")?;
            }
            _ => unreachable!(),
        }
        i += 1;
        flush_start = i;
    }

    // Final flush of any trailing pass-through region.
    if flush_start < bytes.len() {
        emit_with_policy::<_, P>(buf, &bytes[flush_start..])?;
    }
    Ok(())
}

/// Flush the pass-through region `[start, end)` into `buf`.
#[inline(always)]
fn flush_impl<S, P>(buf: &mut S, bytes: &[u8], start: usize, end: usize) -> Result<(), WriteError>
where
    S: ?Sized + EscapeSinkLike,
    P: EscapePolicy,
{
    if start < end {
        emit_with_policy::<_, P>(buf, &bytes[start..end])
    } else {
        Ok(())
    }
}

/// `</tag` followed by a tag-ending character (space, tab, LF, CR, FF, `/`,
/// `>`, or EOF). Matches HTML5 §13.2.5.17 end-tag recognition in raw-text
/// element states — the tokenizer accepts any of these as the delimiter
/// after the tag name, so the escaper must recognize them all.
#[inline]
fn starts_with_close_tag(input: &[u8], tag: &[u8]) -> bool {
    if input.len() < 2 + tag.len() {
        return false;
    }
    if &input[..2] != b"</" {
        return false;
    }
    if !input[2..2 + tag.len()].eq_ignore_ascii_case(tag) {
        return false;
    }
    match input.get(2 + tag.len()) {
        None => true,
        Some(&c) => is_tag_end_char(c),
    }
}

/// `<tag` followed by a tag-ending character.
#[inline]
fn starts_with_open_tag(input: &[u8], tag: &[u8]) -> bool {
    if input.len() < 1 + tag.len() {
        return false;
    }
    if input[0] != b'<' {
        return false;
    }
    if !input[1..1 + tag.len()].eq_ignore_ascii_case(tag) {
        return false;
    }
    match input.get(1 + tag.len()) {
        None => true,
        Some(&c) => is_tag_end_char(c),
    }
}

#[inline(always)]
fn is_tag_end_char(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t' | b'\n' | b'\r' | 0x0C /* FF */ | b'/' | b'>'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::ContentMode;
    use proptest::prelude::*;

    fn any_string() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<char>(), 0..256)
            .prop_map(|chars| chars.into_iter().collect::<String>())
    }

    /// A biased generator that mixes arbitrary characters with occasional
    /// injected sentinel-adjacent substrings. Random `any::<char>()` runs
    /// almost never produce `</script` or `<!--` by chance, so we inject
    /// them explicitly to exercise the mask paths that matter.
    fn any_string_with_sentinel_fragments() -> impl Strategy<Value = String> {
        let fragments: Vec<&'static str> = vec![
            "</script>",
            "</script ",
            "</SCRIPT>",
            "</script/",
            "<script>",
            "<script ",
            "<SCRIPT src=x>",
            "<!--",
            "-->",
            "<!-->",
            "<\\/script>",
            "<\\script>",
            "<\\!--",
            "--\\>",
            "a",
            "-",
            "\u{00A0}",
            "\0",
            ">",
        ];
        proptest::collection::vec(proptest::sample::select(fragments), 0..24)
            .prop_map(|parts| parts.concat())
    }

    fn basic_unescape(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut i = 0;
        let bytes = input.as_bytes();
        while i < bytes.len() {
            // Reverse any `<\X` sentinel mask (covers `<\/script`, `<\/style`,
            // `<\script`, `<\style`, `<\!--`). The escape layer guarantees
            // that `<\` only appears in its output as the product of masking
            // because every bare `<` in the input is escaped to `&lt;`.
            if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                out.push('<');
                i += 2;
                continue;
            }
            if bytes[i] == b'&' {
                if input[i..].starts_with("&amp;") {
                    out.push('&');
                    i += 5;
                    continue;
                }
                if input[i..].starts_with("&lt;") {
                    out.push('<');
                    i += 4;
                    continue;
                }
                if input[i..].starts_with("&gt;") {
                    out.push('>');
                    i += 4;
                    continue;
                }
                if input[i..].starts_with("&quot;") {
                    out.push('"');
                    i += 6;
                    continue;
                }
                if input[i..].starts_with("&#39;") {
                    out.push('\'');
                    i += 5;
                    continue;
                }
                if input[i..].starts_with("&nbsp;") {
                    out.push('\u{00A0}');
                    i += 6;
                    continue;
                }
            }
            let ch = input[i..].chars().next().expect("valid UTF-8");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// NULL→FFFD is lossy, so roundtrip comparisons use a normalized original.
    fn normalize_nulls(s: &str) -> String {
        s.replace('\0', "\u{FFFD}")
    }

    /// Assert that the escaped output is safe to embed inside a `<script>`
    /// raw-text element. None of the following HTML5 tokenizer state
    /// transitions must be reachable in the byte stream:
    ///
    /// - `</script` followed by a tag-end char (§13.2.5.17) closes the
    ///   element.
    /// - `<script` followed by a tag-end char (§13.2.5.19) transitions
    ///   into script-data-escaped-double state.
    /// - `<!--` (§13.2.5.18) opens script-data-escaped state.
    /// - `-->` (§13.2.5.22) closes script-data-escaped state.
    ///
    /// All four must appear ONLY in their masked forms after escaping.
    fn assert_script_mode_safe(escaped: &str) {
        assert_no_raw_tag_close(escaped, "script");
        assert_no_raw_tag_open(escaped, "script");
        assert!(
            !escaped.contains("<!--"),
            "raw <!-- in script-mode output: {escaped:?}"
        );
        assert!(
            !escaped.contains("-->"),
            "raw --> in script-mode output: {escaped:?}"
        );
    }

    fn assert_style_mode_safe(escaped: &str) {
        assert_no_raw_tag_close(escaped, "style");
        assert_no_raw_tag_open(escaped, "style");
        assert!(
            !escaped.contains("<!--"),
            "raw <!-- in style-mode output: {escaped:?}"
        );
        assert!(
            !escaped.contains("-->"),
            "raw --> in style-mode output: {escaped:?}"
        );
    }

    /// Check that `</tag` followed by a tag-end char does not appear
    /// anywhere in the output. The scan is case-insensitive, matching
    /// HTML5 end-tag recognition.
    fn assert_no_raw_tag_close(escaped: &str, tag: &str) {
        let needle = format!("</{tag}");
        let lower = escaped.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let mut pos = 0;
        while let Some(off) = lower[pos..].find(&needle) {
            let start = pos + off;
            let after = bytes.get(start + needle.len()).copied();
            if is_html5_tag_end(after) {
                panic!("raw </{tag}<end-char> in output at byte {start}: {escaped:?}");
            }
            pos = start + 1;
        }
    }

    /// Same for `<tag` open-tag start.
    fn assert_no_raw_tag_open(escaped: &str, tag: &str) {
        let needle = format!("<{tag}");
        let lower = escaped.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let mut pos = 0;
        while let Some(off) = lower[pos..].find(&needle) {
            let start = pos + off;
            // Skip `</tag` — that's the close-tag case, already checked.
            if start > 0
                && bytes.get(start - 1) == Some(&b'/')
                && bytes.get(start.wrapping_sub(2)) == Some(&b'<')
            {
                pos = start + 1;
                continue;
            }
            let after = bytes.get(start + needle.len()).copied();
            if is_html5_tag_end(after) {
                panic!("raw <{tag}<end-char> in output at byte {start}: {escaped:?}");
            }
            pos = start + 1;
        }
    }

    fn is_html5_tag_end(c: Option<u8>) -> bool {
        match c {
            None => true,
            Some(b) => matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | b'/' | b'>'),
        }
    }

    proptest! {
        #[test]
        fn text_html_roundtrips(original in any_string()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Html).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn text_script_masks_sentinel(original in any_string()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Script).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_script_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn text_style_masks_sentinel(original in any_string()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Style).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_style_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        /// Same two properties, but exercised against a biased generator
        /// that frequently injects `</script`, `<!--`, `-->`, etc. The
        /// unbiased test almost never generates these substrings so the
        /// Gap P mask paths would otherwise go unexercised under proptest.
        #[test]
        fn text_script_sentinel_biased(original in any_string_with_sentinel_fragments()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Script).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_script_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn text_style_sentinel_biased(original in any_string_with_sentinel_fragments()) {
            let mut buf = Vec::new();
            escape_text(&mut buf, &original, ContentMode::Style).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            assert_style_mode_safe(&escaped);
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }

        #[test]
        fn attr_roundtrips(original in any_string()) {
            let mut buf = Vec::new();
            escape_attr_value(&mut buf, &original).unwrap();
            let escaped = String::from_utf8(buf).unwrap();
            prop_assert!(!escaped.contains('"'));
            prop_assert!(!escaped.contains('\''));
            prop_assert!(!escaped.contains('<'));
            prop_assert!(!escaped.contains('>'));
            let recovered = basic_unescape(&escaped);
            prop_assert_eq!(recovered, normalize_nulls(&original));
        }
    }

    #[test]
    fn nbsp_only_text() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\u{00A0}").unwrap();
        assert_eq!(buf, b"&nbsp;");
    }

    #[test]
    fn nbsp_mixed_with_html_chars() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "a\u{00A0}<b>\u{00A0}&c").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "a&nbsp;&lt;b&gt;&nbsp;&amp;c"
        );
    }

    #[test]
    fn nbsp_consecutive() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\u{00A0}\u{00A0}\u{00A0}").unwrap();
        assert_eq!(buf, b"&nbsp;&nbsp;&nbsp;");
    }

    #[test]
    fn nbsp_0xc2_not_followed_by_0xa0() {
        // U+00BF = [0xC2, 0xBF] — should NOT be escaped
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\u{00BF}").unwrap();
        assert_eq!(buf, "\u{00BF}".as_bytes());
    }

    #[test]
    fn nbsp_in_attr_value() {
        let mut buf = Vec::new();
        escape_attr_value(&mut buf, "hello\u{00A0}world").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "hello&nbsp;world");
    }

    #[test]
    fn nbsp_in_script_sentinel() {
        let mut buf = Vec::new();
        escape_text(&mut buf, "\u{00A0}", ContentMode::Script).unwrap();
        assert_eq!(buf, b"&nbsp;");
    }

    #[test]
    fn null_replaced_with_fffd_in_text() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "a\0b").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a\u{FFFD}b");
    }

    #[test]
    fn null_replaced_with_fffd_in_attr() {
        let mut buf = Vec::new();
        escape_attr_value(&mut buf, "a\0b").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a\u{FFFD}b");
    }

    #[test]
    fn null_only() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\0").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\u{FFFD}");
    }

    #[test]
    fn null_and_nbsp_mixed() {
        let mut buf = Vec::new();
        escape_text_normal(&mut buf, "\0\u{00A0}\0").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\u{FFFD}&nbsp;\u{FFFD}");
    }

    #[test]
    fn xml_text_preserves_nbsp_and_escapes_markup() {
        let mut buf = Vec::new();
        escape_text_xml(&mut buf, "a\u{00A0}<b>&c").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a\u{00A0}&lt;b&gt;&amp;c");
    }

    #[test]
    fn xml_attr_preserves_nbsp_and_escapes_quotes() {
        let mut buf = Vec::new();
        escape_attr_value_xml(&mut buf, "a\u{00A0}\"b'c<d>&").unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "a\u{00A0}&quot;b&#39;c&lt;d&gt;&amp;"
        );
    }

    #[test]
    fn xml_text_rejects_null() {
        let mut buf = Vec::new();
        let err = escape_text_xml(&mut buf, "a\0b").unwrap_err();
        assert_eq!(err, WriteError::InvalidCharForDialect);
    }

    #[test]
    fn xml_attr_rejects_null() {
        let mut buf = Vec::new();
        let err = escape_attr_value_xml(&mut buf, "a\0b").unwrap_err();
        assert_eq!(err, WriteError::InvalidCharForDialect);
    }

    #[test]
    fn xml_policy_rejects_raw_text_modes() {
        let mut buf = Vec::new();
        let err =
            escape_text_in::<XmlPolicy, _>(&mut buf, "body", ContentMode::Script).unwrap_err();
        assert_eq!(err, WriteError::RawTextUnsupportedForDialect);
    }

    // -----------------------------------------------------------------
    // CVE review Gap P — script/style sentinel completeness.
    // Each test covers a Go html/template CVE precedent (CVE-2023-39318,
    // CVE-2023-39319, CVE-2023-24538) plus the new close-tag-with-attrs
    // case.
    // -----------------------------------------------------------------

    fn script_escape(s: &str) -> String {
        let mut buf = Vec::new();
        escape_text(&mut buf, s, ContentMode::Script).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn style_escape(s: &str) -> String {
        let mut buf = Vec::new();
        escape_text(&mut buf, s, ContentMode::Style).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn script_sentinel_exact_close_tag() {
        let out = script_escape("</script>");
        assert!(!out.contains("</script>"));
        assert!(out.contains(r"<\/script"));
    }

    #[test]
    fn script_sentinel_close_tag_with_whitespace() {
        // CVE-2023-39319 shape: `</script ` with trailing whitespace
        // (attribute list) is still a valid end tag per HTML5 §13.2.5.17.
        let out = script_escape("</script foo=bar>");
        assert!(!out.contains("</script"));
        assert!(out.contains(r"<\/script"));
    }

    #[test]
    fn script_sentinel_close_tag_case_insensitive() {
        let out = script_escape("</SCRIPT>");
        assert!(!out.to_ascii_lowercase().contains("</script"));
        assert!(out.contains(r"<\/SCRIPT"));
    }

    #[test]
    fn script_sentinel_open_tag_case_insensitive() {
        // CVE-2023-39318: nested `<script` in script-data-escaped state.
        let out = script_escape("<script src=x>");
        // The HTML tokenizer's script-data-double-escape transition needs
        // `<script` followed by a tag-end char. Masking to `<\script` breaks
        // the transition without corrupting JS string semantics.
        assert!(!out.contains("<script"));
        assert!(out.contains(r"<\script"));
    }

    #[test]
    fn script_sentinel_open_tag_not_masked_when_tag_name_continues() {
        // `<scripted` is NOT a script start-tag (per tokenizer); it must
        // pass through via the `<` → `&lt;` path unchanged.
        let out = script_escape("<scripted>");
        assert!(!out.contains(r"<\scripted"));
        assert!(out.contains("&lt;scripted"));
    }

    #[test]
    fn script_sentinel_html_comment_start() {
        // CVE-2023-39318 precedent: `<!--` inside a JS string transitions
        // into script-data-escaped state.
        let out = script_escape("<!-- comment -->");
        assert!(!out.contains("<!--"));
        assert!(out.contains(r"<\!--"));
    }

    #[test]
    fn script_sentinel_comment_end_unreachable_via_gt_escape() {
        // No explicit `-->` mask is needed: the trailing `>` is always
        // escaped to `&gt;`, so `-->` can never appear in the output even
        // without an `<!--` mask (which we still mask as a belt-and-braces
        // measure in `script_sentinel_html_comment_start`).
        let out = script_escape("<!-- comment -->");
        assert!(!out.contains("-->"));
        // The trailing `>` becomes `&gt;`.
        assert!(out.contains("--&gt;"));
    }

    #[test]
    fn script_sentinel_bare_dash_not_masked() {
        // `&nbsp;` escape in `emit_with_nbsp_null_escape` doesn't touch the
        // `-` byte, but `&` itself is a trigger — assert the output is what
        // we expect byte-for-byte (spaces pass through, `-` passes through).
        let out = script_escape("a - b");
        assert_eq!(out, "a - b");
    }

    #[test]
    fn style_sentinel_covers_style_tag() {
        let out = style_escape("</style>");
        assert!(!out.contains("</style>"));
        assert!(out.contains(r"<\/style"));
    }

    #[test]
    fn style_sentinel_covers_nested_style_open() {
        let out = style_escape("<style>");
        assert!(!out.contains("<style>"));
        assert!(out.contains(r"<\style"));
    }

    #[test]
    fn script_mode_does_not_mask_style_tag() {
        // Only the active tag name is treated as a sentinel.
        let out = script_escape("</style>");
        assert!(out.contains("&lt;/style"));
    }

    #[test]
    fn script_sentinel_all_patterns_combined() {
        let input = "a<script>x</script>b<!--c-->d";
        let out = script_escape(input);
        // None of the raw tokens survive.
        assert!(!out.contains("<script>"));
        assert!(!out.contains("</script>"));
        assert!(!out.contains("<!--"));
        assert!(!out.contains("-->"));
        // `<script`, `</script`, `<!--` show up in masked form.
        assert!(out.contains(r"<\script"));
        assert!(out.contains(r"<\/script"));
        assert!(out.contains(r"<\!--"));
        // `-->` is unreachable because the trailing `>` is always escaped.
        assert!(out.contains("--&gt;"));
    }
}
