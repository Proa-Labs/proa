// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-time mapping from normalized template bytes to authored Rust literals.

use super::{ExprLocation, TplNode};
use proc_macro2::Span;
use std::ops::Range;
use std::sync::Arc;
use syn::LitStr;

/// Decoded-byte to authored-token mapping for one template literal.
#[derive(Debug)]
pub(super) struct LiteralMap {
    span: Span,
    ranges: Option<Vec<Range<usize>>>,
}

impl LiteralMap {
    /// Map a decoded byte range to the literal token, falling back to the
    /// whole literal when the lexical token differs from its decoded value.
    pub(super) fn locate(&self, decoded: Range<usize>) -> ExprLocation {
        self.ranges
            .as_ref()
            .and_then(|ranges| {
                Some(ExprLocation::Literal {
                    span: self.span,
                    range: ranges.get(decoded.start)?.start
                        ..ranges.get(decoded.end.checked_sub(1)?)?.end,
                })
            })
            .unwrap_or(ExprLocation::Rust(self.span))
    }
}

pub(super) struct MappedLiteral {
    pub text: String,
    ranges: Option<Vec<Range<usize>>>,
}

impl MappedLiteral {
    pub fn new(lit: &LitStr) -> Self {
        let value = lit.value();
        let encoded = lit
            .span()
            .source_text()
            .unwrap_or_else(|| lit.token().to_string());
        let original_ranges = decode_ranges(&encoded, &value);
        let mut text = String::with_capacity(value.len());
        let mut kept = Vec::with_capacity(value.len());
        let mut append = |range: Range<usize>| {
            text.push_str(&value[range.clone()]);
            kept.extend(range);
        };
        // A leading newline is layout only when content follows it; a
        // whitespace-only literal such as `"\n"` is itself the content.
        if let Some(body) = value
            .strip_prefix('\n')
            .filter(|body| !body.trim().is_empty())
        {
            let common = crate::md_layout::layout_indent(body);
            let mut offset = 1;
            for line in body.split_inclusive('\n') {
                let prefix = common
                    .chars()
                    .zip(line.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(ch, _)| ch.len_utf8())
                    .sum::<usize>();
                append(offset + prefix..offset + line.len());
                offset += line.len();
            }
        } else if super::is_multiline_raw_literal(lit) {
            let common = value
                .split('\n')
                .skip(1)
                .filter(|line| !line.trim_matches([' ', '\t', '\r']).is_empty())
                .map(|line| {
                    line.bytes()
                        .take_while(|b| matches!(b, b' ' | b'\t'))
                        .count()
                })
                .min()
                .unwrap_or(0);
            let mut offset = 0;
            for (index, line) in value.split_inclusive('\n').enumerate() {
                let content_len = line.strip_suffix('\n').map_or(line.len(), str::len);
                if index == 0 || common == 0 {
                    append(offset..offset + line.len());
                } else if line[..content_len]
                    .trim_matches([' ', '\t', '\r'])
                    .is_empty()
                {
                    if line[..content_len].ends_with('\r') {
                        append(offset + content_len - 1..offset + content_len);
                    }
                    if line.ends_with('\n') {
                        append(offset + content_len..offset + line.len());
                    }
                } else {
                    append(offset + common..offset + line.len());
                }
                offset += line.len();
            }
        } else {
            append(0..value.len());
        }
        let ranges =
            original_ranges.map(|ranges| kept.into_iter().map(|i| ranges[i].clone()).collect());
        Self { text, ranges }
    }

    pub fn map_nodes(&self, nodes: &mut [TplNode], span: Span) {
        let literal = Arc::new(LiteralMap {
            span,
            ranges: self.ranges.clone(),
        });
        for node in nodes {
            if let TplNode::Expr(expr) = node {
                if let ExprLocation::Decoded(range) = &expr.location {
                    expr.location = literal.locate(range.clone());
                }
                expr.attach(&literal);
            }
        }
    }
}

/// Map decoded bytes to the source character/escape that produces them.
/// If the lexical token differs from the decoded value, preserve the literal's
/// span rather than manufacturing a precise but incorrect source range.
fn decode_ranges(encoded: &str, expected: &str) -> Option<Vec<Range<usize>>> {
    let quote = encoded.find('"')?;
    let raw = encoded[..quote].starts_with('r');
    let hashes = if raw {
        encoded[..quote].bytes().filter(|b| *b == b'#').count()
    } else {
        0
    };
    let end = encoded.len().checked_sub(1 + hashes)?;
    let mut position = quote + 1;
    let mut decoded = String::with_capacity(expected.len());
    let mut ranges = Vec::with_capacity(expected.len());
    while position < end {
        let start = position;
        let ch = encoded[position..end].chars().next()?;
        position += ch.len_utf8();
        let produced = if ch == '\\' && !raw {
            let escape = encoded[position..end].chars().next()?;
            position += escape.len_utf8();
            match escape {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '0' => '\0',
                '\\' => '\\',
                '"' => '"',
                '\'' => '\'',
                'x' => {
                    let hex = encoded.get(position..position + 2)?;
                    position += 2;
                    char::from_u32(u32::from_str_radix(hex, 16).ok()?)?
                }
                'u' => {
                    if encoded.as_bytes().get(position) != Some(&b'{') {
                        return None;
                    }
                    position += 1;
                    let finish = position + encoded[position..end].find('}')?;
                    let hex = encoded[position..finish].replace('_', "");
                    position = finish + 1;
                    char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?
                }
                '\n' | '\r' => {
                    if escape == '\r' {
                        if encoded.as_bytes().get(position) != Some(&b'\n') {
                            return None;
                        }
                        position += 1;
                    }
                    while position < end {
                        let next = encoded[position..end].chars().next()?;
                        if !matches!(next, ' ' | '\t' | '\n' | '\r') {
                            break;
                        }
                        position += next.len_utf8();
                    }
                    continue;
                }
                _ => return None,
            }
        } else if ch == '\r' && encoded.as_bytes().get(position) == Some(&b'\n') {
            position += 1;
            '\n'
        } else {
            ch
        };
        decoded.push(produced);
        ranges.extend(std::iter::repeat_n(start..position, produced.len_utf8()));
    }
    (decoded == expected).then_some(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::{MdTemplateInput, TplNode};

    fn literal_expression(input: &str) -> (String, String) {
        let template: MdTemplateInput = syn::parse_str(input).unwrap();
        let nodes = template.semantic_nodes().unwrap();
        let expression = nodes
            .iter()
            .find_map(|node| match node {
                TplNode::Expr(expr) => Some(expr),
                _ => None,
            })
            .unwrap();
        let ExprLocation::Literal { range, .. } = &expression.location else {
            panic!("literal range missing");
        };
        (
            template.lit.token().to_string()[range.clone()].to_string(),
            quote::quote!(#expression).to_string(),
        )
    }

    #[test]
    fn raw_dedenting_preserves_authored_expression() {
        let input = "r###\"\n    # Héllo\n\n    { self.title }\n    \"###";
        let (source, expression) = literal_expression(input);
        assert_eq!(source, "self.title");
        assert_eq!(expression, "self . title");
    }

    #[test]
    fn cooked_escapes_preserve_source_ranges() {
        let (source, expression) = literal_expression("\"\\n    \\u{263a} { self.title }\\n\"");
        assert_eq!(source, "self.title");
        assert_eq!(expression, "self . title");
    }

    #[test]
    fn control_slots_keep_their_whole_authored_range() {
        let input = "r###\"\n    {if self.ready {\n        md_sync! { \"- \" {item.title} }\n    }}\n    \"###";
        let template: MdTemplateInput = syn::parse_str(input).unwrap();
        let nodes = template.semantic_nodes().unwrap();
        let TplNode::Expr(slot) = &nodes[0] else {
            panic!("{nodes:?}");
        };
        assert!(matches!(slot.expr, syn::Expr::If(_)));
        let ExprLocation::Literal { range, .. } = &slot.location else {
            panic!("literal range missing");
        };
        let source = &template.lit.token().to_string()[range.clone()];
        assert!(source.starts_with("if self.ready {"), "{source}");
        assert!(source.ends_with('}'), "{source}");
        assert!(source.contains("{item.title}"), "{source}");
    }

    #[test]
    fn named_arguments_have_real_rust_spans() {
        let template: MdTemplateInput =
            syn::parse_str("r###\"{title}\"###, title = self.name").unwrap();
        let nodes = template.semantic_nodes().unwrap();
        let TplNode::Expr(expr) = &nodes[0] else {
            panic!();
        };
        assert!(matches!(expr.location, ExprLocation::Rust(_)));
        assert_eq!(quote::quote!(#expr).to_string(), "self . name");
    }

    #[test]
    fn normalization_matches_existing_layout_rules() {
        for value in [
            "\n    a\n      b\n    ",
            "\n\ta\n\tb\n\t",
            "\n  a\n\tb\n",
            "first\n  second\n  ",
            "first\n\tsecond\n\t",
            "first\n    a\n\r\n    b\n",
            "\n",
            "\n    ",
        ] {
            let lit = LitStr::new(value, Span::call_site());
            let mapped = MappedLiteral::new(&lit);
            let expected = if value.starts_with('\n') {
                crate::md_layout::dedent_template(value)
            } else if super::super::is_multiline_raw_literal(&lit) {
                super::super::dedent_multiline_literal(value.to_owned())
            } else {
                value.to_owned()
            };
            assert_eq!(mapped.text, expected);
        }
    }
}
