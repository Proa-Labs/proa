// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::shared::attr::has_dangerous_url_scheme;
use crate::shared::write_buf::{WriteBuf, WriteError};

/// Whether an ASCII byte must be backslash-escaped to keep literal runtime
/// data inert.
///
/// CommonMark permits escaping every ASCII punctuation byte, but escaping
/// the full set makes ordinary prose unreadable (`GitHub\, Inc\.`,
/// `2001\:db8\:\:\/32`). This escaper is context-free, so it keeps two
/// classes:
///
/// - bytes significant anywhere inline: `` \ ` * _ [ ] ( ) < > | ~ & ``
///   (`&` because a literal value could otherwise complete an entity
///   reference, `~` because of GFM strikethrough, `|` because of tables);
/// - bytes significant at line start: `# + - . ! > =` (list markers,
///   ordered-list dots, headings, blockquotes, setext underlines). A slot
///   can begin a line, and this escaper cannot see its position, so these
///   stay escaped everywhere.
///
/// Everything else (`, : ; ' " / ? @ % ^ $ { }` and friends) has no meaning
/// CommonMark assigns to a bare occurrence and passes through untouched.
#[inline(always)]
const fn is_commonmark_escapable(byte: u8) -> bool {
    matches!(
        byte,
        b'\\'
            | b'`'
            | b'*'
            | b'_'
            | b'['
            | b']'
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'|'
            | b'~'
            | b'&'
            | b'#'
            | b'+'
            | b'-'
            | b'.'
            | b'!'
            | b'='
            // A bare scheme like http://... in a runtime value would become a
            // live GFM autolink; escaping the colon keeps it inert text.
            | b':'
    )
}

/// Exact encoded length of [`escape_md`] for one literal runtime value.
///
/// Returns `None` only if the encoded length would overflow `usize`.
pub fn escaped_md_len(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut len = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 2;
            len = len.checked_add(1)?;
        } else {
            index += 1;
            len = len.checked_add(if is_commonmark_escapable(byte) { 2 } else { 1 })?;
        }
    }
    Some(len)
}

/// Write runtime text as one literal Markdown text node.
///
/// CRLF, lone CR, LF, and tab become spaces so a value cannot create a new
/// block or line-oriented construct. Punctuation that can create inline or
/// line-oriented CommonMark syntax is escaped; benign punctuation stays
/// readable, and other UTF-8 bytes are preserved.
pub fn escape_md<B: WriteBuf>(buf: &mut B, value: &str) -> Result<(), WriteError> {
    let bytes = value.as_bytes();
    let mut chunk_start = 0;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        let is_break = matches!(byte, b'\r' | b'\n' | b'\t');
        if !is_break && !is_commonmark_escapable(byte) {
            index += 1;
            continue;
        }

        if chunk_start < index {
            buf.extend(&bytes[chunk_start..index])?;
        }

        if is_break {
            buf.push(b' ')?;
            if byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
        } else {
            buf.push(b'\\')?;
            buf.push(byte)?;
        }

        index += 1;
        chunk_start = index;
    }

    if chunk_start < bytes.len() {
        buf.extend(&bytes[chunk_start..])?;
    }
    Ok(())
}

/// Write a runtime heading label without allowing it to alter heading syntax.
#[inline(always)]
pub fn write_heading_label<B: WriteBuf>(buf: &mut B, value: &str) -> Result<(), WriteError> {
    escape_md(buf, value)
}

/// Write one runtime blockquote line without allowing nested block syntax.
#[inline(always)]
pub fn write_blockquote_text<B: WriteBuf>(buf: &mut B, value: &str) -> Result<(), WriteError> {
    escape_md(buf, value)
}

#[inline(always)]
fn write_backticks<B: WriteBuf>(buf: &mut B, count: usize) -> Result<(), WriteError> {
    for _ in 0..count {
        buf.push(b'`')?;
    }
    Ok(())
}

fn longest_backtick_run(value: &str) -> usize {
    let mut longest_run = 0usize;
    let mut current_run = 0usize;
    for &byte in value.as_bytes() {
        if byte == b'`' {
            current_run += 1;
            longest_run = longest_run.max(current_run);
        } else {
            current_run = 0;
        }
    }
    longest_run
}

#[derive(Clone, Copy)]
struct CodeSpanAnalysis {
    delimiter_len: usize,
    needs_padding: bool,
}

fn normalized_code_content_len(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut len = bytes.len();
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\r' && bytes[index + 1] == b'\n' {
            len = len.checked_sub(1)?;
            index += 2;
        } else {
            index += 1;
        }
    }
    Some(len)
}

fn analyze_code_span(value: &str) -> CodeSpanAnalysis {
    let bytes = value.as_bytes();
    let longest_run = longest_backtick_run(value);

    let first = bytes.first().copied().map(normalized_code_edge);
    let last = bytes.last().copied().map(normalized_code_edge);
    let all_spaces = !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| matches!(byte, b' ' | b'\r' | b'\n'));
    let needs_padding = first == Some(b'`')
        || last == Some(b'`')
        || (first == Some(b' ') && last == Some(b' ') && !all_spaces);

    CodeSpanAnalysis {
        delimiter_len: longest_run.saturating_add(1),
        needs_padding,
    }
}

#[inline(always)]
const fn normalized_code_edge(byte: u8) -> u8 {
    if matches!(byte, b'\r' | b'\n') {
        b' '
    } else {
        byte
    }
}

fn write_code_span_content<B: WriteBuf>(buf: &mut B, value: &str) -> Result<(), WriteError> {
    let bytes = value.as_bytes();
    let mut chunk_start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if !matches!(byte, b'\r' | b'\n') {
            index += 1;
            continue;
        }

        if chunk_start < index {
            buf.extend(&bytes[chunk_start..index])?;
        }
        buf.push(b' ')?;
        if byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 1;
        }
        index += 1;
        chunk_start = index;
    }

    if chunk_start < bytes.len() {
        buf.extend(&bytes[chunk_start..])?;
    }
    Ok(())
}

/// Write a complete CommonMark inline-code span for arbitrary runtime text.
///
/// The delimiter is one backtick longer than the longest payload run. Padding
/// is added when delimiter adjacency or CommonMark's edge-space normalization
/// would otherwise change the payload. Line endings become single spaces and
/// CRLF is treated as one logical line ending.
pub fn write_inline_code<B: WriteBuf>(buf: &mut B, value: &str) -> Result<(), WriteError> {
    // CommonMark has no empty backtick span: two adjacent backticks form one
    // delimiter run rather than an opening/closing pair. A fixed trusted HTML
    // code element is the only lossless, inert representation of empty code.
    if value.is_empty() {
        return buf.extend_static(b"<code></code>");
    }
    let analysis = analyze_code_span(value);
    write_backticks(buf, analysis.delimiter_len)?;
    if analysis.needs_padding {
        buf.push(b' ')?;
    }
    write_code_span_content(buf, value)?;
    if analysis.needs_padding {
        buf.push(b' ')?;
    }
    write_backticks(buf, analysis.delimiter_len)
}

/// Exact encoded length of [`write_inline_code`].
pub fn inline_code_len(value: &str) -> Option<usize> {
    if value.is_empty() {
        return Some(b"<code></code>".len());
    }
    let analysis = analyze_code_span(value);
    normalized_code_content_len(value)?
        .checked_add(analysis.delimiter_len.checked_mul(2)?)?
        .checked_add(usize::from(analysis.needs_padding) * 2)
}

#[derive(Clone, Copy)]
enum CharacterEncoding {
    Copy,
    BackslashEscape,
    PercentEncode,
}

#[inline(always)]
fn fence_info_encoding(ch: char) -> CharacterEncoding {
    if ch == '`' || ch.is_control() || ch.is_whitespace() {
        CharacterEncoding::PercentEncode
    } else {
        CharacterEncoding::Copy
    }
}

#[inline(always)]
fn destination_encoding(ch: char) -> CharacterEncoding {
    if ch.is_control() || ch.is_whitespace() {
        CharacterEncoding::PercentEncode
    } else if matches!(ch, '\\' | '(' | ')') {
        CharacterEncoding::BackslashEscape
    } else {
        CharacterEncoding::Copy
    }
}

fn measured_char_len(ch: char, encoding: CharacterEncoding) -> Option<usize> {
    let len = ch.len_utf8();
    match encoding {
        CharacterEncoding::Copy => Some(len),
        CharacterEncoding::BackslashEscape => len.checked_add(1),
        CharacterEncoding::PercentEncode => len.checked_mul(3),
    }
}

fn write_fence_info<B: WriteBuf>(buf: &mut B, info: &str) -> Result<(), WriteError> {
    for ch in info.chars() {
        let mut encoded = [0u8; 4];
        let bytes = ch.encode_utf8(&mut encoded).as_bytes();
        match fence_info_encoding(ch) {
            CharacterEncoding::Copy => buf.extend(bytes)?,
            CharacterEncoding::PercentEncode => write_percent_encoded(buf, bytes)?,
            CharacterEncoding::BackslashEscape => unreachable!(),
        }
    }
    Ok(())
}

fn fence_info_len(info: &str) -> Option<usize> {
    info.chars().try_fold(0usize, |len, ch| {
        len.checked_add(measured_char_len(ch, fence_info_encoding(ch))?)
    })
}

/// Write a complete adaptive CommonMark fenced code block.
///
/// The fence is at least three backticks and is one byte longer than every
/// backtick run in `value`, so runtime code can never close it. The info string
/// stays on the opening line: whitespace, controls, and backticks are
/// percent-encoded. Code bytes and authored line endings are otherwise
/// preserved verbatim.
pub fn write_fenced_code<B: WriteBuf>(
    buf: &mut B,
    info: &str,
    value: &str,
) -> Result<(), WriteError> {
    let fence_len = longest_backtick_run(value).saturating_add(1).max(3);
    write_backticks(buf, fence_len)?;
    write_fence_info(buf, info)?;
    buf.push(b'\n')?;
    buf.extend(value.as_bytes())?;
    if !matches!(value.as_bytes().last(), Some(b'\r' | b'\n')) {
        buf.push(b'\n')?;
    }
    write_backticks(buf, fence_len)?;
    buf.extend(b"\n\n")
}

/// Exact encoded length of [`write_fenced_code`].
pub fn fenced_code_len(info: &str, value: &str) -> Option<usize> {
    let fence_len = longest_backtick_run(value).checked_add(1)?.max(3);
    fence_len
        .checked_mul(2)?
        .checked_add(fence_info_len(info)?)?
        .checked_add(3)?
        .checked_add(value.len())?
        .checked_add(usize::from(!matches!(
            value.as_bytes().last(),
            Some(b'\r' | b'\n')
        )))
}

#[inline(always)]
const fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        _ => b'A' + (value - 10),
    }
}

fn write_percent_encoded<B: WriteBuf>(buf: &mut B, bytes: &[u8]) -> Result<(), WriteError> {
    for &byte in bytes {
        buf.push(b'%')?;
        buf.push(hex_digit(byte >> 4))?;
        buf.push(hex_digit(byte & 0x0f))?;
    }
    Ok(())
}

/// Write a sanitized CommonMark link destination without `(` / `)` wrappers.
///
/// Dangerous schemes, including executable `data:` URLs, emit an empty
/// destination. Controls and whitespace are percent-encoded, while backslash
/// and parentheses are CommonMark-backslash-escaped. Other UTF-8 is preserved.
/// The returned boolean reports whether a non-empty destination was emitted.
pub fn write_link_destination<B: WriteBuf>(buf: &mut B, value: &str) -> Result<bool, WriteError> {
    if value.is_empty() || has_dangerous_url_scheme(value) {
        return Ok(false);
    }

    let bytes = value.as_bytes();
    let mut chunk_start = 0usize;
    for (index, ch) in value.char_indices() {
        let char_len = ch.len_utf8();
        let encoding = destination_encoding(ch);
        if matches!(encoding, CharacterEncoding::Copy) {
            continue;
        }

        if chunk_start < index {
            buf.extend(&bytes[chunk_start..index])?;
        }
        match encoding {
            CharacterEncoding::Copy => unreachable!(),
            CharacterEncoding::BackslashEscape => {
                buf.push(b'\\')?;
                buf.extend(&bytes[index..index + char_len])?;
            }
            CharacterEncoding::PercentEncode => {
                write_percent_encoded(buf, &bytes[index..index + char_len])?;
            }
        }
        chunk_start = index + char_len;
    }

    if chunk_start < bytes.len() {
        buf.extend(&bytes[chunk_start..])?;
    }
    Ok(true)
}

/// Exact encoded length of [`write_link_destination`].
///
/// Dangerous and empty destinations measure as zero bytes.
pub fn link_destination_len(value: &str) -> Option<usize> {
    if value.is_empty() || has_dangerous_url_scheme(value) {
        return Some(0);
    }
    value.chars().try_fold(0usize, |len, ch| {
        len.checked_add(measured_char_len(ch, destination_encoding(ch))?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{html, Parser};

    fn escaped(value: &str) -> String {
        let mut buf = Vec::new();
        escape_md(&mut buf, value).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn code(value: &str) -> String {
        let mut buf = Vec::new();
        write_inline_code(&mut buf, value).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn destination(value: &str) -> String {
        let mut buf = Vec::new();
        write_link_destination(&mut buf, value).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn fenced(info: &str, value: &str) -> String {
        let mut buf = Vec::new();
        write_fenced_code(&mut buf, info, value).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn parsed_code_html(value: &str) -> String {
        let source = code(value);
        let mut html_output = String::new();
        html::push_html(&mut html_output, Parser::new(&source));
        html_output
    }

    #[test]
    fn plain_text_and_unicode_are_preserved() {
        assert_eq!(escaped("Hello, 世界"), "Hello, 世界");
    }

    #[test]
    fn escapes_significant_punctuation_and_preserves_benign_punctuation() {
        // Significant anywhere inline, or at a line start a slot could open.
        let significant = r##"!#&()*+-.:<=>[\]_`|~"##;
        let expected = significant
            .bytes()
            .flat_map(|byte| [b'\\', byte])
            .collect::<Vec<_>>();
        assert_eq!(escaped(significant).as_bytes(), expected);
        // No CommonMark meaning attaches to a bare occurrence of these, so
        // prose keeps reading like prose.
        let benign = r##""$%',/;?@^{}"##;
        assert_eq!(escaped(benign), benign);
    }

    #[test]
    fn normalizes_line_endings_and_tabs_without_structural_injection() {
        assert_eq!(
            escaped("safe\r\n# heading\r- item\n<div>\t[x](javascript:bad)"),
            "safe \\# heading \\- item \\<div\\> \\[x\\]\\(javascript\\:bad\\)"
        );
    }

    #[test]
    fn inline_code_selects_a_delimiter_longer_than_every_payload_run() {
        assert_eq!(code("a`b``c```d"), "````a`b``c```d````");
        assert_eq!(code("`edge`"), "`` `edge` ``");
        assert_eq!(code("``````"), "``````` `````` ```````");
    }

    #[test]
    fn inline_code_padding_preserves_edge_spaces_and_normalizes_crlf_once() {
        assert_eq!(code(" value "), "`  value  `");
        assert_eq!(code("all spaces"), "`all spaces`");
        assert_eq!(code("a\r\nb\rc\nd"), "`a b c d`");
    }

    #[test]
    fn inline_code_empty_uses_an_inert_lossless_fallback() {
        assert_eq!(code(""), "<code></code>");
        assert_eq!(inline_code_len(""), Some("<code></code>".len()));
    }

    #[test]
    fn commonmark_parser_preserves_empty_edge_and_all_space_payloads() {
        assert_eq!(parsed_code_html(""), "<p><code></code></p>\n");
        assert_eq!(parsed_code_html(" value "), "<p><code> value </code></p>\n");
        assert_eq!(parsed_code_html("   "), "<p><code>   </code></p>\n");
        assert_eq!(parsed_code_html("`edge`"), "<p><code>`edge`</code></p>\n");
    }

    #[test]
    fn fenced_code_uses_a_payload_safe_delimiter() {
        assert_eq!(
            fenced("rust", "before\n```\nafter"),
            "````rust\nbefore\n```\nafter\n````\n\n"
        );
        assert_eq!(
            fenced("bad` info\n", "code\n"),
            "```bad%60%20info%0A\ncode\n```\n\n"
        );
    }

    #[test]
    fn destination_escapes_markdown_delimiters_and_neutralizes_whitespace() {
        assert_eq!(
            destination("https://example.test/a (b)\\c?q=hello world"),
            "https://example.test/a%20\\(b\\)\\\\c?q=hello%20world"
        );
        assert_eq!(destination("/café/東京"), "/café/東京");
        assert_eq!(destination("/a\r\n\tb"), "/a%0D%0A%09b");
    }

    #[test]
    fn dangerous_destinations_emit_empty() {
        for value in [
            "javascript:alert(1)",
            "java\nscript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "data:image/svg+xml,<svg onload=alert(1)>",
        ] {
            assert_eq!(destination(value), "", "value: {value}");
        }
    }

    #[test]
    fn measurements_exactly_match_every_semantic_writer() {
        for value in [
            "",
            "ASCII",
            "# [x](y) <tag>",
            "crlf\r\ncr\rlf\ntab\t",
            "Unicode 世界\u{2028}space",
            "` `` ``` edge `",
            "javascript:alert(1)",
            "https://example.test/a (b)\\c",
        ] {
            let mut literal = Vec::new();
            escape_md(&mut literal, value).unwrap();
            assert_eq!(
                escaped_md_len(value),
                Some(literal.len()),
                "literal: {value:?}"
            );

            let mut inline = Vec::new();
            write_inline_code(&mut inline, value).unwrap();
            assert_eq!(
                inline_code_len(value),
                Some(inline.len()),
                "code: {value:?}"
            );

            let mut link = Vec::new();
            write_link_destination(&mut link, value).unwrap();
            assert_eq!(
                link_destination_len(value),
                Some(link.len()),
                "destination: {value:?}"
            );

            let mut fence = Vec::new();
            write_fenced_code(&mut fence, value, value).unwrap();
            assert_eq!(
                fenced_code_len(value, value),
                Some(fence.len()),
                "fence: {value:?}"
            );
        }
    }
}
