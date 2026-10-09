// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::rc::Rc;

use htmd::{
    element_handler::{HandlerResult, Handlers},
    Element, Node,
};
use markup5ever_rcdom::NodeData;

struct Row {
    cells: Vec<Rc<Node>>,
    in_head: bool,
}

fn tag(node: &Node) -> Option<&str> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.as_ref()),
        _ => None,
    }
}

// proa-perf-ok(scope): opt-in table conversion owns its row/cell snapshot
fn rows(node: &Node, in_head: bool, output: &mut Vec<Row>) {
    for child in node.children.borrow().iter() {
        match tag(child) {
            Some("thead" | "tbody" | "tfoot") => rows(child, tag(child) == Some("thead"), output),
            Some("tr") => output.push(Row {
                cells: child
                    .children
                    .borrow()
                    .iter()
                    .filter(|cell| matches!(tag(cell), Some("td" | "th")))
                    .cloned()
                    .collect(),
                in_head,
            }),
            _ => {}
        }
    }
}

fn complex(node: &Node, is_cell: bool) -> bool {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        if is_cell
            && attrs.borrow().iter().any(|attr| {
                matches!(attr.name.local.as_ref(), "rowspan" | "colspan")
                    && attr.value.as_ref() != "1"
            })
        {
            return true;
        }
        // Unknown/custom elements may contain block semantics. Use a
        // conservative inline allowlist instead of flattening their output.
        if !is_cell
            && !matches!(
                name.local.as_ref(),
                "a" | "abbr"
                    | "b"
                    | "bdi"
                    | "bdo"
                    | "cite"
                    | "code"
                    | "data"
                    | "dfn"
                    | "em"
                    | "i"
                    | "img"
                    | "kbd"
                    | "mark"
                    | "q"
                    | "rp"
                    | "rt"
                    | "ruby"
                    | "s"
                    | "samp"
                    | "small"
                    | "span"
                    | "strong"
                    | "sub"
                    | "sup"
                    | "time"
                    | "u"
                    | "var"
                    | "wbr"
                    | "del"
                    | "ins"
            )
        {
            return true;
        }
    }
    node.children
        .borrow()
        .iter()
        .any(|child| complex(child, false))
}

fn header(row: &Row) -> bool {
    row.in_head
        || row.cells.iter().any(|cell| {
            if tag(cell) != Some("th") {
                return false;
            }
            let NodeData::Element { attrs, .. } = &cell.data else {
                return false;
            };
            !attrs.borrow().iter().any(|attr| {
                attr.name.local.as_ref() == "scope"
                    && (attr.value.eq_ignore_ascii_case("row")
                        || attr.value.eq_ignore_ascii_case("rowgroup"))
            })
        })
}

// proa-perf-ok(scope): buffered table conversion keeps every cell instead of filtering th/td separately
pub(super) fn table(handlers: &dyn Handlers, element: Element<'_>) -> Option<HandlerResult> {
    let mut source_rows = Vec::new();
    rows(element.node, false, &mut source_rows);
    let columns = source_rows
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    let mut output = String::from("\n\n");
    for child in element
        .node
        .children
        .borrow()
        .iter()
        .filter(|node| tag(node) == Some("caption"))
    {
        let caption = handlers.walk_children(child).content;
        if !caption.trim().is_empty() {
            output.push_str(caption.trim_matches('\n'));
            output.push_str("\n\n");
        }
    }
    if columns == 0 {
        return Some(output.into());
    }
    let linear = source_rows.iter().filter(|row| row.in_head).count() > 1
        || source_rows
            .iter()
            .flat_map(|row| &row.cells)
            .any(|cell| complex(cell, true));
    let has_header = source_rows.first().is_some_and(header);
    let rendered: Vec<Vec<String>> = source_rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| handlers.walk_children(cell).content)
                .collect()
        })
        .collect();
    if linear {
        // Markdown cannot encode spans or block content in a pipe cell.
        // Ordered items retain row order, with nested items retaining cells.
        for (index, row) in rendered.iter().enumerate() {
            let marker = format!("{}.\n", index + 1);
            let indent = marker.len() + 1;
            output.push_str(&marker);
            for cell in row {
                let cell = cell
                    .trim_matches('\n')
                    .trim_start_matches([' ', '\r', '\t']);
                for _ in 0..indent {
                    output.push(' ');
                }
                output.push_str("- ");
                let mut lines = cell.split('\n');
                output.push_str(lines.next().unwrap_or(""));
                output.push('\n');
                for line in lines {
                    for _ in 0..indent + 2 {
                        output.push(' ');
                    }
                    output.push_str(line);
                    output.push('\n');
                }
            }
            output.push('\n');
        }
    } else {
        let empty = Vec::new();
        let first = if has_header { &rendered[0] } else { &empty };
        pipe_row(&mut output, first, columns);
        output.push('|');
        for _ in 0..columns {
            output.push_str(" --- |");
        }
        output.push('\n');
        for row in rendered.iter().skip(usize::from(has_header)) {
            pipe_row(&mut output, row, columns);
        }
    }
    output.push_str("\n\n");
    Some(output.into())
}

// proa-perf-ok(scope): pipe tables are built in the opt-in conversion buffer
fn pipe_row(output: &mut String, cells: &[String], columns: usize) {
    output.push('|');
    for column in 0..columns {
        output.push(' ');
        if let Some(cell) = cells.get(column) {
            let cell = cell.trim_matches([' ', '\r', '\n', '\t']);
            let mut backticks = 0;
            let mut code_delimiter = 0;
            let mut slashes = 0;
            for ch in cell.chars() {
                if ch == '`' && (code_delimiter > 0 || slashes % 2 == 0) {
                    backticks += 1;
                } else {
                    if backticks > 0 {
                        if code_delimiter == 0 {
                            code_delimiter = backticks;
                        } else if code_delimiter == backticks {
                            code_delimiter = 0;
                        }
                    }
                    backticks = 0;
                }
                // Table parsing removes one slash before a pipe even in code.
                // Ordinary text may already have its pipe escaped.
                if ch == '|' && (code_delimiter > 0 || slashes % 2 == 0) {
                    output.push('\\');
                }
                if ch == '\\' {
                    slashes += 1;
                } else {
                    slashes = 0;
                }
                if matches!(ch, '\r' | '\n') {
                    output.push(' ');
                } else {
                    output.push(ch);
                }
            }
        }
        output.push_str(" |");
    }
    output.push('\n');
}
