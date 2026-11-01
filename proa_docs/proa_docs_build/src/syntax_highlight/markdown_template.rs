// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;

/// Locate Rust expressions embedded in the single string literal accepted by
/// `md!` and `md_sync!`.
///
/// Syntect sees the literal as Rust string content. Proa adds a language layer
/// inside it: ordinary bytes are Markdown, while each brace-delimited slot is
/// one Rust expression, control flow included. Keeping this scanner specific to
/// those two macros prevents unrelated Rust strings containing braces from
/// being reclassified.
pub(super) fn rust_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut cursor = 0;

    while cursor < bytes.len() {
        if source[cursor..].starts_with("//") {
            cursor = source[cursor..]
                .find('\n')
                .map_or(source.len(), |offset| cursor + offset + 1);
            continue;
        }
        if source[cursor..].starts_with("/*") {
            cursor = skip_block_comment(source, cursor);
            continue;
        }
        if bytes[cursor] == b'"' {
            cursor = normal_string_bounds(source, cursor)
                .map_or(source.len(), |(_, _, literal_end)| literal_end);
            continue;
        }
        if let Some((_, _, literal_end)) = raw_string_bounds(source, cursor) {
            cursor = literal_end;
            continue;
        }
        if !is_identifier_start(bytes[cursor]) {
            cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
            continue;
        }

        let identifier_start = cursor;
        cursor += 1;
        while cursor < bytes.len() && is_identifier_continue(bytes[cursor]) {
            cursor += 1;
        }
        if !matches!(&source[identifier_start..cursor], "md" | "md_sync") {
            continue;
        }

        let Some((content, literal_end, raw)) = macro_literal(source, cursor) else {
            continue;
        };
        collect_ranges(source, content, raw, &mut ranges);
        cursor = literal_end;
    }

    ranges.sort_unstable_by_key(|range| range.start);
    ranges
}

fn macro_literal(source: &str, after_identifier: usize) -> Option<(Range<usize>, usize, bool)> {
    let mut cursor = skip_rust_trivia(source, after_identifier);
    if source.as_bytes().get(cursor) != Some(&b'!') {
        return None;
    }
    cursor = skip_rust_trivia(source, cursor + 1);
    let closing_delimiter = match source.as_bytes().get(cursor) {
        Some(b'{') => b'}',
        Some(b'(') => b')',
        Some(b'[') => b']',
        _ => return None,
    };
    cursor = skip_rust_trivia(source, cursor + 1);

    let (start, end, literal_end, raw) = if source.as_bytes().get(cursor) == Some(&b'"') {
        let (start, end, literal_end) = normal_string_bounds(source, cursor)?;
        (start, end, literal_end, false)
    } else {
        let (start, end, literal_end) = raw_string_bounds(source, cursor)?;
        (start, end, literal_end, true)
    };
    let after_literal = skip_rust_trivia(source, literal_end);
    if source.as_bytes().get(after_literal) != Some(&closing_delimiter) {
        // Multiple tokens select the legacy token grammar. Braces inside one
        // of its string literals are ordinary string bytes.
        return None;
    }
    Some((start..end, literal_end, raw))
}

fn skip_rust_trivia(source: &str, mut cursor: usize) -> usize {
    while cursor < source.len() {
        let byte = source.as_bytes()[cursor];
        if byte.is_ascii_whitespace() {
            cursor += 1;
        } else if source[cursor..].starts_with("//") {
            cursor = source[cursor..]
                .find('\n')
                .map_or(source.len(), |offset| cursor + offset + 1);
        } else if source[cursor..].starts_with("/*") {
            cursor = skip_block_comment(source, cursor);
        } else {
            break;
        }
    }
    cursor
}

fn skip_block_comment(source: &str, start: usize) -> usize {
    let mut cursor = start + 2;
    let mut depth = 1_u32;
    while cursor < source.len() {
        if source[cursor..].starts_with("/*") {
            depth += 1;
            cursor += 2;
        } else if source[cursor..].starts_with("*/") {
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return cursor;
            }
        } else {
            cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
    source.len()
}

fn normal_string_bounds(source: &str, quote: usize) -> Option<(usize, usize, usize)> {
    let mut cursor = quote + 1;
    while cursor < source.len() {
        match source.as_bytes()[cursor] {
            b'\\' => {
                cursor += 1;
                if cursor < source.len() {
                    cursor += source[cursor..].chars().next()?.len_utf8();
                }
            }
            b'"' => return Some((quote + 1, cursor, cursor + 1)),
            _ => cursor += source[cursor..].chars().next()?.len_utf8(),
        }
    }
    None
}

fn raw_string_bounds(source: &str, start: usize) -> Option<(usize, usize, usize)> {
    if source.as_bytes().get(start) != Some(&b'r') {
        return None;
    }
    let mut quote = start + 1;
    while source.as_bytes().get(quote) == Some(&b'#') {
        quote += 1;
    }
    if source.as_bytes().get(quote) != Some(&b'"') {
        return None;
    }
    let hashes = quote - start - 1;
    let closing = format!("\"{}", "#".repeat(hashes));
    let content_start = quote + 1;
    let relative_end = source[content_start..].find(&closing)?;
    let content_end = content_start + relative_end;
    Some((content_start, content_end, content_end + closing.len()))
}

fn collect_ranges(source: &str, template: Range<usize>, raw: bool, ranges: &mut Vec<Range<usize>>) {
    let mut cursor = template.start;
    while cursor < template.end {
        if source[cursor..template.end].starts_with("{{") {
            cursor += 2;
            continue;
        }
        if source.as_bytes()[cursor] != b'{' {
            cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
            continue;
        }

        let Some(close) = matching_brace(source, cursor, template.end, raw) else {
            break;
        };
        // The whole slot is Rust; a nested template literal inside it is an
        // ordinary Rust string token.
        ranges.push(cursor..close + 1);
        cursor = close + 1;
    }
}

fn matching_brace(source: &str, open: usize, end: usize, raw: bool) -> Option<usize> {
    let mut cursor = open;
    let mut depth = 0_i32;
    let mut in_string = false;
    while cursor < end {
        let (ch, width) = template_char(source, cursor, raw)?;
        if in_string {
            if ch == '"' {
                in_string = false;
            }
            cursor += width;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += width;
    }
    None
}

/// Return the next logical template character and its source width. Quotes in
/// an ordinary Rust string appear as `\"`; raw strings contain them directly.
fn template_char(source: &str, cursor: usize, raw: bool) -> Option<(char, usize)> {
    if !raw && source[cursor..].starts_with("\\\"") {
        return Some(('"', 2));
    }
    let ch = source[cursor..].chars().next()?;
    Some((ch, ch.len_utf8()))
}

const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

const fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
