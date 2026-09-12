// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::markdown::escape_text;

use super::attr_value;

/// Build-time lowerer for an authored `<Files>` tree.
#[derive(Debug, Default)]
pub(crate) struct Files {
    lines: Vec<String>,
    depth: usize,
}

impl Files {
    pub(crate) fn push(&mut self, raw: &str) -> Result<(), String> {
        if raw.is_empty() {
            return Ok(());
        }
        if raw == "</Folder>" {
            self.depth = self.depth.saturating_sub(1);
            return Ok(());
        }
        if raw.starts_with("<Folder ") {
            let name =
                attr_value(raw, "name").ok_or_else(|| format!("missing Folder name: {raw}"))?;
            self.push_label(&format!("{}/", name.trim_end_matches('/')));
            self.depth += 1;
            return Ok(());
        }
        if raw.starts_with("<File ") {
            let name =
                attr_value(raw, "name").ok_or_else(|| format!("missing File name: {raw}"))?;
            self.push_label(&name);
            return Ok(());
        }
        Err(format!("unsupported <Files> child: {raw}"))
    }

    pub(crate) fn emit_markdown(self, out: &mut String) {
        out.push_str("\n```proa-filetree\n");
        for line in self.lines {
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str("```\n\n");
    }

    /// Render the intermediate `proa-filetree` fence as accessible HTML.
    // proa-perf-ok(scope): docs file trees are compiled once into trusted static HTML; labels are escaped before insertion
    pub(super) fn emit_html(code: &str, html: &mut String) {
        html.push_str("<div class=\"proa-docs-filetree\" data-proa-docs-filetree role=\"tree\">");
        for raw_line in code.lines() {
            if raw_line.trim().is_empty() {
                continue;
            }

            let depth = raw_line
                .chars()
                .take_while(|ch| *ch == ' ')
                .count()
                .saturating_div(2);
            let label = raw_line.trim();
            let is_folder = label.ends_with('/');

            let _ = write!(
                html,
                "<div class=\"proa-docs-filetree-row\" data-kind=\"{kind}\" role=\"treeitem\"",
                kind = if is_folder { "folder" } else { "file" }
            );
            if is_folder {
                html.push_str(" aria-expanded=\"true\"");
            }
            html.push('>');
            for _ in 0..depth {
                html.push_str(
                    "<span class=\"proa-docs-filetree-indent\" aria-hidden=\"true\"></span>",
                );
            }
            html.push_str(
                "<span class=\"proa-docs-filetree-icon\" aria-hidden=\"true\"></span><code>",
            );
            escape_text(label, html);
            html.push_str("</code></div>");
        }
        html.push_str("</div>");
    }

    fn push_label(&mut self, label: &str) {
        let mut line = String::new();
        for _ in 0..self.depth {
            line.push_str("  ");
        }
        line.push_str(label);
        self.lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_nested_files_to_a_file_tree_fence() {
        let mut files = Files::default();
        files.push(r#"<Folder name="src">"#).unwrap();
        files.push(r#"<File name="main.rs" />"#).unwrap();
        files.push("</Folder>").unwrap();
        let mut out = String::new();
        files.emit_markdown(&mut out);
        assert_eq!(out, "\n```proa-filetree\nsrc/\n  main.rs\n```\n\n");
    }
}
