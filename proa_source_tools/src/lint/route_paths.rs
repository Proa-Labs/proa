// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `route/express-path-syntax`: Express-style `:name` and `*name` segments in
//! the path literal of an `axum::Router` call. Axum 0.8 writes them `{name}`
//! and `{*name}`, and it panics on the old spelling while the router is
//! built, so a server with such a route never starts.
//!
//! Precision comes before recall. A literal is checked only when it is the
//! path argument of `route`, `route_service`, `nest` or `nest_service`, in a
//! file that names Axum, and only when that Axum is the 0.8 Proa builds on:
//! the file goes through `proa::axum`, or its package depends on `proa`.
//! Paths held in constants or built at runtime are not followed, and a file
//! that opts out of Axum's check with `without_v07_checks` is never reported.
//!
//! `proa::axum::Route::endpoint` and its sibling constructors are not
//! reported: `proa_framework_core::route_spec` rewrites both spellings to
//! braces before the path reaches Axum, so the old one is accepted there.

use super::*;
use crate::diagnostic::{SourceLabel, SourceRange, Suggestion, SuggestionApplicability};

/// `axum::Router` methods whose first argument is a route path.
const ROUTER_METHODS: [&str; 4] = ["route", "route_service", "nest", "nest_service"];

/// `path` locates the file on disk; `display` is the path diagnostics print.
pub(super) fn lint_route_paths(
    path: &Path,
    display: &Path,
    file: &File,
    source: &str,
    ignored_ranges: &[RangeInclusive<usize>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Every reportable literal contains one of these; most files have neither.
    if !(source.contains("/:") || source.contains("/*")) || source.contains("without_v07_checks") {
        return;
    }
    let mut usage = AxumUsage::default();
    usage.visit_file(file);
    if !usage.axum {
        return;
    }
    let mut calls = RouteCalls { found: Vec::new() };
    calls.visit_file(file);
    calls.found.retain(|(literal, _)| {
        !line_in_ranges(literal.span().start().line, ignored_ranges)
            && !express_segments(&literal.value()).is_empty()
    });
    if calls.found.is_empty() || !(usage.proa_axum || package_uses_proa(path)) {
        return;
    }
    // A method chain is visited outermost call first; report in source order.
    calls.found.sort_by_key(|(literal, _)| {
        let start = literal.span().start();
        (start.line, start.column)
    });
    for (literal, nested) in calls.found {
        diagnostics.push(diagnostic(display, source, &literal, nested));
    }
}

/// How a file refers to Axum and to Proa's Axum adapter.
#[derive(Default)]
struct AxumUsage {
    /// Some path or import goes through a crate or module named `axum`.
    axum: bool,
    /// Some path or import goes through `proa::axum` or `proa_framework_axum`.
    proa_axum: bool,
}

impl AxumUsage {
    fn record(&mut self, segments: &[String]) {
        self.axum |= segments
            .iter()
            .any(|segment| segment == "axum" || segment == "proa_framework_axum");
        self.proa_axum |= through_proa_axum(segments);
    }

    fn record_use(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.record_use(&path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {
                self.record(prefix);
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.record_use(tree, prefix);
                }
            }
        }
    }
}

/// `proa::axum::..` or `proa_framework_axum::..`.
fn through_proa_axum(segments: &[String]) -> bool {
    segments
        .first()
        .is_some_and(|first| first == "proa_framework_axum")
        || segments
            .windows(2)
            .any(|pair| pair[0] == "proa" && pair[1] == "axum")
}

fn path_segments(path: &SynPath) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

impl<'ast> Visit<'ast> for AxumUsage {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.record_use(&node.tree, &mut Vec::new());
    }
    fn visit_path(&mut self, node: &'ast SynPath) {
        self.record(&path_segments(node));
        visit::visit_path(self, node);
    }
}

/// Collects `(path literal, is a nest prefix)` for every `Router` call.
struct RouteCalls {
    found: Vec<(syn::LitStr, bool)>,
}

fn string_literal(expr: &Expr) -> Option<&syn::LitStr> {
    match expr {
        Expr::Lit(literal) => match &literal.lit {
            syn::Lit::Str(text) => Some(text),
            _ => None,
        },
        Expr::Paren(inner) => string_literal(&inner.expr),
        Expr::Group(inner) => string_literal(&inner.expr),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for RouteCalls {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method = node.method.to_string();
        if ROUTER_METHODS.contains(&method.as_str()) && node.args.len() == 2 {
            if let Some(literal) = string_literal(&node.args[0]) {
                self.found
                    .push((literal.clone(), method.starts_with("nest")));
            }
        }
        visit::visit_expr_method_call(self, node);
    }
}

/// The `/`-separated segments Axum 0.8 rejects, as `(byte offset, text)`.
/// This mirrors `validate_v07_paths` in `axum::routing::path_router`.
fn express_segments(path: &str) -> Vec<(usize, &str)> {
    if !path.starts_with('/') {
        return Vec::new();
    }
    let mut offset = 0;
    let mut found = Vec::new();
    for segment in path.split('/') {
        if segment.starts_with(':') || segment.starts_with('*') {
            found.push((offset, segment));
        }
        offset += segment.len() + 1;
    }
    found
}

/// The path rewritten for Axum 0.8, and whether every capture kept a name.
/// A wildcard is dropped from a nest prefix, which may not contain one.
/// `None` when nothing remains to mount at.
fn corrected_path(path: &str, nested: bool) -> Option<(String, bool)> {
    let mut named = true;
    let mut segments = Vec::new();
    for segment in path.split('/') {
        if let Some(name) = segment.strip_prefix(':') {
            named &= !name.is_empty();
            segments.push(format!(
                "{{{}}}",
                if name.is_empty() { "name" } else { name }
            ));
        } else if let Some(name) = segment.strip_prefix('*') {
            if nested {
                continue;
            }
            named &= !name.is_empty();
            segments.push(format!(
                "{{*{}}}",
                if name.is_empty() { "rest" } else { name }
            ));
        } else {
            segments.push(segment.to_owned());
        }
    }
    let corrected = segments.join("/");
    (corrected.len() > 1 || !nested).then_some((corrected, named))
}

fn diagnostic(path: &Path, source: &str, literal: &syn::LitStr, nested: bool) -> Diagnostic {
    let value = literal.value();
    let (offset, segment) = express_segments(&value)[0];
    let wildcard = segment.starts_with('*');
    let sigil = if wildcard { '*' } else { ':' };
    let name = &segment[1..];

    // Underline the segment when the literal's text is its value verbatim;
    // a raw or escaped literal is underlined whole.
    let whole = SourceRange::from_span(path, literal.span());
    let verbatim = literal
        .span()
        .source_text()
        .is_some_and(|text| text == format!("\"{value}\""));
    let range = if verbatim {
        let column = whole.column + 1 + value[..offset].chars().count();
        SourceRange {
            column,
            end_column: column + segment.chars().count(),
            ..whole.clone()
        }
    } else {
        whole.clone()
    };

    let (message, label) = if wildcard && nested {
        (
            format!("nest prefix `{value}` ends in the Express-style wildcard `{segment}`; a nest prefix cannot contain a wildcard"),
            "`nest` already matches every path under its prefix".to_owned(),
        )
    } else if wildcard {
        (
            format!("route path `{value}` uses the Express-style wildcard `{segment}`; Axum 0.8 writes wildcards as `{{*{name}}}`"),
            "Axum 0.8 rejects a segment that starts with `*`".to_owned(),
        )
    } else {
        (
            format!("route path `{value}` uses the Express-style capture `{segment}`; Axum 0.8 writes captures as `{{{name}}}`"),
            "Axum 0.8 rejects a segment that starts with `:`".to_owned(),
        )
    };
    let diagnostic = Diagnostic::new(
        path,
        range.line,
        range.column,
        Severity::Error,
        "route/express-path-syntax",
        message,
    )
    .with_label(SourceLabel::new(range, source, label, true))
    .with_note(format!("Axum 0.8 panics while the router is built (\"Path segments must not start with `{sigil}`\"), so the server exits at startup instead of serving requests"));
    match corrected_path(&value, nested) {
        Some((corrected, named)) => diagnostic.with_suggestion(Suggestion {
            message: if named {
                "write the path as".to_owned()
            } else {
                "name the segment and write the path as".to_owned()
            },
            range: whole,
            replacement: format!("\"{corrected}\""),
            applicability: if named {
                SuggestionApplicability::MachineApplicable
            } else {
                SuggestionApplicability::MaybeIncorrect
            },
        }),
        None => diagnostic.with_help(
            "a router cannot be nested at `/`: `merge` the router, or mount the service with `fallback_service`",
        ),
    }
}

/// Whether the file's package builds on Proa, and therefore on Axum 0.8.
fn package_uses_proa(path: &Path) -> bool {
    let absolute = content::absolute(path);
    let Some(root) = absolute.parent().and_then(asset_urls::package_scope) else {
        return false;
    };
    root.join("proa.config.rs").is_file()
        || std::fs::read_to_string(root.join("Cargo.toml"))
            .is_ok_and(|manifest| manifest_depends_on_proa(&manifest))
}

/// A `proa` or `proa_framework_axum` key in one of the package's dependency
/// tables, inline or as its own table. `[workspace.dependencies]` only
/// declares versions for members, so it proves nothing about this package.
fn manifest_depends_on_proa(manifest: &str) -> bool {
    const CRATES: [&str; 2] = ["proa", "proa_framework_axum"];
    let dependency_table =
        |table: &str| table.ends_with("dependencies") && !table.starts_with("workspace");
    let mut in_dependencies = false;
    for line in manifest.lines().map(str::trim) {
        if let Some(table) = line
            .strip_prefix('[')
            .and_then(|table| table.strip_suffix(']'))
        {
            in_dependencies = dependency_table(table);
            if CRATES.iter().any(|name| {
                table
                    .strip_suffix(name)
                    .and_then(|table| table.strip_suffix('.'))
                    .is_some_and(dependency_table)
            }) {
                return true;
            }
        } else if in_dependencies
            && CRATES.iter().any(|name| {
                line.strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('.') || rest.trim_start().starts_with('='))
            })
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn express_segments_match_axum_validation() {
        assert_eq!(express_segments("/issues/:id"), [(8, ":id")]);
        assert_eq!(
            express_segments("/orgs/:org/files/*path"),
            [(6, ":org"), (17, "*path")]
        );
        for accepted in [
            "/",
            "/issues/{id}",
            "/files/{*path}",
            "/a:b/c*d",
            "/time/12:30",
            "issues/:id",
            "",
        ] {
            assert!(express_segments(accepted).is_empty(), "{accepted}");
        }
    }

    #[test]
    fn corrected_paths_use_braces_and_drop_nest_wildcards() {
        let fixed = |path, nested| corrected_path(path, nested);
        assert_eq!(
            fixed("/issues/:id", false),
            Some(("/issues/{id}".into(), true))
        );
        assert_eq!(
            fixed("/orgs/:org/files/*path", false),
            Some(("/orgs/{org}/files/{*path}".into(), true))
        );
        assert_eq!(
            fixed("/files/*", false),
            Some(("/files/{*rest}".into(), false))
        );
        assert_eq!(
            fixed("/api/:version", true),
            Some(("/api/{version}".into(), true))
        );
        assert_eq!(fixed("/static/*path", true), Some(("/static".into(), true)));
        assert_eq!(fixed("/*path", true), None);
    }

    #[test]
    fn manifests_are_recognized_by_a_proa_dependency_key() {
        for manifest in [
            "[dependencies]\nproa = \"0.4\"",
            "[dependencies]\nproa = { version = \"0.4.2\", features = [\"axum\"] }",
            "[dependencies]\nproa.workspace = true",
            "[dependencies.proa]\nversion = \"0.4\"",
            "[target.'cfg(unix)'.dependencies.proa_framework_axum]\npath = \"..\"",
            "[dev-dependencies]\n  proa_framework_axum = { workspace = true }",
        ] {
            assert!(manifest_depends_on_proa(manifest), "{manifest}");
        }
        for manifest in [
            "[package]\nname = \"proa\"",
            "[dependencies]\nproa_dsl = \"0.2\"\naxum = \"0.7\"",
            "[dependencies]\nproa-cli = \"0.2\"",
            "[dependencies.proa_core]\nversion = \"0.4\"",
            "[workspace.dependencies]\nproa = { path = \"proa\" }",
            "[workspace.dependencies.proa]\npath = \"proa\"",
            "[features]\nproa = []",
        ] {
            assert!(!manifest_depends_on_proa(manifest), "{manifest}");
        }
    }
}
