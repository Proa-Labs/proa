// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Editor snapshots: disk counterparts plus original (never projected) buffers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;

use super::{content, lint_file_with_cache};
use crate::diagnostic::{Diagnostic, Severity, SourceLabel, SourceRange};

#[derive(Debug, Deserialize)]
pub struct SourceOverlay {
    pub path: PathBuf,
    pub source: String,
}

/// Analyze open files and their package-local counterparts without compiling or
/// writing buffers to disk. A snapshot replaces, rather than patches, disk text.
/// New, unsaved module files participate in discovery too.
pub fn lint_snapshot(overlays: &[SourceOverlay]) -> Result<Vec<Diagnostic>> {
    let paths: Vec<_> = overlays.iter().map(|o| snapshot_path(&o.path)).collect();
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut sources: BTreeMap<_, _> = content::snapshot_sources(
        // Nonexistent files are supplied below rather than read from disk.
        &paths
            .iter()
            .filter(|p| p.is_file())
            .cloned()
            .collect::<Vec<_>>(),
        &content::ContentOptions::default(),
    )?
    .into_iter()
    .collect();
    // An entirely new file still needs its saved counterparts. Discover using
    // the closest Cargo package (or existing parent for standalone snippets).
    for path in &paths {
        if !path.is_file() {
            if let Some(root) = path
                .parent()
                .and_then(|p| p.ancestors().find(|p| p.join("Cargo.toml").is_file()))
            {
                for (path, source) in content::snapshot_sources(
                    &[],
                    &content::ContentOptions {
                        root: Some(root.to_owned()),
                        ..Default::default()
                    },
                )? {
                    sources.insert(path, source);
                }
            }
        }
    }
    let reporting: BTreeSet<_> = paths.iter().map(|p| content::absolute(p)).collect();
    for (overlay, path) in overlays.iter().zip(&paths) {
        sources.insert(path.clone(), overlay.source.clone());
    }

    let mut diagnostics = Vec::new();
    let mut invalid_packages = BTreeSet::new();
    for (path, source) in &sources {
        // Broken Rust is normal during typing. Do not turn an undiscoverable
        // impl into a confirmed omission or a stale exception in another view.
        if super::is_test_path(path) {
            continue;
        }
        if let Err(error) = syn::parse_file(source) {
            invalid_packages.insert(package(path));
            let range = SourceRange::from_span(path, error.span());
            diagnostics.push(
                Diagnostic::new(
                    path,
                    range.line,
                    range.column,
                    Severity::Error,
                    "syntax/rust",
                    error.to_string(),
                )
                .with_label(SourceLabel::new(
                    range,
                    source,
                    "fix Rust syntax to resume content checks for this package",
                    true,
                )),
            );
        }
        if reporting.contains(path) {
            diagnostics.extend(lint_file_with_cache(path, source, false, None));
        }
    }
    let valid_sources: Vec<_> = sources
        .iter()
        .filter(|(path, _)| !invalid_packages.contains(&package(path)))
        .map(|(p, s)| (p.clone(), s.clone()))
        .collect();
    diagnostics.extend(content::lint_sources(
        &valid_sources,
        &reporting,
        false,
        &content::ContentOptions::default(),
    ));

    // Keep exact snapshot evidence for UTF-16 conversion in the editor, even
    // for ordinary lints which normally only carry a line and scalar column.
    for diagnostic in &mut diagnostics {
        diagnostic.path = content::absolute(&diagnostic.path);
        if diagnostic.labels.is_empty() {
            if let Some(source) = sources.get(&diagnostic.path) {
                let range = SourceRange {
                    path: diagnostic.path.clone(),
                    line: diagnostic.line,
                    column: diagnostic.column,
                    end_line: diagnostic.line,
                    end_column: diagnostic.column + 1,
                };
                diagnostic
                    .labels
                    .push(SourceLabel::new(range, source, &diagnostic.message, true));
            }
        }
    }
    Ok(diagnostics)
}

fn package(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.join("Cargo.toml").is_file())
        .unwrap_or_else(|| path.parent().unwrap_or(path))
        .to_owned()
}

fn snapshot_path(path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    // A new buffer has no inode yet. Canonicalize its existing ancestor so it
    // still shares a module identity with files reached through a symlink.
    for ancestor in path.ancestors() {
        if let Ok(canonical) = ancestor.canonicalize() {
            return canonical.join(path.strip_prefix(ancestor).unwrap());
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str = "use crate::product::{Product,Notice}; use proa_core::WebRenderSync; use proa_macros::html_sync; impl WebRenderSync for Product {fn render(self,cx:&mut Cx)->Result {html_sync! {{self.title}}.render(cx)}}";
    const MD: &str = "use crate::product::Product; use proa_core::MdRenderSync; use proa_macros::md_sync; impl MdRenderSync for Product {fn render_md(self,cx:&mut Cx)->Result {md_sync! {r###\"{self.title}\"###}.render_md(cx)}}";

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        for (path, source) in [
            (
                "Cargo.toml",
                "[package]\nname='live_fixture'\nversion='0.1.0'\n",
            ),
            ("src/lib.rs", "mod product;mod html;mod md;"),
            (
                "src/product.rs",
                "pub struct Product {pub title:String} pub struct Notice;",
            ),
            ("src/html.rs", HTML),
            ("src/md.rs", MD),
        ] {
            std::fs::write(root.path().join(path), source).unwrap();
        }
        root
    }

    fn overlay(root: &Path, file: &str, source: &str) -> SourceOverlay {
        SourceOverlay {
            path: root.join(file),
            source: source.to_owned(),
        }
    }

    fn coverage(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
        diagnostics
            .iter()
            .filter(|d| d.code == "render/content-coverage")
            .collect()
    }

    #[test]
    fn unsaved_html_checks_closed_markdown_and_unsaved_repair_clears_it() {
        let root = fixture();
        let html = HTML.replace("{self.title}", "{self.title} {Notice::new()}");
        let diagnostics = lint_snapshot(&[overlay(root.path(), "src/html.rs", &html)]).unwrap();
        let missing = coverage(&diagnostics);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].message.contains("Notice"));
        assert!(missing[0].labels[0].range.path.ends_with("src/md.rs"));
        assert!(missing[0].labels[1]
            .source
            .iter()
            .any(|line| line.text.contains("Notice::new()")));

        let md = MD.replace(
            "{self.title}",
            "{self.title} {crate::product::Notice::new()}",
        );
        let fixed = lint_snapshot(&[
            overlay(root.path(), "src/html.rs", &html),
            overlay(root.path(), "src/md.rs", &md),
        ])
        .unwrap();
        assert!(coverage(&fixed).is_empty());
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/html.rs")).unwrap(),
            HTML
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/md.rs")).unwrap(),
            MD
        );
        // Closing/discarding the unsaved HTML buffer restores disk content.
        assert!(
            coverage(&lint_snapshot(&[overlay(root.path(), "src/md.rs", MD)]).unwrap()).is_empty()
        );
    }

    #[test]
    fn new_unsaved_module_and_disk_counterpart_changes_participate() {
        let root = fixture();
        std::fs::remove_file(root.path().join("src/md.rs")).unwrap();
        let html = HTML.replace("{self.title}", "{self.title} {Notice::new()}");
        std::fs::write(root.path().join("src/html.rs"), &html).unwrap();
        let diagnostics = lint_snapshot(&[overlay(root.path(), "src/md.rs", MD)]).unwrap();
        assert_eq!(coverage(&diagnostics).len(), 1);
        assert!(!root.path().join("src/md.rs").exists());
        std::fs::write(root.path().join("src/html.rs"), HTML).unwrap();
        assert!(
            coverage(&lint_snapshot(&[overlay(root.path(), "src/md.rs", MD)]).unwrap()).is_empty()
        );
    }

    #[test]
    fn incomplete_rust_does_not_invent_missing_content_or_stale_omissions() {
        let root = fixture();
        let broken = "use crate::product::Product; impl";
        let diagnostics = lint_snapshot(&[overlay(root.path(), "src/md.rs", broken)]).unwrap();
        assert!(diagnostics.iter().any(|d| d.code == "syntax/rust"));
        assert!(coverage(&diagnostics).is_empty());
        assert!(!diagnostics
            .iter()
            .any(|d| d.code == "render/content-omit-stale"));
        assert!(lint_snapshot(&[]).unwrap().is_empty());
    }
}
