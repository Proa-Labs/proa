// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `proa chunks`: consolidation inspector for HTML and XML macro bodies.
//!
//! The macros compile a template into alternating static byte runs and
//! dynamic slots. The number, size, and placement of those runs decide how
//! much of a render is bulk `memcpy` versus per-slot work. This command estimates
//! that segmentation straight from the parsed template so authors can see what
//! breaks consolidation without reading macro expansions.
//!
//! The byte counts are estimates: compile-time attribute escaping, entity
//! normalization, and void-element handling can shift sizes by a few
//! percent. Treat the output as a map, not a ledger.

use std::path::PathBuf;

use anyhow::{Context, Result};
use proa_dsl::parse::{Attribute, Element, Node};
use serde::Serialize;

use crate::source::{display_path, find_macros, find_rust_files, MacroDialect};

#[derive(Debug, Serialize)]
pub struct MacroChunkReport {
    pub path: String,
    pub line: usize,
    pub macro_name: String,
    /// Number of consolidated static runs.
    pub runs: usize,
    /// Total bytes across static runs (estimate).
    pub static_bytes: usize,
    /// Largest single static run in bytes (estimate).
    pub largest_run: usize,
    /// Dynamic text/child slots (`{expr}` nodes).
    pub text_slots: usize,
    /// Dynamic attribute value slots (`attr={expr}`).
    pub attr_slots: usize,
}

#[derive(Default)]
struct ChunkEstimator {
    is_xml: bool,
    runs: Vec<usize>,
    current: usize,
    text_slots: usize,
    attr_slots: usize,
}

impl ChunkEstimator {
    fn push_static(&mut self, bytes: usize) {
        self.current += bytes;
    }

    fn break_run(&mut self, is_attr: bool) {
        if self.current > 0 {
            self.runs.push(self.current);
            self.current = 0;
        }
        if is_attr {
            self.attr_slots += 1;
        } else {
            self.text_slots += 1;
        }
    }

    fn finish(mut self) -> (Vec<usize>, usize, usize) {
        if self.current > 0 {
            self.runs.push(self.current);
        }
        (self.runs, self.text_slots, self.attr_slots)
    }

    fn walk_nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.walk_node(node);
        }
    }

    fn walk_node(&mut self, node: &Node) {
        match node {
            Node::Element(el) => self.walk_element(el),
            Node::Fragment(children) => self.walk_nodes(children),
            Node::Text(lit) => self.push_static(lit.value().len()),
            Node::Block(_) => self.break_run(false),
            Node::Doctype(doctype) => self.push_static(doctype.len() + 2),
            Node::XmlDeclaration(declaration) => self.push_static(declaration.to_string().len()),
        }
    }

    fn walk_element(&mut self, el: &Element) {
        let tag = el.tag_str();
        // `<tag`
        self.push_static(1 + tag.len());
        for attr in &el.attrs {
            match attr {
                Attribute::Static { name, value } => {
                    // ` name="value"`
                    self.push_static(1 + name.as_markup_str().len() + 2 + value.value().len() + 1);
                }
                Attribute::Bool { name } => {
                    self.push_static(1 + name.as_markup_str().len());
                    if self.is_xml {
                        self.push_static(3); // `=""`
                    }
                }
                Attribute::Dynamic { name, .. } => {
                    // ` name="` then the slot, then the closing quote.
                    self.push_static(1 + name.as_markup_str().len() + 2);
                    self.break_run(true);
                    self.push_static(1);
                }
            }
        }
        if el.self_closing && el.children.is_empty() {
            self.push_static(2); // `/>`
            return;
        }
        self.push_static(1); // `>`
        self.walk_nodes(&el.children);
        // Only HTML has void elements; XML vocabularies can reuse their names.
        if self.is_xml || !crate::fmt::html_void_element(&tag) {
            // `</tag>`
            self.push_static(3 + tag.len());
        }
    }
}

pub fn analyze_paths(paths: &[PathBuf]) -> Result<Vec<MacroChunkReport>> {
    let mut reports = Vec::new();
    for path in find_rust_files(paths)? {
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let display = display_path(&path);
        for inv in find_macros(&source) {
            if inv.dialect == MacroDialect::Md {
                continue;
            }
            let Ok(root) = crate::lint::parse_invocation(&source, &inv) else {
                continue;
            };
            let mut estimator = ChunkEstimator {
                is_xml: inv.dialect == MacroDialect::Xml,
                ..Default::default()
            };
            estimator.walk_nodes(&root.nodes);
            let (runs, text_slots, attr_slots) = estimator.finish();
            let static_bytes: usize = runs.iter().sum();
            let largest_run = runs.iter().copied().max().unwrap_or(0);
            reports.push(MacroChunkReport {
                path: display.display().to_string(),
                line: inv.name_line,
                macro_name: inv.name.clone(),
                runs: runs.len(),
                static_bytes,
                largest_run,
                text_slots,
                attr_slots,
            });
        }
    }
    Ok(reports)
}

pub fn print_reports(reports: &[MacroChunkReport], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(reports)?);
        return Ok(());
    }
    if reports.is_empty() {
        println!("no HTML or XML macro bodies found");
        return Ok(());
    }
    println!(
        "{:<48} {:>5} {:>12} {:>12} {:>6} {:>6}",
        "macro (file:line)", "runs", "static", "largest", "txt{}", "attr{}"
    );
    for report in reports {
        println!(
            "{:<48} {:>5} {:>11}B {:>11}B {:>6} {:>6}",
            format!("{}:{} {}!", report.path, report.line, report.macro_name),
            report.runs,
            report.static_bytes,
            report.largest_run,
            report.text_slots,
            report.attr_slots,
        );
    }
    let total_static: usize = reports.iter().map(|r| r.static_bytes).sum();
    let total_runs: usize = reports.iter().map(|r| r.runs).sum();
    println!(
        "\n{} macro bodies, {} static runs, {}B static total. Fewer, larger runs render faster: every dynamic slot is a run boundary, so hoist invariant attribute/text expressions into literals where possible.",
        reports.len(),
        total_runs,
        total_static
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::find_macros;

    #[test]
    fn xml_declaration_joins_the_opening_static_run() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("feed.rs");
        std::fs::write(
            &path,
            r#"fn render() {
                xml_sync! {
                    <?xml version="1.0" encoding="UTF-8"?>
                    <link enabled>{title}</link>
                }
            }"#,
        )
        .unwrap();
        let reports = analyze_paths(&[path]).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].macro_name, "xml_sync");
        assert_eq!(reports[0].runs, 2);
        assert_eq!(reports[0].text_slots, 1);
        assert_eq!(reports[0].attr_slots, 0);
        assert_eq!(
            reports[0].largest_run,
            r#"<?xml version="1.0" encoding="UTF-8"?><link enabled="">"#.len()
        );
    }

    #[test]
    fn estimates_runs_and_slots() {
        let source = r#"
fn render() {
    html_sync! {
        <div class="container" data-id={id}>
            "Hello "
            {name}
            <span>"!"</span>
        </div>
    }
}
"#;
        let inv = find_macros(source).into_iter().next().expect("macro");
        let root = crate::lint::parse_invocation(source, &inv).expect("parse");
        let mut estimator = ChunkEstimator::default();
        estimator.walk_nodes(&root.nodes);
        let (runs, text_slots, attr_slots) = estimator.finish();
        // Slot boundaries: data-id attr, {name} child.
        assert_eq!(attr_slots, 1);
        assert_eq!(text_slots, 1);
        // Three runs: before attr slot, between attr slot and {name}, after.
        assert_eq!(runs.len(), 3, "runs: {runs:?}");
        let static_bytes: usize = runs.iter().sum();
        assert!(static_bytes > 30, "static bytes too small: {static_bytes}");
    }
}
