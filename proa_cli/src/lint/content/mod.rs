// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Bounded source analysis of output contributions across registered targets.
//! This is deliberately independent of execution mode and does not certify
//! semantic equivalence. Unsupported Rust remains an explicit boundary.

use crate::diagnostic::{
    Diagnostic, Severity, SourceLabel, SourceRange, Suggestion, SuggestionApplicability,
};
use proa_dsl::content::ContentPolicy;
use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod analyze;
mod discover;
pub(super) use discover::absolute;
#[cfg(test)]
mod tests;

/// A parser belongs to a logical output format, not a sync/async variant.
#[derive(Debug, Clone, Copy)]
pub enum TemplateSyntax {
    Html,
    Markdown,
    /// New formats return semantic output/control nodes using their own parser.
    Custom(fn(proc_macro2::TokenStream) -> syn::Result<Vec<proa_dsl::md::TplNode>>),
}

/// Registration point for another render target. Supply its trait identities,
/// output entry points, and syntax; comparison and intent handling are shared.
/// A format with a different grammar can supply a custom semantic parser,
/// or provide summaries directly through [`compare_views`].
#[derive(Debug, Clone)]
pub struct TargetDescriptor {
    pub name: String,
    pub display_name: String,
    pub traits: Vec<String>,
    pub methods: Vec<String>,
    pub templates: Vec<String>,
    pub adapters: Vec<String>,
    /// Functions whose async argument is polled as this render outcome.
    pub future_sinks: Vec<String>,
    pub syntax: TemplateSyntax,
}

pub fn default_targets() -> Vec<TargetDescriptor> {
    vec![
        TargetDescriptor {
            name: "html".into(),
            display_name: "HTML".into(),
            traits: [
                "WebRenderSync",
                "WebRender",
                "WebRenderSendSync",
                "WebRenderSend",
            ]
            .map(|s| format!("proa_core::{s}"))
            .into(),
            methods: vec!["render".into(), "render_send".into()],
            templates: ["html", "html_sync"]
                .map(|s| format!("proa_macros::{s}"))
                .into(),
            adapters: [
                "to_async",
                "text",
                "text::text",
                "StatelessSyncToSend",
                "SendSyncToAsync",
            ]
            .map(|s| format!("proa_core::{s}"))
            .into(),
            future_sinks: vec!["proa_core::RenderOutcome::pending".into()],
            syntax: TemplateSyntax::Html,
        },
        TargetDescriptor {
            name: "markdown".into(),
            display_name: "Markdown".into(),
            traits: ["MdRenderSync", "MdRender"]
                .map(|s| format!("proa_core::{s}"))
                .into(),
            methods: vec!["render_md".into()],
            templates: ["md", "md_sync"]
                .map(|s| format!("proa_macros::{s}"))
                .into(),
            adapters: ["to_md_async", "text", "text::text"]
                .map(|s| format!("proa_core::{s}"))
                .into(),
            future_sinks: vec!["proa_core::RenderOutcome::pending".into()],
            syntax: TemplateSyntax::Markdown,
        },
    ]
}

#[derive(Debug, Clone)]
pub struct ContentOptions {
    /// Analysis root may include unchanged counterparts. Reporting stays scoped
    /// to the requested files. None discovers enclosing Cargo packages.
    pub root: Option<PathBuf>,
    pub targets: Vec<TargetDescriptor>,
    /// Directed comparisons. A component need not implement every target.
    pub comparisons: Vec<(String, String)>,
    /// Emit informational paired/unpaired/exempt discovery status.
    pub report_status: bool,
}
impl Default for ContentOptions {
    fn default() -> Self {
        Self {
            root: None,
            report_status: false,
            targets: default_targets(),
            comparisons: vec![("html".into(), "markdown".into())],
        }
    }
}

/// Canonical contribution identity. Child inputs stay separate: sharing
/// `self.data` does not make two different children interchangeable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentKey {
    Value(String),
    Child {
        ty: String,
        inputs: BTreeSet<String>,
    },
    Role(String),
}
impl ContentKey {
    fn label(&self) -> String {
        match self {
            Self::Value(v) => v.clone(),
            Self::Child { ty, .. } => ty.rsplit("::").next().unwrap_or(ty).into(),
            Self::Role(v) => format!("content part {v:?}"),
        }
    }
    fn inputs(&self) -> BTreeSet<String> {
        match self {
            Self::Value(v) => BTreeSet::from([v.clone()]),
            Self::Child { inputs, .. } => inputs.clone(),
            Self::Role(_) => BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub expression: String,
    pub range: SourceRange,
}
#[derive(Debug, Clone)]
pub struct Contribution {
    pub key: ContentKey,
    pub presence: Vec<Presence>,
    pub range: SourceRange,
}
#[derive(Debug, Clone)]
pub struct Boundary {
    pub may_hide_content: bool,
    pub message: String,
    pub range: SourceRange,
}
#[derive(Debug, Clone)]
pub struct Omission {
    pub selector: OmissionKey,
    pub from: Option<String>,
    pub range: SourceRange,
    pub presence: Vec<Presence>,
}
#[derive(Debug, Clone)]
pub enum OmissionKey {
    Contributions(Vec<ContentKey>),
    Values(BTreeSet<String>),
    Child(String),
    Role(String),
    Unknown(String),
}

/// Target analyzers can produce this format and reuse the comparison engine.
#[derive(Debug, Clone)]
pub struct ViewSummary {
    pub component: String,
    pub target: String,
    pub target_label: String,
    pub source: String,
    pub entry: SourceRange,
    pub output: Option<SourceRange>,
    pub contributions: Vec<Contribution>,
    pub boundaries: Vec<Boundary>,
    pub omissions: Vec<Omission>,
    pub skip: bool,
    /// Unrendered bindings, resolved at definition time, for useful fixes.
    pub bindings: BTreeMap<String, Vec<ContentKey>>,
    /// Complete Rust child type spellings that need no guessed type arguments.
    pub child_selectors: BTreeMap<String, String>,
}

fn same_presence(a: &[Presence], b: &[Presence]) -> bool {
    a.iter()
        .map(|p| &p.expression)
        .eq(b.iter().map(|p| &p.expression))
}
fn diag(
    view: &ViewSummary,
    range: SourceRange,
    code: &'static str,
    message: String,
    label: &str,
) -> Diagnostic {
    Diagnostic::new(
        &range.path,
        range.line,
        range.column,
        if code == "render/content-policy-invalid" {
            Severity::Error
        } else {
            Severity::Info
        },
        code,
        message,
    )
    .with_label(SourceLabel::new(range, &view.source, label, true))
}

// A local selector belongs after its declaration, while it is still available.
// If shadowing makes the insertion ambiguous, offer a typed selector instead.
fn local_marker_range(view: &ViewSummary, name: &str) -> Option<SourceRange> {
    use syn::spanned::Spanned;
    use syn::visit::Visit;
    struct Declarations<'a> {
        view: &'a ViewSummary,
        name: &'a str,
        ranges: Vec<SourceRange>,
    }
    impl<'ast> Visit<'ast> for Declarations<'_> {
        fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
            let start = method.block.brace_token.span.open().start();
            if start.line == self.view.entry.line && start.column + 2 == self.view.entry.column {
                syn::visit::visit_block(self, &method.block);
            }
        }
        fn visit_local(&mut self, local: &'ast syn::Local) {
            if matches!(&local.pat, syn::Pat::Ident(p) if p.ident == self.name) {
                let mut range = SourceRange::from_span(&self.view.entry.path, local.span());
                range.line = range.end_line;
                range.column = range.end_column;
                self.ranges.push(range);
            }
            syn::visit::visit_local(self, local);
        }
        fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
        fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {}
    }
    let file = syn::parse_file(&view.source).ok()?;
    let mut declarations = Declarations {
        view,
        name,
        ranges: Vec::new(),
    };
    declarations.visit_file(&file);
    if declarations.ranges.len() == 1 {
        declarations.ranges.pop()
    } else {
        None
    }
}

/// Compare summaries without depending on their parsers. Absence is confirmed
/// only inside the supported subset; any opaque output prevents that assertion.
pub fn compare_views(source: &ViewSummary, target: &ViewSummary) -> Vec<Diagnostic> {
    if source.skip || target.skip {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    let mut waived = BTreeSet::new();
    for marker in &target.omissions {
        if marker
            .from
            .as_deref()
            .is_some_and(|from| from != source.target)
        {
            continue;
        }
        if let OmissionKey::Unknown(message) = &marker.selector {
            diagnostics.push(
                diag(
                    target,
                    marker.range.clone(),
                    "render/content-omit-unresolved",
                    message.clone(),
                    "this omission has no resolved selector",
                )
                .with_help("select an in-scope value, a resolvable child type, or a named part"),
            );
            continue;
        }
        let matches: Vec<_> = source
            .contributions
            .iter()
            .enumerate()
            .filter(|(_, part)| {
                let selects = match &marker.selector {
                    OmissionKey::Contributions(keys) => keys.contains(&part.key),
                    OmissionKey::Values(values) => {
                        !values.is_empty() && !values.is_disjoint(&part.key.inputs())
                    }
                    OmissionKey::Child(ty) => {
                        matches!(&part.key, ContentKey::Child {ty: child, ..} if child == ty)
                    }
                    OmissionKey::Role(role) => {
                        matches!(&part.key, ContentKey::Role(r) if r == role)
                    }
                    OmissionKey::Unknown(_) => false,
                };
                selects
                    && (marker.presence.is_empty()
                        || same_presence(&marker.presence, &part.presence))
            })
            .collect();
        // Repeated uses of one obligation under one guard are one exception.
        let distinct: BTreeSet<_> = matches
            .iter()
            .map(|(_, c)| {
                (
                    &c.key,
                    c.presence.iter().map(|p| &p.expression).collect::<Vec<_>>(),
                )
            })
            .collect();
        let (code, message, help) = if matches.is_empty() {
            (
                "render/content-omit-stale",
                format!(
                    "`{}`'s {} omission no longer matches {} content",
                    target.component, target.target_label, source.target_label
                ),
                "remove this omission marker, or select an existing rendered contribution"
                    .to_owned(),
            )
        } else if distinct.len() > 1 {
            (
                "render/content-omit-ambiguous",
                format!(
                    "this omission selects {} distinct contributions",
                    distinct.len()
                ),
                format!(
                    "select one child with `omit!(path::Child)` or one named part; candidates: {}",
                    distinct
                        .iter()
                        .map(|(key, _)| key.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        } else if matches.iter().all(|(_, s)| {
            target
                .contributions
                .iter()
                .any(|t| t.key == s.key && same_presence(&t.presence, &s.presence))
        }) {
            (
                "render/content-omit-redundant",
                "this content is already rendered under the required presence conditions".into(),
                "remove this omission marker".into(),
            )
        } else {
            waived.extend(matches.iter().map(|(index, _)| *index));
            continue;
        };
        diagnostics.push(
            diag(
                target,
                marker.range.clone(),
                code,
                message,
                "this exception does not waive content",
            )
            .with_help(help),
        );
    }
    let mut seen = BTreeSet::new();
    for (index, part) in source.contributions.iter().enumerate() {
        if waived.contains(&index) {
            continue;
        }
        let matches: Vec<_> = target
            .contributions
            .iter()
            .filter(|p| p.key == part.key)
            .collect();
        if matches
            .iter()
            .any(|p| same_presence(&p.presence, &part.presence))
        {
            continue;
        }
        let guard_key = part
            .presence
            .iter()
            .map(|p| p.expression.clone())
            .collect::<Vec<_>>();
        if !seen.insert((part.key.clone(), guard_key)) {
            continue;
        }
        let changed_child=target.contributions.iter().find(|c|matches!((&part.key,&c.key),(ContentKey::Child{ty:source,..},ContentKey::Child{ty:target,..}) if source==target));
        let forwarded=target.contributions.iter().find(|c|matches!(&part.key,ContentKey::Value(value) if matches!(&c.key,ContentKey::Child{inputs,..} if inputs.contains(value))));
        let uncertain = changed_child.is_some()
            || forwarded.is_some()
            || !matches.is_empty()
            || target.boundaries.iter().any(|b| b.may_hide_content);
        let code = if uncertain {
            "render/content-coverage-unchecked"
        } else {
            "render/content-coverage"
        };
        let label = part.key.label();
        let message = if !matches.is_empty() {
            format!(
                "`{label}` has different presence conditions in `{}`'s {} and {} views",
                target.component, source.target_label, target.target_label
            )
        } else if uncertain {
            format!(
                "{} coverage of `{label}` in `{}` could not be checked",
                target.target_label, target.component
            )
        } else {
            format!(
                "`{}`'s {} view does not render `{label}` from its {} view",
                target.component, target.target_label, source.target_label
            )
        };
        let at = matches
            .first()
            .and_then(|m| m.presence.first())
            .map(|p| p.range.clone())
            .or_else(|| changed_child.map(|part| part.range.clone()))
            .or_else(|| forwarded.map(|part| part.range.clone()))
            .or_else(|| {
                target
                    .boundaries
                    .iter()
                    .find(|b| b.may_hide_content)
                    .map(|b| b.range.clone())
            })
            .unwrap_or_else(|| {
                target
                    .output
                    .clone()
                    .unwrap_or_else(|| target.entry.clone())
            });
        let mut d = diag(
            target,
            at,
            code,
            message,
            if !matches.is_empty() {
                "checked view's presence condition"
            } else if uncertain {
                "coverage stops at this boundary"
            } else {
                "this view does not render the contribution"
            },
        );
        let source_at = if !matches.is_empty() {
            part.presence
                .first()
                .map(|p| p.range.clone())
                .unwrap_or_else(|| part.range.clone())
        } else {
            part.range.clone()
        };
        d = d.with_label(SourceLabel::new(
            source_at,
            &source.source,
            format!("`{label}` rendered in {}", source.target_label),
            false,
        ));
        if !matches.is_empty() {
            d = d.with_note("the checker has not established that these conditions select the same input states")
                .with_help("decide presence once in shared preparation, or acknowledge this contribution with a targeted omission");
        } else if uncertain {
            if changed_child.is_some() {
                d.notes.push("the same child is rendered with different inputs; the checker has not established their correspondence".into());
            }
            if forwarded.is_some() {
                d.notes.push("the input is forwarded to a child, but forwarding does not prove that the child renders it".into());
            }
            for b in target
                .boundaries
                .iter()
                .chain(source.boundaries.iter())
                .take(3)
            {
                d.notes.push(b.message.clone());
            }
            d = d.with_note("a helper or unsupported path may already include this content; no omission is confirmed")
                .with_help("expose the child in a supported render path, or name its contribution with `proa_macros::content_part!(\"role\", expression)` in both views");
        } else {
            if let Some((name, _)) = target
                .bindings
                .iter()
                .find(|(name, keys)| name.as_str() != "self" && keys.contains(&part.key))
            {
                d = d.with_help(format!("add `{{{name}}}` to the {} template; constructing or testing a value does not render it", target.target_label));
            } else {
                d = d.with_help(format!(
                    "render `{label}` in the {} view",
                    target.target_label
                ));
            }
        }
        // An omission of a child is meaningful even without a target-local
        // constructor. Canonical crate paths remain resolvable in this crate.
        let local = target.bindings.iter().find_map(|(name, keys)| {
            (name.as_str() != "self" && keys.contains(&part.key))
                .then(|| local_marker_range(target, name).map(|range| (name, range)))
                .flatten()
        });
        let omit = if let Some((name, _)) = &local {
            Some(format!("proa_macros::omit!(&{name});"))
        } else {
            match &part.key {
                ContentKey::Child { ty, .. } => source
                    .child_selectors
                    .get(ty)
                    .map(|ty| format!("proa_macros::omit!({ty});")),
                ContentKey::Role(role) => Some(format!("proa_macros::omit!({role:?});")),
                ContentKey::Value(value)
                    if value.starts_with("self.")
                        && syn::parse_str::<proa_dsl::content::OmitContent>(value).is_ok()
                        && (source.component == target.component
                            || target.bindings.get("self").is_some_and(|keys| {
                                keys.iter().any(|key| key.inputs().contains(value))
                            })) =>
                {
                    Some(format!("proa_macros::omit!({value});"))
                }
                _ => None,
            }
        };
        if let Some(replacement) = omit {
            let mut range = local
                .as_ref()
                .map_or_else(|| target.entry.clone(), |(_, range)| range.clone());
            range.end_line = range.line;
            range.end_column = range.column;
            d = d.with_suggestion(Suggestion {
                message: if let Some((name, _)) = &local {
                    format!("to omit it intentionally, add this after `{name}` is declared inside the render method")
                } else {
                    "to omit it intentionally, add this inside the render method before its template".into()
                },
                range,
                replacement,
                applicability: SuggestionApplicability::MaybeIncorrect,
            });
        }
        diagnostics.push(d);
    }
    // Even fully matched dynamic fields cannot prove unnamed prose equivalence.
    if diagnostics.is_empty() {
        if let Some(boundary) = source
            .boundaries
            .first()
            .or_else(|| target.boundaries.first())
        {
            let view = if source.boundaries.first().is_some() {
                source
            } else {
                target
            };
            diagnostics.push(diag(view, boundary.range.clone(), "render/content-coverage-unchecked", format!("`{}`'s {} → {} content is only partially checked", target.component,source.target_label,target.target_label), "this output is outside the supported analysis").with_note(boundary.message.clone()).with_help("use matching `content_part!(\"role\", expression)` wrappers for semantic contributions hidden by this boundary"));
        }
    }
    let mut grouped: Vec<Diagnostic> = Vec::new();
    let mut opaque_groups = BTreeMap::new();
    for diagnostic in diagnostics {
        let opaque = diagnostic.code == "render/content-coverage-unchecked"
            && diagnostic
                .notes
                .iter()
                .any(|n| n.contains("no omission is confirmed"));
        let location = (diagnostic.path.clone(), diagnostic.line, diagnostic.column);
        if opaque {
            if let Some(index) = opaque_groups.get(&location).copied() {
                let previous: &mut Diagnostic = &mut grouped[index];
                previous
                    .labels
                    .extend(diagnostic.labels.into_iter().filter(|label| !label.primary));
                previous.message = format!(
                    "`{}`'s {} coverage of {} {} contributions could not be checked",
                    target.component,
                    target.target_label,
                    previous.labels.len() - 1,
                    source.target_label
                );
                continue;
            }
            opaque_groups.insert(location, grouped.len());
        }
        grouped.push(diagnostic);
    }
    grouped
}

pub(super) fn lint_sources(
    sources: &[(PathBuf, String)],
    reporting: &BTreeSet<PathBuf>,
    include_tests: bool,
    options: &ContentOptions,
) -> Vec<Diagnostic> {
    discover::lint_sources(sources, reporting, include_tests, options)
}
pub(super) fn lint_paths(
    files: &[PathBuf],
    include_tests: bool,
    options: &ContentOptions,
) -> anyhow::Result<Vec<Diagnostic>> {
    let sources = discover::load_sources(files, options)?;
    Ok(lint_sources(
        &sources,
        &files.iter().map(|p| discover::absolute(p)).collect(),
        include_tests,
        options,
    ))
}
fn span_range(path: &Path, span: Span) -> SourceRange {
    SourceRange::from_span(path, span)
}
