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

        let Some((literal, literal_end)) = macro_literal(source, cursor) else {
            continue;
        };
        collect_ranges(source, literal, &mut ranges);
        cursor = literal_end;
    }

    ranges.sort_unstable_by_key(|range| range.start);
    ranges
}

fn macro_literal(source: &str, after_identifier: usize) -> Option<(Range<usize>, usize)> {
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

    let (_, _, literal_end) = if source.as_bytes().get(cursor) == Some(&b'"') {
        let (start, end, literal_end) = normal_string_bounds(source, cursor)?;
        (start, end, literal_end)
    } else {
        let (start, end, literal_end) = raw_string_bounds(source, cursor)?;
        (start, end, literal_end)
    };
    let after_literal = skip_rust_trivia(source, literal_end);
    if source.as_bytes().get(after_literal) != Some(&closing_delimiter) {
        // Multiple tokens select the legacy token grammar. Braces inside one
        // of its string literals are ordinary string bytes.
        return None;
    }
    Some((cursor..literal_end, literal_end))
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

/// Use the compiler's decoded/dedented parser and literal source map rather
/// than maintaining a second Markdown/Rust lexer in the documentation layer.
fn collect_ranges(source: &str, literal: Range<usize>, ranges: &mut Vec<Range<usize>>) {
    use proa_dsl::md::{ExprLocation, MdTemplateInput, TplNode};
    let Ok(template) = syn::parse_str::<MdTemplateInput>(&source[literal.clone()]) else {
        return;
    };
    let Ok(nodes) = template.parse_nodes() else {
        return;
    };
    for node in nodes {
        let TplNode::Expr(expr) = node else { continue };
        let ExprLocation::Literal { range, .. } = expr.location else {
            continue;
        };
        let mut start = literal.start + range.start;
        let mut end = literal.start + range.end;
        // Include adjacent slot delimiters in Rust punctuation highlighting.
        while start > literal.start && source.as_bytes()[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        while end < literal.end && source.as_bytes()[end].is_ascii_whitespace() {
            end += 1;
        }
        if source.as_bytes().get(start.wrapping_sub(1)) == Some(&b'{') {
            start -= 1;
        }
        if source.as_bytes().get(end) == Some(&b'}') {
            end += 1;
        }
        ranges.push(start..end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_quoted_fences_leave_only_real_slots_highlighted() {
        let source = r####"md_sync! { r###"
            {{name}} {{{name}}} \{name} \{{name}} `{{name}}`
            > ~~~text
            > {{name}} {not_rust
            > ~~~
            {name}
            > > ```text
            > > {{name}} {not_rust
            > {name}
            "###
        }"####;
        let ranges = rust_ranges(source);
        assert_eq!(ranges.len(), 4);
        for range in ranges {
            assert_eq!(&source[range], "{name}");
        }
    }

    #[test]
    fn slot_ranges_share_compiler_backtick_and_rust_string_rules() {
        for source in [
            r##"md_sync! { r#"a `` {value} ` end"# }"##,
            r##"md_sync! { r#"Escaped \`{value}`"# }"##,
            r##"md_sync! { r#"{if ready { r"}" } else { "{" }}"# }"##,
        ] {
            let ranges = rust_ranges(source);
            assert_eq!(ranges.len(), 1, "{source}");
            assert!(source[ranges[0].clone()].starts_with('{'));
        }
        for source in [
            r##"md_sync! { r#"`{literal}`"# }"##,
            r##"md_sync! { r#"a `` {literal} `` end"# }"##,
            r##"md_sync! { r#"Even \\`{literal}`"# }"##,
        ] {
            assert!(rust_ranges(source).is_empty(), "{source}");
        }
    }
}

const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

const fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
