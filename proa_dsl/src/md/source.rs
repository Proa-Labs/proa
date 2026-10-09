// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-time mapping from normalized template bytes to authored Rust literals.

use super::{ExprLocation, TemplateExpr, TplNode};
use proc_macro2::Span;
use std::ops::Range;
use syn::LitStr;

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
        if let Some(body) = value.strip_prefix('\n') {
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
        for node in nodes {
            match node {
                TplNode::Text(_) => {}
                TplNode::Expr(expr) => self.map_expr(expr, span),
                TplNode::For { iter, body, .. } => {
                    self.map_expr(iter, span);
                    self.map_nodes(body, span);
                }
                TplNode::If { branches } => {
                    for branch in branches {
                        if let Some(cond) = &mut branch.cond {
                            self.map_expr(cond, span);
                        }
                        self.map_nodes(&mut branch.body, span);
                    }
                }
            }
        }
    }

    fn map_expr(&self, expr: &mut TemplateExpr, span: Span) {
        let ExprLocation::Decoded(range) = &expr.location else {
            return;
        };
        expr.location = self
            .ranges
            .as_ref()
            .and_then(|ranges| {
                Some(ExprLocation::Literal {
                    span,
                    range: ranges.get(range.start)?.start
                        ..ranges.get(range.end.checked_sub(1)?)?.end,
                })
            })
            .unwrap_or(ExprLocation::Rust(span));
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
    fn nested_conditions_and_loops_keep_ranges() {
        let input = "r###\"\n    {if self.ready {\n    {for item in &self.items {\n    {item.title}\n    }}\n    }}\n    \"###";
        let template: MdTemplateInput = syn::parse_str(input).unwrap();
        let nodes = template.semantic_nodes().unwrap();
        let TplNode::If { branches } = &nodes[0] else {
            panic!();
        };
        let cond = branches[0].cond.as_ref().unwrap();
        let ExprLocation::Literal { range, .. } = &cond.location else {
            panic!();
        };
        assert_eq!(
            &template.lit.token().to_string()[range.clone()],
            "self.ready"
        );
        let loop_node = branches[0]
            .body
            .iter()
            .find(|node| matches!(node, TplNode::For { .. }))
            .unwrap();
        let TplNode::For { iter, body, .. } = loop_node else {
            panic!();
        };
        let ExprLocation::Literal { range, .. } = &iter.location else {
            panic!();
        };
        assert_eq!(
            &template.lit.token().to_string()[range.clone()],
            "&self.items"
        );
        let TplNode::Expr(item) = &body[0] else {
            panic!();
        };
        let ExprLocation::Literal { range, .. } = &item.location else {
            panic!();
        };
        assert_eq!(
            &template.lit.token().to_string()[range.clone()],
            "item.title"
        );
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
