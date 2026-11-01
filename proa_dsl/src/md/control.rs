// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Control-flow parsing and source layout, shared by macros and the formatter.

use super::{
    extract_braces, fenced_code_end, inline_code_end, layout_prefix_len, split_body,
    subslice_offset, syntax, ExprLocation, IfBranch, TemplateExpr, TplNode,
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
    Parser::default().nodes(input, 0, "", 0, 0)
}

/// Indent multiline template branches without changing their Markdown content.
/// Top-level controls retain `{if ...}` / `{for ...}` delimiters. Standalone
/// nested controls may use ordinary `if ...` / `for ...` blocks.
pub fn format_control_flow(input: &str) -> Result<String, String> {
    let mut parser = Parser {
        record_layout: true,
        ..Default::default()
    };
    let before = parser.nodes(input, 0, "", 0, 0)?;
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
        closers: usize,
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
                        self.control(&input[pos..], base + pos, margin, depth, closers + 1)?;
                    nodes.push(node);
                    pos += consumed;
                    let line_tail = input[pos..].split('\n').next().unwrap_or("").trim();
                    if !line_tail.is_empty() && !line_tail.starts_with('}') {
                        return Err("unexpected tokens after nested Markdown control block; put following Markdown on a new line".into());
                    }
                    at_line_start = false;
                    continue;
                }
                at_line_start = false;
            }

            if bytes[pos] == b'`' {
                if let Some(end) = inline_code_end(input, pos) {
                    for (index, line) in input[pos..end].split_inclusive('\n').enumerate() {
                        let stripped = if index == 0 {
                            0
                        } else {
                            layout_prefix_len(margin, line)
                        };
                        text.push_str(&line[stripped..]);
                    }
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
                let content_start = pos + 1;
                let content = input[content_start..].trim_start();
                let origin =
                    base + content_start + subslice_offset(&input[content_start..], content);
                if syntax::is_control(content) {
                    let (node, consumed) =
                        self.control(content, origin, margin, depth, closers + 2)?;
                    let mut end = origin - base + consumed;
                    end += input[end..].len() - input[end..].trim_start().len();
                    if bytes.get(end) != Some(&b'}') {
                        return Err(
                            "unexpected tokens after Markdown control block; expected closing `}`"
                                .into(),
                        );
                    }
                    pos = end + 1;
                    nodes.push(node);
                    if self.record_layout
                        && depth > 0
                        && input[start..pos].contains('\n')
                        && standalone_range(input, start..pos)
                    {
                        self.redundant_braces.extend([base + start, base + end]);
                    }
                } else {
                    let (content, end) = extract_braces(input, content_start)?;
                    pos = end + 1;
                    let trimmed = content.trim();
                    let origin = base + content_start + subslice_offset(content, trimmed);
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
        closers: usize,
    ) -> Result<(Vec<TplNode>, usize), String> {
        let (body, rest, multiline) = split_body(input, closers)?;
        let margin = if multiline {
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
            self.nodes(body, start, margin, depth, closers)?,
            input.len() - rest.len(),
        ))
    }

    fn control(
        &mut self,
        input: &str,
        base: usize,
        margin: &str,
        depth: usize,
        closers: usize,
    ) -> Result<(TplNode, usize), String> {
        if let Some(rest) = syntax::keyword(input, "for") {
            let body_start = syntax::control_open(input)?;
            let header_start = input.len() - rest.len();
            let header = &input[header_start..body_start];
            let in_pos = syntax::for_in(header).ok_or("expected `in` in for loop")?;
            let pattern = header[..in_pos].trim();
            let iterator = header[in_pos + 2..].trim();
            let pat = syn::parse::Parser::parse_str(syn::Pat::parse_single, pattern)
                .map_err(|error| format!("bad for pattern `{pattern}`: {error}"))?;
            let iter = expression(iterator, base + subslice_offset(input, iterator))?;
            let (body, consumed) = self.body(
                &input[body_start..],
                base + body_start,
                margin,
                depth + 1,
                closers,
            )?;
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
        let mut header_start = 0;
        let mut end;
        loop {
            let header = &input[header_start..];
            let rest = syntax::keyword(header, "if").ok_or("expected `if`")?;
            let body_start = header_start + syntax::control_open(header)?;
            let condition = input[input.len() - rest.len()..body_start].trim();
            let cond = expression(condition, base + subslice_offset(input, condition))?;
            let (body, consumed) = self.body(
                &input[body_start..],
                base + body_start,
                margin,
                depth + 1,
                closers,
            )?;
            branches.push(IfBranch {
                cond: Some(cond),
                body,
            });
            end = body_start + consumed;
            let tail = input[end..].trim_start();
            let Some(after_else) = tail
                .strip_prefix("else")
                .filter(|after| after.starts_with('{') || after.starts_with(char::is_whitespace))
            else {
                break;
            };
            let after_else = after_else.trim_start();
            if syntax::keyword(after_else, "if").is_some() {
                header_start = subslice_offset(input, after_else);
                continue;
            }
            let body_start = subslice_offset(input, after_else);
            let (body, consumed) =
                self.body(after_else, base + body_start, margin, depth + 1, closers)?;
            branches.push(IfBranch { cond: None, body });
            end = body_start + consumed;
            break;
        }
        Ok((TplNode::If { branches }, end))
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
    syntax::is_control(input) && syntax::control_open(input).is_ok()
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
