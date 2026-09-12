// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::markdown::{escape_attr, escape_text};

use super::LanguageIcon;

/// Build-time renderer for one authored `<CodeExplorer>` container.
///
/// Markdown children are rendered by the surrounding pipeline between calls
/// to [`open_file`](Self::open_file) and [`close_file`](Self::close_file). The
/// explorer records its file names and inserts the selector chrome ahead of
/// those already-rendered panels when [`finish`](Self::finish) is called.
#[derive(Debug)]
pub(super) struct CodeExplorer {
    group: usize,
    chrome_insert_at: usize,
    files: Vec<String>,
}

// proa-perf-ok(scope): docs explorers are lowered once at build time into trusted static HTML; authored paths are escaped before insertion
impl CodeExplorer {
    /// Open the tab container and return the state that owns its files.
    pub(super) fn open(group: usize, html: &mut String) -> Self {
        html.push_str("<div class=\"proa-docs-tabs proa-docs-explorer\" data-proa-docs-explorer>");
        Self {
            group,
            chrome_insert_at: html.len(),
            files: Vec::new(),
        }
    }

    /// Open a code-file panel while remembering its authored path.
    pub(super) fn open_file(&mut self, name: String, html: &mut String) {
        if self.files.is_empty() {
            html.push_str("<div class=\"proa-docs-tabs-panels\">");
        }
        let index = self.files.len();
        CodeFile { index, name: &name }.open(html);
        self.files.push(name);
    }

    /// Close the current code-file panel.
    pub(super) fn close_file(&self, html: &mut String) {
        CodeFile::close(html);
    }

    /// Finish the explorer and return how many files it contains.
    pub(super) fn finish(self, html: &mut String) -> usize {
        if !self.files.is_empty() {
            html.push_str("</div>");
        }

        let mut chrome = String::new();
        self.render_chrome(&mut chrome);
        html.insert_str(self.chrome_insert_at, &chrome);
        html.push_str("</div>");
        self.files.len()
    }

    /// Render the radios, file tabs, and active-file breadcrumbs.
    fn render_chrome(&self, html: &mut String) {
        for index in 0..self.files.len() {
            let _ = write!(
                html,
                "<input class=\"proa-docs-tabs-radio\" type=\"radio\" name=\"proa-docs-tabs-{group}\" id=\"proa-docs-tabs-{group}-{index}\"",
                group = self.group
            );
            if index == 0 {
                html.push_str(" checked");
            }
            html.push('>');
        }

        let root = common_directory(&self.files);
        html.push_str("<div class=\"proa-docs-tabs-list\" role=\"tablist\">");
        for (index, name) in self.files.iter().enumerate() {
            let _ = write!(
                html,
                "<label class=\"proa-docs-tabs-tab proa-docs-explorer-file\" for=\"proa-docs-tabs-{group}-{index}\">",
                group = self.group
            );
            LanguageIcon::new(language_for_path(name)).emit_html(html);
            html.push_str("<span class=\"proa-docs-explorer-file-name\">");
            escape_text(relative_label(name, root), html);
            html.push_str("</span></label>");
        }
        html.push_str("</div>");
        render_paths(html, &self.files);
    }
}

/// Build-time renderer for one `<CodeFile>` panel.
struct CodeFile<'a> {
    index: usize,
    name: &'a str,
}

// proa-perf-ok(scope): code-file wrappers are fixed build-time markup; authored file names are escaped before insertion
impl CodeFile<'_> {
    fn open(self, html: &mut String) {
        let _ = write!(
            html,
            "<div class=\"proa-docs-tabs-panel\" role=\"tabpanel\" data-index=\"{index}\" aria-label=\"",
            index = self.index
        );
        escape_attr(self.name, html);
        html.push_str("\">");
    }

    fn close(html: &mut String) {
        html.push_str("</div>");
    }
}

/// The deepest directory shared by every file, or `""` when there is none.
fn common_directory(files: &[String]) -> &str {
    let Some(first) = files.first() else {
        return "";
    };
    let Some(cut) = first.rfind('/') else {
        return "";
    };
    let mut root = &first[..cut];
    for file in files.iter().skip(1) {
        let Some(cut) = file.rfind('/') else {
            return "";
        };
        root = common_path_prefix(root, &file[..cut]);
        if root.is_empty() {
            return "";
        }
    }
    root
}

/// Longest prefix of `a` and `b` ending on a path-segment boundary.
fn common_path_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let mut end = 0;
    let mut left = a.split('/');
    let mut right = b.split('/');
    while let (Some(left), Some(right)) = (left.next(), right.next()) {
        if left != right {
            break;
        }
        if end > 0 {
            end += 1;
        }
        end += left.len();
    }
    &a[..end]
}

fn relative_label<'a>(name: &'a str, root: &str) -> &'a str {
    if root.is_empty() {
        return name;
    }
    name.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(name)
}

fn language_for_path(path: &str) -> &str {
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename.eq_ignore_ascii_case("dockerfile") {
        return "dockerfile";
    }
    filename
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension)
}

// proa-perf-ok(scope): active-file breadcrumbs are fixed build-time markup; every authored path segment is escaped before insertion
fn render_paths(html: &mut String, files: &[String]) {
    if files.is_empty() {
        return;
    }

    html.push_str("<div class=\"proa-docs-explorer-paths\">");
    for file in files {
        html.push_str("<div class=\"proa-docs-explorer-path\" aria-label=\"File: ");
        escape_attr(file, html);
        html.push_str("\">");
        for (index, segment) in file.split('/').enumerate() {
            if index > 0 {
                html.push_str(
                    "<span class=\"proa-docs-explorer-path-separator\" aria-hidden=\"true\">&gt;</span>",
                );
            }
            html.push_str("<span class=\"proa-docs-explorer-path-segment\">");
            escape_text(segment, html);
            html.push_str("</span>");
        }
        html.push_str("</div>");
    }
    html.push_str("</div>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_is_spliced_ahead_of_rendered_panels() {
        let mut html = String::new();
        let mut explorer = CodeExplorer::open(0, &mut html);
        explorer.open_file("src/data/products.rs".to_string(), &mut html);
        html.push_str("PRODUCTS");
        explorer.close_file(&mut html);
        explorer.open_file("src/data/mod.rs".to_string(), &mut html);
        html.push_str("MOD");
        explorer.close_file(&mut html);
        assert_eq!(explorer.finish(&mut html), 2);

        let radio = html.find("type=\"radio\"").expect("radio");
        let list = html.find("proa-docs-tabs-list").expect("file list");
        let panels = html.find("proa-docs-tabs-panels").expect("panels");
        assert!(radio < list && radio < panels, "{html}");
        assert!(html.starts_with("<div class=\"proa-docs-tabs proa-docs-explorer\""));
        assert!(!html.contains("proa-docs-explorer-shell"));
        assert!(html.contains("data-code-icon=\"rust\""));
        assert!(html.contains("<span class=\"proa-docs-explorer-file-name\">products.rs</span>"));
        assert!(html.contains("<span class=\"proa-docs-explorer-file-name\">mod.rs</span>"));
        assert!(html.contains("class=\"proa-docs-explorer-paths\""));
        assert!(html.contains(
            "class=\"proa-docs-explorer-path\" aria-label=\"File: src/data/products.rs\""
        ));
        assert!(html.contains(">src</span>"));
        assert!(html.contains(
            ">&gt;</span><span class=\"proa-docs-explorer-path-segment\">data</span><span class=\"proa-docs-explorer-path-separator\" aria-hidden=\"true\">&gt;</span><span class=\"proa-docs-explorer-path-segment\">products.rs</span>"
        ));
        assert!(
            html.find("proa-docs-tabs-list").unwrap()
                < html.find("proa-docs-explorer-path").unwrap()
        );
        assert!(html.contains("PRODUCTS") && html.contains("MOD"));
        assert_eq!(html.matches("<div").count(), html.matches("</div>").count());
    }

    #[test]
    fn paths_are_retained_when_files_share_no_directory() {
        let mut html = String::new();
        let mut explorer = CodeExplorer::open(3, &mut html);
        explorer.open_file("src/main.rs".to_string(), &mut html);
        explorer.close_file(&mut html);
        explorer.open_file("Cargo.toml".to_string(), &mut html);
        explorer.close_file(&mut html);
        explorer.finish(&mut html);

        assert!(html.contains("aria-label=\"File: src/main.rs\""));
        assert!(html.contains("aria-label=\"File: Cargo.toml\""));
        assert!(html.contains(">src/main.rs</span></label>"));
        assert!(html.contains(">Cargo.toml</span></label>"));
        assert!(html.contains("data-code-icon=\"rust\""));
        assert!(html.contains("data-code-icon=\"toml\""));
        assert!(html.contains("name=\"proa-docs-tabs-3\""));
    }

    #[test]
    fn shared_directory_stops_on_a_segment_boundary() {
        let files = [
            "src/pages/home.rs".to_string(),
            "src/panels/side.rs".to_string(),
        ];
        assert_eq!(common_directory(&files), "src");
        assert_eq!(relative_label("src/pages/home.rs", "src"), "pages/home.rs");
    }

    #[test]
    fn language_is_inferred_from_the_file_name() {
        assert_eq!(language_for_path("src/lib.rs"), "rs");
        assert_eq!(language_for_path("Cargo.toml"), "toml");
        assert_eq!(language_for_path("Dockerfile"), "dockerfile");
        assert_eq!(language_for_path("Makefile"), "");
    }
}
