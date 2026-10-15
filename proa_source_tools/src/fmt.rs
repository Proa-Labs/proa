// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proa_dsl::parse::{Attribute, Element, HtmlRoot, Node};
use quote::ToTokens;
use syn::spanned::Spanned;
use syn::{Attribute as SynAttribute, Item, Path as SynPath};

use crate::diagnostic::{Diagnostic, Severity};
use crate::lint::parse_invocation;
use crate::source::{
    display_path, find_macros, find_rust_files, MacroDialect, MacroInvocation, SourceMap,
};

const INDENT_WIDTH: usize = 4;
const INLINE_WIDTH: usize = 100;

#[derive(Debug, Default)]
pub struct FormatSummary {
    pub changed: Vec<PathBuf>,
    pub diagnostics: Vec<Diagnostic>,
    /// Unified diffs for changed files, populated in check mode.
    pub diffs: Vec<(PathBuf, String)>,
}

pub fn format_paths_with_options(
    paths: &[PathBuf],
    check: bool,
    verify: bool,
) -> Result<FormatSummary> {
    let mut summary = FormatSummary::default();
    let mut pending_writes = Vec::new();
    for path in find_rust_files(paths)? {
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut formatted = format_file(&path, &source)?;
        if verify {
            formatted.diagnostics.extend(verify_render_preserving(
                &path,
                &source,
                &formatted.output,
            ));
        }
        let has_errors = formatted
            .diagnostics
            .iter()
            .any(|diag| diag.severity == Severity::Error);

        if formatted.output != source && !has_errors {
            summary.changed.push(display_path(&path));
            if check {
                summary.diagnostics.push(
                    Diagnostic::new(
                        display_path(&path),
                        1,
                        1,
                        Severity::Warning,
                        "format",
                        "Proa macro formatting differs",
                    )
                    .with_help("run `cargo proa fmt` to rewrite Proa macro bodies"),
                );
                summary.diffs.push((
                    display_path(&path),
                    unified_diff(&source, &formatted.output),
                ));
            } else {
                pending_writes.push((path, formatted.output));
            }
        }

        summary.diagnostics.extend(formatted.diagnostics);
    }

    let has_errors = summary
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    if !check && !has_errors {
        for (path, output) in pending_writes {
            std::fs::write(&path, output)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
    }

    Ok(summary)
}

fn unified_diff(original: &str, formatted: &str) -> String {
    similar::TextDiff::from_lines(original, formatted)
        .unified_diff()
        .context_radius(2)
        .to_string()
}

pub fn format_file(path: &Path, source: &str) -> Result<FormattedFile> {
    let display = display_path(path);
    let invocations = find_macros(source);
    let mut output = source.to_owned();
    let mut diagnostics = Vec::new();
    let rsjs_ranges = rsjs_line_ranges(source);

    for inv in invocations.iter().rev() {
        if inv.dialect == MacroDialect::Md {
            let body = inv.body(source);
            let formatted = format_md_body(inv, body);
            output.replace_range(inv.body_start..inv.body_end, &formatted);
            continue;
        }
        match parse_invocation(source, inv) {
            Ok(root) => {
                let body = inv.body(source);
                let preserve_legacy_names = line_in_ranges(inv.name_line, &rsjs_ranges);
                let formatted = format_macro_body(inv, body, &root, preserve_legacy_names);
                output.replace_range(inv.body_start..inv.body_end, &formatted);
            }
            Err(err) => diagnostics.push(Diagnostic::new(
                &display,
                inv.name_line,
                inv.name_column + 1,
                Severity::Error,
                "parse",
                format!("failed to parse {}! macro body: {err}", inv.name),
            )),
        }
    }

    Ok(FormattedFile {
        output,
        diagnostics,
    })
}

/// Verify formatting is render-preserving: every macro body in the
/// formatted output must parse to a structurally identical template tree.
/// Differences mean the formatter changed semantics, not just layout, and
/// the write is refused.
pub fn verify_render_preserving(path: &Path, original: &str, formatted: &str) -> Vec<Diagnostic> {
    let display = display_path(path);
    let mut diagnostics = Vec::new();
    let before = find_macros(original);
    let after = find_macros(formatted);
    let before_rsjs_ranges = rsjs_line_ranges(original);
    let after_rsjs_ranges = rsjs_line_ranges(formatted);
    if before.len() != after.len() {
        diagnostics.push(Diagnostic::new(
            &display,
            1,
            1,
            Severity::Error,
            "format/verify",
            format!(
                "formatting changed the number of Proa macros ({} -> {})",
                before.len(),
                after.len()
            ),
        ));
        return diagnostics;
    }
    for (inv_before, inv_after) in before.iter().zip(after.iter()) {
        if inv_before.dialect == MacroDialect::Md || inv_after.dialect == MacroDialect::Md {
            // A Markdown template has no node tree; compare the text it lowers
            // to instead, which is the whole of its rendered output.
            let text_before = md_render_text(original, inv_before);
            let text_after = md_render_text(formatted, inv_after);
            if text_before != text_after {
                diagnostics.push(
                    Diagnostic::new(
                        &display,
                        inv_before.name_line,
                        inv_before.name_column + 1,
                        Severity::Error,
                        "format/verify",
                        "formatting would change rendered Markdown".to_owned(),
                    )
                    .with_help("this is a formatter bug; the file was not rewritten"),
                );
            }
            continue;
        }
        let Ok(root_before) = parse_invocation(original, inv_before) else {
            // Original parse errors are reported by the formatter itself.
            continue;
        };
        let root_after = match parse_invocation(formatted, inv_after) {
            Ok(root) => root,
            Err(err) => {
                // The original parsed but the formatter's output does not:
                // that is corrupted output and must block the write, not be
                // skipped like a pre-existing parse error.
                diagnostics.push(
                    Diagnostic::new(
                        &display,
                        inv_before.name_line,
                        inv_before.name_column + 1,
                        Severity::Error,
                        "format/verify",
                        format!("formatted output no longer parses: {err}"),
                    )
                    .with_help("this is a formatter bug; the file was not rewritten"),
                );
                continue;
            }
        };
        let allow_legacy_names = line_in_ranges(inv_before.name_line, &before_rsjs_ranges)
            || line_in_ranges(inv_after.name_line, &after_rsjs_ranges);
        if let Err(why) = nodes_equal(&root_before.nodes, &root_after.nodes, allow_legacy_names) {
            diagnostics.push(
                Diagnostic::new(
                    &display,
                    inv_before.name_line,
                    inv_before.name_column + 1,
                    Severity::Error,
                    "format/verify",
                    format!("formatting would change rendered output: {why}"),
                )
                .with_help("this is a formatter bug; the file was not rewritten"),
            );
        }
    }
    diagnostics
}

pub(crate) fn rsjs_line_ranges(source: &str) -> Vec<RangeInclusive<usize>> {
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };

    let mut ranges = Vec::new();
    collect_rsjs_line_ranges(&file.items, false, &mut ranges);
    ranges.sort_by_key(|range| *range.start());
    ranges
}

fn collect_rsjs_line_ranges(
    items: &[Item],
    in_client_module: bool,
    ranges: &mut Vec<RangeInclusive<usize>>,
) {
    for item in items {
        match item {
            Item::Impl(item_impl)
                if in_client_module || attrs_have_any_last_ident(&item_impl.attrs, &["rsjs"]) =>
            {
                push_span_line_range(item_impl.span(), ranges);
            }
            Item::Fn(item_fn)
                if in_client_module
                    || attrs_have_any_last_ident(&item_fn.attrs, &["rsjs", "client"]) =>
            {
                push_span_line_range(item_fn.span(), ranges);
            }
            Item::Mod(item_mod) => {
                let is_client_module = in_client_module
                    || attrs_have_any_last_ident(
                        &item_mod.attrs,
                        &["rsjs", "client", "client_module"],
                    )
                    || attrs_have_path_suffix(&item_mod.attrs, &["rsjs", "client_module"]);
                if is_client_module {
                    push_span_line_range(item_mod.span(), ranges);
                }
                if let Some((_, items)) = &item_mod.content {
                    collect_rsjs_line_ranges(items, is_client_module, ranges);
                }
            }
            _ => {}
        }
    }
}

fn push_span_line_range(span: proc_macro2::Span, ranges: &mut Vec<RangeInclusive<usize>>) {
    let start = span.start().line;
    let end = span.end().line;
    if start != 0 && end >= start {
        ranges.push(start..=end);
    }
}

fn attrs_have_any_last_ident(attrs: &[SynAttribute], names: &[&str]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| names.iter().any(|name| segment.ident == *name))
    })
}

fn attrs_have_path_suffix(attrs: &[SynAttribute], suffix: &[&str]) -> bool {
    attrs.iter().any(|attr| path_ends(attr.path(), suffix))
}

fn path_ends(path: &SynPath, suffix: &[&str]) -> bool {
    path.segments.len() >= suffix.len()
        && path
            .segments
            .iter()
            .skip(path.segments.len() - suffix.len())
            .zip(suffix)
            .all(|(seg, expected)| seg.ident == *expected)
}

fn line_in_ranges(line: usize, ranges: &[RangeInclusive<usize>]) -> bool {
    ranges.iter().any(|range| range.contains(&line))
}

pub(crate) fn html_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
            | "path"
            | "circle"
            | "ellipse"
            | "line"
            | "polygon"
            | "polyline"
            | "rect"
            | "stop"
            | "use"
            | "image"
            | "animate"
            | "animateMotion"
            | "animateTransform"
            | "set"
    )
}

fn nodes_equal(
    a: &[Node],
    b: &[Node],
    allow_legacy_names: bool,
) -> std::result::Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("child count changed ({} -> {})", a.len(), b.len()));
    }
    for (left, right) in a.iter().zip(b.iter()) {
        node_equal(left, right, allow_legacy_names)?;
    }
    Ok(())
}

fn node_equal(a: &Node, b: &Node, allow_legacy_names: bool) -> std::result::Result<(), String> {
    match (a, b) {
        (Node::Element(ea), Node::Element(eb)) => element_equal(ea, eb, allow_legacy_names),
        (Node::Fragment(ca), Node::Fragment(cb)) => nodes_equal(ca, cb, allow_legacy_names),
        (Node::Text(ta), Node::Text(tb)) => {
            if ta.value() == tb.value() {
                Ok(())
            } else {
                Err(format!(
                    "text changed ({:?} -> {:?})",
                    ta.value(),
                    tb.value()
                ))
            }
        }
        (Node::Block(xa), Node::Block(xb)) => {
            let sa = normalize_expr_for_compare(&xa.to_token_stream().to_string());
            let sb = normalize_expr_for_compare(&xb.to_token_stream().to_string());
            if sa == sb {
                Ok(())
            } else {
                Err(format!("expression changed ({sa} -> {sb})"))
            }
        }
        (Node::Doctype(da), Node::Doctype(db)) => {
            if da == db {
                Ok(())
            } else {
                Err("doctype changed".to_string())
            }
        }
        _ => Err("node kind changed".to_string()),
    }
}

fn normalize_expr_for_compare(expr: &str) -> String {
    let normalized = format_embedded_macro_bodies(expr, false);
    let tokens = syn::parse_str::<syn::Expr>(&normalized)
        .map(|expr| expr.to_token_stream().to_string())
        .unwrap_or(normalized);
    strip_trailing_separators(&tokens)
}

/// Drop separators that sit immediately before a closing delimiter.
///
/// Breaking a struct literal across lines adds a trailing comma after the last
/// field, exactly as `cargo fmt` does. That is punctuation, not meaning, but
/// `to_token_stream` keeps it — so without this the render-preserving check
/// reports `{ … id : None }` and `{ … id : None , }` as different expressions
/// and refuses its own formatter's output.
fn strip_trailing_separators(tokens: &str) -> String {
    let mut out = String::with_capacity(tokens.len());
    let mut pending_separator = false;

    for token in tokens.split_inclusive(' ') {
        let trimmed = token.trim_end();
        if trimmed == "," {
            // Hold it: emit only once we know a value follows.
            pending_separator = true;
            continue;
        }
        if pending_separator {
            pending_separator = false;
            if !trimmed.starts_with(['}', ')', ']']) {
                out.push_str(", ");
            }
        }
        out.push_str(token);
    }
    if pending_separator {
        out.push_str(", ");
    }
    out
}

fn element_equal(
    a: &Element,
    b: &Element,
    allow_legacy_names: bool,
) -> std::result::Result<(), String> {
    let tag_a = a.tag_str();
    if !markup_names_equal(&a.name, &b.name, allow_legacy_names) {
        return Err(format!("tag changed ({} -> {})", tag_a, b.tag_str()));
    }
    // Self-closing form only matters for non-void elements in the HTML
    // dialect, where `<div/>` emits an unclosed `<div>`.
    if !html_void_element(&tag_a)
        && (a.self_closing != b.self_closing)
        && (a.children.is_empty() || b.children.is_empty())
    {
        return Err(format!(
            "<{tag_a}> self-closing form changed on a non-void element"
        ));
    }
    if a.attrs.len() != b.attrs.len() {
        return Err(format!(
            "<{tag_a}> attribute count changed ({} -> {})",
            a.attrs.len(),
            b.attrs.len()
        ));
    }
    for (attr_a, attr_b) in a.attrs.iter().zip(b.attrs.iter()) {
        attr_equal(&tag_a, attr_a, attr_b, allow_legacy_names)?;
    }
    nodes_equal(&a.children, &b.children, allow_legacy_names)
}

fn attr_equal(
    tag: &str,
    a: &Attribute,
    b: &Attribute,
    allow_legacy_names: bool,
) -> std::result::Result<(), String> {
    match (a, b) {
        (
            Attribute::Static {
                name: na,
                value: va,
            },
            Attribute::Static {
                name: nb,
                value: vb,
            },
        ) => {
            if !markup_names_equal(na, nb, allow_legacy_names) {
                return Err(format!("<{tag}> attribute name changed"));
            }
            if va.value() != vb.value() {
                return Err(format!(
                    "<{tag} {}> attribute value changed",
                    na.as_markup_str()
                ));
            }
            Ok(())
        }
        (
            Attribute::Dynamic {
                name: na,
                value: va,
            },
            Attribute::Dynamic {
                name: nb,
                value: vb,
            },
        ) => {
            if !markup_names_equal(na, nb, allow_legacy_names) {
                return Err(format!("<{tag}> dynamic attribute name changed"));
            }
            let sa = va.to_token_stream().to_string();
            let sb = vb.to_token_stream().to_string();
            if sa != sb {
                return Err(format!(
                    "<{tag} {}> dynamic attribute expression changed",
                    na.as_markup_str()
                ));
            }
            Ok(())
        }
        (Attribute::Bool { name: na }, Attribute::Bool { name: nb }) => {
            if markup_names_equal(na, nb, allow_legacy_names) {
                Ok(())
            } else {
                Err(format!("<{tag}> boolean attribute changed"))
            }
        }
        _ => Err(format!("<{tag}> attribute kind changed")),
    }
}

fn markup_names_equal(
    a: &proa_dsl::parse::MarkupName,
    b: &proa_dsl::parse::MarkupName,
    allow_legacy_names: bool,
) -> bool {
    a.as_markup_str() == b.as_markup_str()
        || ((allow_legacy_names
            || a.as_markup_str().starts_with('-')
            || b.as_markup_str().starts_with('-'))
            && a.legacy_ident() == b.legacy_ident())
}

#[derive(Debug)]
pub struct FormattedFile {
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// The parts of a `md!` / `md_sync!` body that the formatter rewrites.
///
/// A Markdown template is one string literal, so there is no markup tree to
/// re-emit. Only the layout indentation inside the literal is canonicalized.
struct MdTemplate {
    /// Opening quote, including any raw-string hashes (`"`, `r#"`, `r###"`).
    open: String,
    /// Source text between the quotes.
    inner: String,
    /// Closing quote, including any raw-string hashes.
    close: String,
}

/// Recognize a body that is exactly one multi-line string literal.
///
/// Returns `None` for token-mode bodies (`md_sync! { "a " {b} }`), for
/// single-line templates (whose leading whitespace is content, not layout),
/// and for literals whose escapes change the line structure, because
/// re-indenting those would change the rendered Markdown rather than the
/// layout.
fn md_template(body: &str) -> Option<MdTemplate> {
    use std::str::FromStr;

    // The body is rebuilt from the literal token, which does not carry
    // comments. Rebuilding one that has a comment would silently discard it.
    if contains_source_comment(body) {
        return None;
    }

    let lit: syn::LitStr = syn::parse2(proc_macro2::TokenStream::from_str(body).ok()?).ok()?;
    let value = lit.value();
    if !proa_dsl::md_layout::is_multiline_template(&value) {
        return None;
    }

    let text = lit.token().to_string();
    let (open, close) = match text.strip_prefix('r') {
        Some(rest) => {
            let hashes = "#".repeat(rest.len() - rest.trim_start_matches('#').len());
            (format!("r{hashes}\""), format!("\"{hashes}"))
        }
        None => ("\"".to_owned(), "\"".to_owned()),
    };
    let inner = text.strip_prefix(&open)?.strip_suffix(&close)?.to_owned();

    // An escape that adds or removes a line break (`\n`, or a trailing `\`
    // line continuation) decouples source lines from rendered lines, so the
    // layout indentation is no longer safe to rewrite.
    if inner.split('\n').count() != value.split('\n').count() {
        return None;
    }

    Some(MdTemplate { open, inner, close })
}

/// Canonical layout for a Markdown template body.
///
/// Every template line and the closing quote are re-anchored one level beneath
/// the macro invocation. The closing macro brace returns to the invocation's
/// own column. The macro strips the indentation shared by all lines at compile
/// time (the rule lives in `proa_dsl::md_layout`, which both sides call), so
/// re-indenting a template can only move it on the page, never change what it
/// renders.
pub(crate) fn format_md_body(inv: &MacroInvocation, body: &str) -> String {
    let Some(tpl) = md_template(body) else {
        return body.to_owned();
    };

    // `inner` starts with the opening newline; everything after it is template.
    let Some(template) = tpl.inner.strip_prefix('\n') else {
        return body.to_owned();
    };
    let common = proa_dsl::md_layout::layout_indent(template);
    let close_indent = inv.indentation_column;
    let content_indent = close_indent + INDENT_WIDTH;

    let mut out = String::with_capacity(body.len());
    out.push(' ');
    out.push_str(&tpl.open);

    let lines: Vec<&str> = template.split('\n').collect();
    let last = lines.len().saturating_sub(1);
    for (idx, line) in lines.iter().enumerate() {
        out.push('\n');
        let content = &line[common_prefix_len(common, line)..];
        if idx == last && content.trim().is_empty() {
            // The line that carries the closing quote.
            push_spaces(&mut out, content_indent);
            continue;
        }
        if content.is_empty() {
            continue;
        }
        push_spaces(&mut out, content_indent);
        out.push_str(content);
    }

    out.push_str(&tpl.close);
    out.push('\n');
    push_spaces(&mut out, close_indent);
    out
}

/// Length of the longest prefix of `common` that `line` also starts with.
fn common_prefix_len(common: &str, line: &str) -> usize {
    common
        .char_indices()
        .zip(line.chars())
        .take_while(|((_, a), b)| a == b)
        .map(|((idx, a), _)| idx + a.len_utf8())
        .last()
        .unwrap_or(0)
}

/// The text a Markdown template lowers to, for render-preserving comparison.
fn md_render_text(source: &str, inv: &MacroInvocation) -> Option<String> {
    use std::str::FromStr;

    let body = inv.body(source);
    let lit: syn::LitStr = syn::parse2(proc_macro2::TokenStream::from_str(body).ok()?).ok()?;
    Some(proa_dsl::md_layout::dedent_template(&lit.value()))
}

pub(crate) fn format_macro_body(
    inv: &MacroInvocation,
    body: &str,
    root: &HtmlRoot,
    preserve_legacy_names: bool,
) -> String {
    // Comments are not represented in the parsed template tree. Rebuilding a
    // body that contains one would silently discard author context, so leave
    // the entire invocation untouched until the formatter has a comment-aware
    // representation.
    if contains_source_comment(body) {
        return body.to_owned();
    }
    if root.nodes.is_empty() {
        return String::new();
    }

    let source = SourceMap::new(body);
    if root_requires_source_fallback(&source, root) {
        return body.to_owned();
    }
    let close_indent = inv.indentation_column;
    let child_indent = close_indent + INDENT_WIDTH;
    let mut out = String::new();
    out.push('\n');

    for (idx, node) in root.nodes.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        format_node(&mut out, &source, node, child_indent, preserve_legacy_names);
    }

    out.push('\n');
    push_spaces(&mut out, close_indent);
    out
}

/// Whether a Rust interpolation differs from the formatter's canonical layout.
///
/// The general `format/macro-body` lint is informational because it also
/// covers harmless markup preferences. Rust expressions are different: a
/// stranded nested macro or an overlong component literal quickly becomes
/// unreadable, so the linter promotes those cases to a warning.
pub(crate) fn macro_body_has_noncanonical_expression_layout(
    inv: &MacroInvocation,
    body: &str,
    root: &HtmlRoot,
    preserve_legacy_names: bool,
) -> bool {
    if contains_source_comment(body) {
        return false;
    }

    let source = SourceMap::new(body);
    if root_requires_source_fallback(&source, root) {
        return false;
    }

    let child_indent = inv.indentation_column + INDENT_WIDTH;
    root.nodes.iter().any(|node| {
        node_has_noncanonical_expression_layout(&source, node, child_indent, preserve_legacy_names)
    })
}

fn node_has_noncanonical_expression_layout(
    source: &SourceMap<'_>,
    node: &Node,
    indent: usize,
    preserve_legacy_names: bool,
) -> bool {
    match node {
        Node::Element(element) => element.children.iter().any(|child| {
            node_has_noncanonical_expression_layout(
                source,
                child,
                indent + INDENT_WIDTH,
                preserve_legacy_names,
            )
        }),
        Node::Fragment(children) => children.iter().any(|child| {
            node_has_noncanonical_expression_layout(
                source,
                child,
                indent + INDENT_WIDTH,
                preserve_legacy_names,
            )
        }),
        Node::Block(expr) => {
            let Some(authored) = source.span_snippet(expr.span()).map(str::trim) else {
                return false;
            };
            if !matches!(expr, syn::Expr::Struct(_)) && !authored.contains('\n') {
                return false;
            }

            let mut canonical = String::new();
            format_block_expr(&mut canonical, source, expr, indent, preserve_legacy_names);
            canonical != authored
        }
        Node::Text(_) | Node::Doctype(_) => false,
    }
}

// proa-perf-ok(scope): the formatter emits template source at build time, outside the SSR render path
fn format_node(
    out: &mut String,
    source: &SourceMap<'_>,
    node: &Node,
    indent: usize,
    preserve_legacy_names: bool,
) {
    match node {
        Node::Element(el) => format_element(out, source, el, indent, preserve_legacy_names),
        Node::Fragment(children) => {
            format_fragment(out, source, children, indent, preserve_legacy_names)
        }
        Node::Text(lit) => {
            push_spaces(out, indent);
            out.push_str(&text_literal_source(source, lit));
        }
        Node::Block(expr) => {
            push_spaces(out, indent);
            out.push('{');
            format_block_expr(out, source, expr, indent, preserve_legacy_names);
            out.push('}');
        }
        Node::Doctype(content) => {
            push_spaces(out, indent);
            if content.is_empty() {
                out.push_str("<!DOCTYPE html>");
            } else {
                out.push_str(content);
            }
        }
    }
}

// proa-perf-ok(scope): the formatter emits template source at build time, outside the SSR render path
fn format_fragment(
    out: &mut String,
    source: &SourceMap<'_>,
    children: &[Node],
    indent: usize,
    preserve_legacy_names: bool,
) {
    if let Some(inline) = inline_children(source, children, preserve_legacy_names) {
        push_spaces(out, indent);
        let _ = write!(out, "<>{inline}</>");
        return;
    }

    push_spaces(out, indent);
    out.push_str("<>");
    for child in children {
        out.push('\n');
        format_node(
            out,
            source,
            child,
            indent + INDENT_WIDTH,
            preserve_legacy_names,
        );
    }
    out.push('\n');
    push_spaces(out, indent);
    out.push_str("</>");
}

// proa-perf-ok(scope): the formatter emits template source at build time, outside the SSR render path
fn format_element(
    out: &mut String,
    source: &SourceMap<'_>,
    el: &Element,
    indent: usize,
    preserve_legacy_names: bool,
) {
    let attrs = format_attrs(source, &el.attrs, preserve_legacy_names);
    let tag_name = format_markup_name_source(source, &el.name, preserve_legacy_names);
    let multiline_attrs = el.attrs.len() > 1
        && (el.name.start_line_column().line < el.opening_end_line_column().line
            || indent + tag_name.len() + attrs.len() + 2 > INLINE_WIDTH);

    if multiline_attrs {
        format_element_with_multiline_attrs(
            out,
            source,
            el,
            indent,
            tag_name.as_ref(),
            preserve_legacy_names,
        );
        return;
    }

    let open = format!("<{}{}>", tag_name, attrs);

    if el.self_closing {
        push_spaces(out, indent);
        out.push('<');
        let _ = write!(out, "{tag_name}{attrs} />");
        return;
    }

    if el.children.is_empty() {
        push_spaces(out, indent);
        let _ = write!(out, "{open}</{tag_name}");
        out.push('>');
        return;
    }

    if let Some(inline) = inline_children(source, &el.children, preserve_legacy_names) {
        let line_len = indent + open.len() + inline.len() + tag_name.len() + 3;
        if line_len <= INLINE_WIDTH {
            push_spaces(out, indent);
            let _ = write!(out, "{open}{inline}</{tag_name}");
            out.push('>');
            return;
        }
    }

    push_spaces(out, indent);
    out.push_str(&open);
    for child in &el.children {
        out.push('\n');
        format_node(
            out,
            source,
            child,
            indent + INDENT_WIDTH,
            preserve_legacy_names,
        );
    }
    out.push('\n');
    push_spaces(out, indent);
    let _ = write!(out, "</{tag_name}");
    out.push('>');
}

// proa-perf-ok(scope): the formatter emits template source at build time, outside the SSR render path
fn format_element_with_multiline_attrs(
    out: &mut String,
    source: &SourceMap<'_>,
    el: &Element,
    indent: usize,
    tag_name: &str,
    preserve_legacy_names: bool,
) {
    push_spaces(out, indent);
    out.push('<');
    out.push_str(tag_name);
    for attr in &el.attrs {
        out.push('\n');
        push_spaces(out, indent + INDENT_WIDTH);
        out.push_str(&format_attr(source, attr, preserve_legacy_names));
    }
    out.push('\n');
    push_spaces(out, indent);

    if el.self_closing {
        out.push_str("/>");
        return;
    }

    out.push('>');
    if el.children.is_empty() {
        let _ = write!(out, "</{tag_name}");
        out.push('>');
        return;
    }

    for child in &el.children {
        out.push('\n');
        format_node(
            out,
            source,
            child,
            indent + INDENT_WIDTH,
            preserve_legacy_names,
        );
    }
    out.push('\n');
    push_spaces(out, indent);
    let _ = write!(out, "</{tag_name}");
    out.push('>');
}

fn inline_children(
    source: &SourceMap<'_>,
    children: &[Node],
    preserve_legacy_names: bool,
) -> Option<String> {
    let mut out = String::new();
    for child in children {
        match child {
            Node::Text(lit) => out.push_str(&text_literal_source(source, lit)),
            Node::Block(expr) => {
                let expr = expr_source(source, expr, preserve_legacy_names);
                if expr.contains('\n') {
                    return None;
                }
                out.push('{');
                out.push_str(expr.trim());
                out.push('}');
            }
            Node::Element(_) | Node::Fragment(_) | Node::Doctype(_) => return None,
        }
    }
    Some(out)
}

fn format_attrs(
    source: &SourceMap<'_>,
    attrs: &[Attribute],
    preserve_legacy_names: bool,
) -> String {
    let mut out = String::new();
    for attr in attrs {
        out.push(' ');
        out.push_str(&format_attr(source, attr, preserve_legacy_names));
    }
    out
}

fn format_attr(source: &SourceMap<'_>, attr: &Attribute, preserve_legacy_names: bool) -> String {
    let mut out = String::new();
    match attr {
        Attribute::Static { name, value } => {
            out.push_str(format_markup_name_source(source, name, preserve_legacy_names).as_ref());
            out.push('=');
            out.push_str(&text_literal_source(source, value));
        }
        Attribute::Dynamic { name, value } => {
            let _ = write!(
                out,
                "{name}={{{value}",
                name = format_markup_name_source(source, name, preserve_legacy_names),
                value = expr_source(source, value, preserve_legacy_names).trim()
            );
            out.push('}');
        }
        Attribute::Bool { name } => {
            out.push_str(format_markup_name_source(source, name, preserve_legacy_names).as_ref());
        }
    }
    out
}

fn format_markup_name_source<'a>(
    source: &'a SourceMap<'_>,
    name: &proa_dsl::parse::MarkupName,
    preserve_legacy_names: bool,
) -> std::borrow::Cow<'a, str> {
    if let Some(candidate) = source_markup_name(source, name) {
        return std::borrow::Cow::Borrowed(candidate);
    }

    let raw = name.as_markup_str();
    if preserve_legacy_names || raw.starts_with('-') {
        std::borrow::Cow::Owned(name.legacy_ident().to_string())
    } else {
        std::borrow::Cow::Owned(raw.to_owned())
    }
}

fn source_markup_name<'a>(
    source: &'a SourceMap<'_>,
    name: &proa_dsl::parse::MarkupName,
) -> Option<&'a str> {
    let tail = source.source_from_span_start(name.span())?;
    let end = tail
        .char_indices()
        .find_map(|(idx, ch)| (!markup_name_source_char(ch)).then_some(idx))
        .unwrap_or(tail.len());
    let candidate = tail.get(..end)?;
    let legacy = name.legacy_ident().to_string();
    (candidate == name.as_markup_str() || candidate == legacy).then_some(candidate)
}

fn markup_name_source_char(ch: char) -> bool {
    ch == '_' || ch == '-' || ch == ':' || ch == '#' || ch.is_alphanumeric()
}

fn text_literal_source(source: &SourceMap<'_>, lit: &syn::LitStr) -> String {
    source
        .span_snippet(lit.span())
        .map(str::trim)
        .map(str::to_owned)
        .unwrap_or_else(|| lit.to_token_stream().to_string())
}

fn expr_source(source: &SourceMap<'_>, expr: &syn::Expr, preserve_legacy_names: bool) -> String {
    let Some(snippet) = source.span_snippet(expr.span()).map(str::to_owned) else {
        return format_embedded_macro_bodies(&compact_expr_source(expr), preserve_legacy_names);
    };
    if snippet.contains('\n') && contains_layout_sensitive_expr(expr) {
        // Hand back the interior unchanged: rewriting a nested macro here would
        // move its indentation baseline out from under the enclosing snippet.
        // The caller re-anchors the whole block with `reindent_block_expr`,
        // which shifts it as a unit and so preserves the relative shape the
        // author wrote.
        return snippet;
    }
    let source = if snippet.contains('\n') && should_compact_multiline_expr(expr) {
        compact_expr_source(expr)
    } else {
        snippet
    };
    format_embedded_macro_bodies(&source, preserve_legacy_names)
}

fn format_embedded_macro_bodies(source: &str, preserve_legacy_names: bool) -> String {
    let invocations = find_macros(source);
    if invocations.is_empty() {
        return source.to_owned();
    }

    let mut output = source.to_owned();
    for inv in invocations.iter().rev() {
        let body = inv.body(source);
        let formatted = if inv.dialect == MacroDialect::Md {
            format_md_body(inv, body)
        } else {
            let Ok(root) = parse_invocation(source, inv) else {
                continue;
            };
            format_macro_body(inv, body, &root, preserve_legacy_names)
        };
        output.replace_range(inv.body_start..inv.body_end, &formatted);
        // `find_macros` has already established that this identifier is
        // followed by a macro bang and a brace delimiter, outside strings and
        // comments. Canonicalize that exact token boundary instead of looking
        // for the lexical substring `" !"` in the whole expression (which
        // also matches unary `!` and string contents).
        let name_end = inv.name_start + inv.name.len();
        output.replace_range(name_end..inv.body_start, "! {");
    }
    output
}

/// Format macros nested in a Rust field value using the field's indentation as
/// their line baseline.
///
/// A value such as `children: Some(html_sync! { ... })` begins after the field
/// name, so the snippet recovered from `syn` has no leading whitespace on its
/// first line. Formatting it directly makes the nested macro believe it lives
/// at column zero. Prefixing only for the formatting pass gives the macro the
/// same baseline rustfmt uses for a field value; removing that first-line
/// prefix afterwards leaves continuation lines correctly anchored.
fn format_embedded_macro_bodies_at_indent(
    source: &str,
    indent: usize,
    preserve_legacy_names: bool,
) -> String {
    if find_macros(source).is_empty() {
        return source.to_owned();
    }

    let prefix = " ".repeat(indent);
    let mut anchored = String::with_capacity(prefix.len() + source.len());
    let _ = write!(anchored, "{prefix}{source}");
    let formatted = format_embedded_macro_bodies(&anchored, preserve_legacy_names);
    formatted
        .strip_prefix(&prefix)
        .unwrap_or(&formatted)
        .to_owned()
}

fn compact_expr_source(expr: &syn::Expr) -> String {
    compact_token_spacing(&expr.to_token_stream().to_string())
}

fn should_compact_multiline_expr(expr: &syn::Expr) -> bool {
    let simple_shape = matches!(
        expr,
        syn::Expr::Array(_)
            | syn::Expr::Await(_)
            | syn::Expr::Cast(_)
            | syn::Expr::Field(_)
            | syn::Expr::Index(_)
            | syn::Expr::Lit(_)
            | syn::Expr::MethodCall(_)
            | syn::Expr::Paren(_)
            | syn::Expr::Path(_)
            | syn::Expr::Reference(_)
            | syn::Expr::Try(_)
            | syn::Expr::Tuple(_)
            | syn::Expr::Unary(_)
    );
    simple_shape
        && !contains_layout_sensitive_expr(expr)
        && compact_expr_source(expr).len() <= INLINE_WIDTH
}

fn contains_layout_sensitive_expr(expr: &syn::Expr) -> bool {
    use syn::visit::Visit;

    #[derive(Default)]
    struct Detector {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Detector {
        fn visit_expr_closure(&mut self, _node: &'ast syn::ExprClosure) {
            self.found = true;
        }

        fn visit_expr_for_loop(&mut self, _node: &'ast syn::ExprForLoop) {
            self.found = true;
        }

        fn visit_expr_if(&mut self, _node: &'ast syn::ExprIf) {
            self.found = true;
        }

        fn visit_expr_macro(&mut self, _node: &'ast syn::ExprMacro) {
            self.found = true;
        }

        fn visit_expr_match(&mut self, _node: &'ast syn::ExprMatch) {
            self.found = true;
        }

        fn visit_expr_struct(&mut self, _node: &'ast syn::ExprStruct) {
            self.found = true;
        }

        fn visit_expr_while(&mut self, _node: &'ast syn::ExprWhile) {
            self.found = true;
        }
    }

    let mut detector = Detector::default();
    detector.visit_expr(expr);
    detector.found
}

fn root_requires_source_fallback(source: &SourceMap<'_>, root: &HtmlRoot) -> bool {
    root.nodes
        .iter()
        .any(|node| node_requires_source_fallback(source, node))
}

fn node_requires_source_fallback(source: &SourceMap<'_>, node: &Node) -> bool {
    match node {
        Node::Element(element) => {
            element.attrs.iter().any(|attr| {
                matches!(
                    attr,
                    Attribute::Dynamic { value, .. }
                        if expr_requires_source_fallback(source, value)
                )
            }) || element
                .children
                .iter()
                .any(|child| node_requires_source_fallback(source, child))
        }
        Node::Fragment(children) => children
            .iter()
            .any(|child| node_requires_source_fallback(source, child)),
        Node::Block(expr) => expr_requires_source_fallback(source, expr),
        Node::Text(_) | Node::Doctype(_) => false,
    }
}

/// Whether a multi-line interpolation forces the whole macro body to be
/// emitted byte-for-byte.
///
/// This used to be true for every multi-line layout-sensitive expression, which
/// meant a single nested `html_sync!` or struct literal froze its entire
/// enclosing body — so nothing in the file was ever re-anchored, and stranded
/// interpolations drifted further right with each surrounding edit.
/// [`reindent_block_expr`] now shifts those blocks as a unit, so the freeze is
/// only needed where leading whitespace is not layout.
///
/// A string literal spanning a line break is exactly that case: the indentation
/// inside it is program data, and moving it would change what the code means.
/// Those keep the byte-for-byte path.
fn expr_requires_source_fallback(source: &SourceMap<'_>, expr: &syn::Expr) -> bool {
    source.span_snippet(expr.span()).is_some_and(|snippet| {
        snippet.contains('\n')
            && contains_layout_sensitive_expr(expr)
            && contains_multiline_string_literal(snippet)
    })
}

/// Whether `snippet` holds a string literal that spans a line break.
fn contains_multiline_string_literal(snippet: &str) -> bool {
    let mut idx = 0;
    while idx < snippet.len() {
        if let Some(end) = rust_string_literal_end(snippet, idx) {
            if snippet[idx..end].contains('\n') {
                return true;
            }
            idx = end;
            continue;
        }
        idx += snippet[idx..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    false
}

fn contains_source_comment(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if let Some(end) = rust_string_literal_end(source, idx) {
            idx = end;
            continue;
        }
        if bytes.get(idx) == Some(&b'/') && matches!(bytes.get(idx + 1), Some(b'/' | b'*')) {
            return true;
        }
        idx += source[idx..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    false
}

fn compact_token_spacing(tokens: &str) -> String {
    let mut out = String::with_capacity(tokens.len());
    let mut segment_start = 0;
    let mut idx = 0;
    while idx < tokens.len() {
        if let Some(end) = rust_string_literal_end(tokens, idx) {
            let _ = write!(
                out,
                "{segment}{literal}",
                segment = compact_token_spacing_segment(&tokens[segment_start..idx]),
                literal = &tokens[idx..end]
            );
            idx = end;
            segment_start = idx;
        } else {
            idx += tokens[idx..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
        }
    }
    out.push_str(&compact_token_spacing_segment(&tokens[segment_start..]));
    out.trim().to_owned()
}

fn rust_string_literal_end(tokens: &str, start: usize) -> Option<usize> {
    normal_string_literal_end(tokens, start).or_else(|| raw_string_literal_end(tokens, start))
}

fn normal_string_literal_end(tokens: &str, start: usize) -> Option<usize> {
    let bytes = tokens.as_bytes();
    let quote = match bytes.get(start) {
        Some(b'"') => start,
        Some(b'b' | b'c') if bytes.get(start + 1) == Some(&b'"') => start + 1,
        _ => return None,
    };

    let mut idx = quote + 1;
    let mut escaped = false;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\\' if !escaped => escaped = true,
            b'"' if !escaped => return Some(idx + 1),
            _ => escaped = false,
        }
        idx += 1;
    }
    None
}

fn raw_string_literal_end(tokens: &str, start: usize) -> Option<usize> {
    let bytes = tokens.as_bytes();
    let mut idx = match bytes.get(start) {
        Some(b'r') => start + 1,
        Some(b'b' | b'c') if bytes.get(start + 1) == Some(&b'r') => start + 2,
        _ => return None,
    };

    let hashes_start = idx;
    while bytes.get(idx) == Some(&b'#') {
        idx += 1;
    }
    if bytes.get(idx) != Some(&b'"') {
        return None;
    }
    let hash_count = idx - hashes_start;
    idx += 1;

    while idx < bytes.len() {
        if bytes[idx] == b'"' {
            let hashes_end = idx + 1 + hash_count;
            if hashes_end <= bytes.len()
                && bytes[idx + 1..hashes_end].iter().all(|byte| *byte == b'#')
            {
                return Some(hashes_end);
            }
        }
        idx += 1;
    }
    None
}

fn compact_token_spacing_segment(tokens: &str) -> String {
    let mut out = String::with_capacity(tokens.len());
    let mut prev_was_space = false;
    for ch in tokens.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                out.push(' ');
                prev_was_space = true;
            }
        } else {
            out.push(ch);
            prev_was_space = false;
        }
    }

    for (from, to) in [
        (" . ", "."),
        (" .", "."),
        (". ", "."),
        (" :: ", "::"),
        (" ::", "::"),
        (":: ", "::"),
        (" (", "("),
        ("[ ", "["),
        (" ]", "]"),
        (" ,", ","),
        (" :", ":"),
        (".. ", ".."),
        (" !", "!"),
        ("| |", "||"),
        (" |", "|"),
        (",..", ", .."),
    ] {
        out = out.replace(from, to);
    }
    compact_unary_bang_spacing(&out)
}

fn compact_unary_bang_spacing(tokens: &str) -> String {
    let mut out = String::with_capacity(tokens.len());
    let mut chars = tokens.chars().peekable();
    while let Some(ch) = chars.next() {
        out.push(ch);
        if ch != '!' || chars.peek() != Some(&' ') {
            continue;
        }
        let previous = out[..out.len() - ch.len_utf8()]
            .chars()
            .rev()
            .find(|candidate| !candidate.is_whitespace());
        if previous.is_none_or(|previous| {
            matches!(
                previous,
                '(' | '['
                    | '{'
                    | '='
                    | ':'
                    | ','
                    | ';'
                    | '|'
                    | '&'
                    | '+'
                    | '-'
                    | '*'
                    | '/'
                    | '%'
                    | '<'
                    | '>'
                    | '?'
            )
        }) {
            chars.next();
        }
    }
    out
}

/// Write the body of a `{ … }` interpolation.
///
/// A component struct literal that does not fit on one line is broken one field
/// per line, the way `cargo fmt` breaks a struct literal in ordinary Rust.
/// Leaving it inline is what produced the pathological columns: a single line
/// carrying several long Tailwind strings pushed any nested `html_sync!` out to
/// column 200+, and everything inside it inherited that.
fn format_block_expr(
    out: &mut String,
    source: &SourceMap<'_>,
    expr: &syn::Expr,
    indent: usize,
    preserve_legacy_names: bool,
) {
    let rendered = expr_source(source, expr, preserve_legacy_names);
    let inline = rendered.trim();

    // Indentation inside a multiline string literal is program data. Keep the
    // expression byte-for-byte while still allowing sibling template nodes to
    // receive canonical formatting.
    if contains_multiline_string_literal(inline) {
        out.push_str(inline);
        return;
    }

    if let syn::Expr::Struct(structure) = expr {
        // `+ 2` for the interpolation braces the caller writes around this.
        let fits = !inline.contains('\n') && indent + inline.len() + 2 <= INLINE_WIDTH;
        if !fits {
            if let Some(multiline) =
                format_struct_multiline(source, structure, indent, preserve_legacy_names)
            {
                out.push_str(&multiline);
                return;
            }
        }
    }

    out.push_str(&reindent_block_expr(inline, indent));
}

/// One field per line, closing brace back at `indent`.
///
/// Returns `None` if any part of the literal cannot be recovered from source,
/// in which case the caller falls back to emitting it as written.
fn format_struct_multiline(
    source: &SourceMap<'_>,
    structure: &syn::ExprStruct,
    indent: usize,
    preserve_legacy_names: bool,
) -> Option<String> {
    let path = source
        .span_snippet(structure.path.span())?
        .trim()
        .to_owned();
    let field_indent = indent + INDENT_WIDTH;

    let mut out = String::new();
    let _ = writeln!(out, "{path} {{");

    for field in &structure.fields {
        let name = source.span_snippet(field.member.span())?.trim().to_owned();
        push_spaces(&mut out, field_indent);
        if field.colon_token.is_none() {
            // Shorthand: `Foo { bar }` must not become `bar: bar`.
            out.push_str(&name);
        } else {
            let _ = write!(out, "{name}: ");
            let value_column = field_indent + name.len() + ": ".len();
            out.push_str(&format_expr_value(
                source,
                &field.expr,
                field_indent,
                value_column,
                preserve_legacy_names,
            ));
        }
        out.push_str(",\n");
    }

    if let Some(rest) = &structure.rest {
        let rest_source = source.span_snippet(rest.span())?.trim().to_owned();
        push_spaces(&mut out, field_indent);
        let _ = write!(
            out,
            "..{rest}",
            rest = reindent_block_expr(&rest_source, field_indent)
        );
        out.push('\n');
    }

    push_spaces(&mut out, indent);
    out.push('}');
    Some(out)
}

/// Format a Rust expression used as a struct-field value.
///
/// Component trees commonly wrap child structs in `Some(...)`. Looking only at
/// the outer field expression therefore misses `Some(BreadcrumbList { ... })`
/// even though the nested literal is far beyond the line-width limit. Recurse
/// through single-argument calls so every struct literal gets the same
/// rustfmt-style width decision, while leaving arbitrary multi-argument calls
/// and other Rust expressions source-preserving.
fn format_expr_value(
    source: &SourceMap<'_>,
    expr: &syn::Expr,
    indent: usize,
    value_column: usize,
    preserve_legacy_names: bool,
) -> String {
    if let syn::Expr::Struct(structure) = expr {
        let authored = expr_source(source, expr, preserve_legacy_names);
        let inline = authored.trim();
        let fits = !inline.contains('\n') && value_column + inline.len() <= INLINE_WIDTH;
        if !fits {
            if let Some(multiline) =
                format_struct_multiline(source, structure, indent, preserve_legacy_names)
            {
                return multiline;
            }
        }
    }

    if let syn::Expr::Call(call) = expr {
        if let Some(argument) = (call.args.len() == 1).then(|| call.args.first()).flatten() {
            if let (Some(function), Some(authored_argument)) = (
                source.span_snippet(call.func.span()).map(str::trim),
                source.span_snippet(argument.span()).map(str::trim),
            ) {
                let formatted_argument = format_expr_value(
                    source,
                    argument,
                    indent,
                    value_column + function.len() + 1,
                    preserve_legacy_names,
                );
                if formatted_argument != authored_argument {
                    return format!("{function}({formatted_argument})");
                }
            }
        }
    }

    let value = expr_source(source, expr, preserve_legacy_names);
    let value = reindent_block_expr(value.trim(), indent);
    let value = format_embedded_macro_bodies_at_indent(&value, indent, preserve_legacy_names);
    reindent_block_expr(value.trim(), indent)
}

/// Re-anchor a multi-line interpolation to the depth it is being written at.
///
/// Only the first line of a `{ … }` interpolation is positioned by the
/// formatter; [`expr_source`] hands back the remaining lines copied verbatim
/// from the file, still carrying whatever absolute indentation they had the
/// last time anything wrote them. Emitting that at a new depth leaves the block
/// stranded at its old column, and because nothing ever corrects it, each edit
/// that moves the enclosing code strands it further out. Real output reached
/// column 223 for a node whose parent sat at 38.
///
/// Only genuinely stranded blocks are touched, and they are shifted as a unit
/// so the relative shape the author wrote survives intact.
///
/// "Stranded" means even the *shallowest* continuation line sits more than two
/// levels deeper than the interpolation itself — a gap no one types. Both
/// hand-written shapes stay within that bound and are returned byte-identical:
///
/// ```text
///     {Tabs {              // closing `}}` lands at `indent`
///         …
///     }}
///
///     {self                // continuation lands one level in
///         .show_icon.then(|| html_sync! { … })}
/// ```
///
/// A stranded block is re-anchored by its shallowest line, which in practice is
/// the one closing the expression.
fn reindent_block_expr(snippet: &str, indent: usize) -> String {
    if !snippet.contains('\n') {
        return snippet.to_owned();
    }

    let mut lines = snippet.lines();
    let Some(first) = lines.next() else {
        return snippet.to_owned();
    };
    let rest: Vec<&str> = lines.collect();

    let Some(base) = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
    else {
        return snippet.to_owned();
    };

    // The sane band: a hand-written continuation closes level with the
    // interpolation, or sits one to two levels in. Anything outside it — far to
    // the right from drift, or to the left of its own opening because the
    // expression used to live on a longer line — is re-anchored.
    if base >= indent && base <= indent + INDENT_WIDTH * 2 {
        return snippet.to_owned();
    }

    let mut out = String::with_capacity(snippet.len());
    out.push_str(first);
    for line in rest {
        out.push('\n');
        if line.trim().is_empty() {
            // Keep blank lines blank rather than padding them with spaces.
            continue;
        }
        let current = line.len() - line.trim_start().len();
        push_spaces(&mut out, indent + current.saturating_sub(base));
        out.push_str(line.trim_start());
    }
    out
}

fn push_spaces(out: &mut String, count: usize) {
    out.extend(std::iter::repeat_n(' ', count));
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod verify_tests;
