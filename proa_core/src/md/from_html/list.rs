// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use htmd::{
    element_handler::{Context, HandlerResult, Handlers},
    node::Node,
    Element,
};

fn integer(node: &Node, name: &str) -> Option<i64> {
    let value = node.attr(name)?;
    let value = value.trim_start_matches([' ', '\t', '\r', '\n', '\u{000C}']);
    let sign = usize::from(value.starts_with(['+', '-']));
    let digits = value.as_bytes()[sign..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    value[..sign + digits].parse::<i32>().ok().map(i64::from)
}

fn is_item(node: &Node) -> bool {
    node.tag_name() == Some("li")
}

// proa-perf-ok(scope): ordered list conversion owns marker and indented Markdown buffers
pub(super) fn ordered_list(
    handlers: &dyn Handlers,
    element: &Element<'_>,
) -> Option<HandlerResult> {
    let children = element.node.children();
    let count = children.iter().filter(|node| is_item(node)).count() as i64;
    let reversed = element.node.attr("reversed").is_some();
    let start = integer(element.node, "start").unwrap_or(if reversed { count } else { 1 });
    // CommonMark renumbers an ordered list after its first marker. Explicit
    // item values/reversed order therefore need labels in an unordered list.
    let labelled = reversed
        || !(0..=999_999_999).contains(&start)
        || start + count.saturating_sub(1) > 999_999_999
        || children
            .iter()
            .any(|node| is_item(&node) && integer(&node, "value").is_some());
    let mut output = String::from("\n\n");
    let mut current = start;
    for child in children.iter() {
        if !is_item(&child) {
            if let Some(result) = handlers.handle(&child, Context::BLOCK) {
                if !result.content.trim().is_empty() {
                    output.push_str(&result.content);
                    output.push_str("\n\n");
                }
            }
            continue;
        }
        current = integer(&child, "value").unwrap_or(current);
        let marker = if labelled {
            format!("- {current}: ")
        } else {
            format!("{current}. ")
        };
        let indent = if labelled { 2 } else { marker.len() };
        let content = handlers.walk_children(&child, Context::BLOCK).content;
        let content = content
            .trim_matches('\n')
            .trim_start_matches([' ', '\r', '\t']);
        output.push_str(&marker);
        let mut lines = content.split('\n');
        output.push_str(lines.next().unwrap_or(""));
        output.push('\n');
        for line in lines {
            if !line.is_empty() {
                for _ in 0..indent {
                    output.push(' ');
                }
                output.push_str(line);
            }
            output.push('\n');
        }
        current += if reversed { -1 } else { 1 };
    }
    output.push_str("\n\n");
    Some(output.into())
}
