// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Control-flow parsing and source layout, shared by macros and the formatter.

use super::{
    extract_braces, fenced_code_end, find_body_brace, inline_code_end, layout_prefix_len,
    split_body, subslice_offset, ExprLocation, IfBranch, TemplateExpr, TplNode,
};
use std::ops::Range;
use syn::Expr;

const INDENT_WIDTH: usize = 4;

struct BodyLayout {
    range: Range<usize>,
    margin: String,
    depth: usize,
}

#[derive(Default)]
struct Parser {
    record_layout: bool,
    bodies: Vec<BodyLayout>,
    redundant_braces: Vec<usize>,
}

pub(super) fn parse(input: &str) -> Result<Vec<TplNode>, String> {
    Parser::default().nodes(input, 0, "", 0)
}

/// Indent multiline template branches without changing their Markdown content.
/// Top-level controls retain `{if ...}` / `{for ...}` delimiters. Standalone
/// nested controls may use ordinary `if ...` / `for ...` blocks.
pub fn format_control_flow(input: &str) -> Result<String, String> {
    let mut parser = Parser {
        record_layout: true,
        ..Default::default()
    };
    let before = parser.nodes(input, 0, "", 0)?;
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;
    for line in input.split_inclusive('\n') {
        let content_start = offset + line.len() - line.trim_start_matches([' ', '\t']).len();
        let body = parser
            .bodies
            .iter()
            .filter(|body| body.range.contains(&content_start))
            .max_by_key(|body| body.depth);
        let (depth, stripped) = body.map_or((0, 0), |body| {
            (body.depth, layout_prefix_len(&body.margin, line))
        });
        if !line.trim().is_empty() {
            output.extend(std::iter::repeat_n(' ', depth * INDENT_WIDTH));
        }
        for (index, ch) in line[stripped..].char_indices() {
            if !parser
                .redundant_braces
                .contains(&(offset + stripped + index))
            {
                output.push(ch);
            }
        }
        offset += line.len();
    }
    // In particular, an inline body may continue onto another line. Its
    // whitespace is content, so leave it alone if re-indenting would alter it.
    if !equivalent(&before, &parse(&output)?) {
        return Err("control-flow formatting would change Markdown content".into());
    }
    Ok(output)
}

fn equivalent(a: &[TplNode], b: &[TplNode]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (TplNode::Text(a), TplNode::Text(b)) => a == b,
            (TplNode::Expr(a), TplNode::Expr(b)) => a.expr == b.expr,
            (
                TplNode::For {
                    pat: ap,
                    iter: ai,
                    body: ab,
                },
                TplNode::For {
                    pat: bp,
                    iter: bi,
                    body: bb,
                },
            ) => ap == bp && ai.expr == bi.expr && equivalent(ab, bb),
            (TplNode::If { branches: a }, TplNode::If { branches: b }) => {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(a, b)| {
                        a.cond.as_ref().map(|c| &c.expr) == b.cond.as_ref().map(|c| &c.expr)
                            && equivalent(&a.body, &b.body)
                    })
            }
            _ => false,
        })
}

impl Parser {
    fn nodes(
        &mut self,
        input: &str,
        base: usize,
        margin: &str,
        depth: usize,
    ) -> Result<Vec<TplNode>, String> {
        let mut nodes = Vec::new();
        let mut pos = 0;
        let bytes = input.as_bytes();
        let mut text = String::new();
        let mut at_line_start = true;

        while pos < bytes.len() {
            if at_line_start {
                pos += layout_prefix_len(margin, &input[pos..]);
                if pos == bytes.len() {
                    break;
                }
                if let Some(end) = fenced_code_end(input, pos, margin) {
                    for (index, line) in input[pos..end].split_inclusive('\n').enumerate() {
                        let stripped = if index == 0 {
                            0
                        } else {
                            layout_prefix_len(margin, line)
                        };
                        text.push_str(&line[stripped..]);
                    }
                    pos = end;
                    at_line_start = true;
                    continue;
                }
                // Bare controls are recognized only on their own line inside
                // another control body. Ordinary top-level prose stays prose.
                if depth > 0 && standalone_control_start(&input[pos..]) {
                    if !text.is_empty() {
                        nodes.push(TplNode::Text(std::mem::take(&mut text)));
                    }
                    let (node, consumed) =
                        self.control(&input[pos..], base + pos, margin, depth)?;
                    nodes.push(node);
                    pos += consumed;
                    at_line_start = false;
                    continue;
                }
                at_line_start = false;
            }

            if bytes[pos] == b'`' {
                if let Some(end) = inline_code_end(input, pos) {
                    text.push_str(&input[pos..end]);
                    at_line_start = input[..end].ends_with('\n');
                    pos = end;
                    continue;
                }
            }

            if bytes[pos] == b'{' {
                if bytes.get(pos + 1) == Some(&b'{') {
                    text.push('{');
                    pos += 2;
                    continue;
                }
                if !text.is_empty() {
                    nodes.push(TplNode::Text(std::mem::take(&mut text)));
                }
                let start = pos;
                pos += 1;
                let content_start = pos;
                let (content, end) = extract_braces(input, pos)?;
                pos = end + 1;
                let trimmed = content.trim();
                let origin = base + content_start + subslice_offset(content, trimmed);
                if trimmed.starts_with("for ") || trimmed.starts_with("if ") {
                    let (node, _) = self.control(trimmed, origin, margin, depth)?;
                    nodes.push(node);
                    if self.record_layout
                        && depth > 0
                        && content.contains('\n')
                        && standalone_range(input, start..pos)
                    {
                        self.redundant_braces.extend([base + start, base + end]);
                    }
                } else {
                    let expr = syn::parse_str::<Expr>(trimmed)
                        .map_err(|error| format!("bad expression `{trimmed}`: {error}"))?;
                    nodes.push(TplNode::Expr(TemplateExpr {
                        expr,
                        location: ExprLocation::Decoded(origin..origin + trimmed.len()),
                    }));
                }
            } else if bytes[pos] == b'}' {
                if bytes.get(pos + 1) == Some(&b'}') {
                    text.push('}');
                    pos += 2;
                    continue;
                }
                return Err(format!("unexpected `}}` at byte {}", base + pos));
            } else {
                let ch = input[pos..].chars().next().unwrap();
                text.push(ch);
                pos += ch.len_utf8();
                at_line_start = ch == '\n';
            }
        }
        if !text.is_empty() {
            nodes.push(TplNode::Text(text));
        }
        Ok(nodes)
    }

    fn body(
        &mut self,
        input: &str,
        base: usize,
        parent_margin: &str,
        depth: usize,
    ) -> Result<(Vec<TplNode>, usize), String> {
        let (body, rest) = split_body(input)?;
        let margin = if input.starts_with("{\n") {
            branch_margin(body, parent_margin)
        } else {
            ""
        };
        let start = base + subslice_offset(input, body);
        if self.record_layout {
            self.bodies.push(BodyLayout {
                range: start..start + body.len(),
                margin: margin.to_owned(),
                depth,
            });
        }
        Ok((
            self.nodes(body, start, margin, depth)?,
            input.len() - rest.len(),
        ))
    }

    fn control(
        &mut self,
        input: &str,
        base: usize,
        margin: &str,
        depth: usize,
    ) -> Result<(TplNode, usize), String> {
        if let Some(rest) = input.strip_prefix("for ") {
            let brace = find_body_brace(rest).ok_or("expected `{` in for body")?;
            let header = &rest[..brace];
            let in_pos = header.find(" in ").ok_or("expected `in` in for loop")?;
            let pattern = header[..in_pos].trim();
            let iterator = header[in_pos + 4..].trim();
            let pat = syn::parse::Parser::parse_str(syn::Pat::parse_single, pattern)
                .map_err(|error| format!("bad for pattern `{pattern}`: {error}"))?;
            let iter = expression(iterator, base + subslice_offset(input, iterator))?;
            let body_start = 4 + brace;
            let (body, consumed) =
                self.body(&input[body_start..], base + body_start, margin, depth + 1)?;
            return Ok((
                TplNode::For {
                    pat: Box::new(pat),
                    iter: Box::new(iter),
                    body,
                },
                body_start + consumed,
            ));
        }

        let mut branches = Vec::new();
        let mut offset = 3; // `if `
        loop {
            let rest = &input[offset..];
            let brace = find_body_brace(rest).ok_or("expected `{` in if body")?;
            let condition = rest[..brace].trim();
            let cond = expression(condition, base + subslice_offset(input, condition))?;
            let body_start = offset + brace;
            let (body, consumed) =
                self.body(&input[body_start..], base + body_start, margin, depth + 1)?;
            branches.push(IfBranch {
                cond: Some(cond),
                body,
            });
            offset = body_start + consumed;
            let tail = input[offset..].trim_start();
            if tail.starts_with("else if ") {
                offset = subslice_offset(input, tail) + 8;
            } else if let Some(after_else) = tail
                .strip_prefix("else")
                .filter(|after| after.starts_with('{') || after.starts_with(char::is_whitespace))
            {
                let after_else = after_else.trim_start();
                let body_start = subslice_offset(input, after_else);
                let (body, consumed) =
                    self.body(after_else, base + body_start, margin, depth + 1)?;
                branches.push(IfBranch { cond: None, body });
                offset = body_start + consumed;
                break;
            } else {
                break;
            }
        }
        Ok((TplNode::If { branches }, offset))
    }
}

fn expression(input: &str, base: usize) -> Result<TemplateExpr, String> {
    Ok(TemplateExpr {
        expr: syn::parse_str(input)
            .map_err(|error| format!("bad expression `{input}`: {error}"))?,
        location: ExprLocation::Decoded(base..base + input.len()),
    })
}

/// Remove the inherited layout plus at most one new level. Additional spaces
/// remain Markdown indentation, including nested lists and indented code.
fn branch_margin<'a>(body: &'a str, parent: &str) -> &'a str {
    let common = crate::md_layout::layout_indent(body);
    let inherited = layout_prefix_len(parent, common);
    let extra = &common[inherited..];
    let added = if extra.starts_with('\t') {
        1
    } else if extra.starts_with("    ") {
        INDENT_WIDTH
    } else {
        0
    };
    &common[..inherited + added]
}

fn standalone_control_start(input: &str) -> bool {
    if !input.starts_with("for ") && !input.starts_with("if ") {
        return false;
    }
    let line = input.lines().next().unwrap_or("");
    find_body_brace(line).is_some_and(|brace| line[brace + 1..].trim().is_empty())
}

fn standalone_range(input: &str, range: Range<usize>) -> bool {
    input[..range.start]
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim()
        .is_empty()
        && input[range.end..]
            .split('\n')
            .next()
            .unwrap_or("")
            .trim()
            .is_empty()
}
