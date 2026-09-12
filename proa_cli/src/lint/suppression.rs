// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Debug)]
pub(crate) struct SuppressionComments {
    lines: HashSet<usize>,
    /// `proa-perf-ok(scope): reason` markers awaiting attachment to the
    /// item that starts directly below them.
    scope_markers: Vec<(usize, usize)>,
    /// Line ranges of items covered by an attached scope marker.
    scopes: Vec<RangeInclusive<usize>>,
    /// Scope markers that did not precede an item (reported as invalid).
    unattached_scopes: Vec<(usize, usize)>,
    invalid: Vec<InvalidSuppression>,
}

#[derive(Debug)]
pub(crate) struct InvalidSuppression {
    line: usize,
    column: usize,
}

impl SuppressionComments {
    pub(crate) fn new(source: &str) -> Self {
        let mut lines = HashSet::new();
        let mut scope_markers = Vec::new();
        let mut invalid = Vec::new();

        for (idx, line) in source.lines().enumerate() {
            let line_no = idx + 1;
            let Some(comment_start) = line_comment_start(line) else {
                continue;
            };
            let comment = &line[comment_start..];
            let Some(marker) = comment.find("proa-perf-ok") else {
                continue;
            };
            let tail = &comment[marker + "proa-perf-ok".len()..];
            let (is_scope, tail) = match tail.strip_prefix("(scope)") {
                Some(rest) => (true, rest),
                None => (false, tail),
            };
            let has_reason = tail
                .strip_prefix(':')
                .is_some_and(|reason| !reason.trim().is_empty());

            if has_reason && is_scope {
                scope_markers.push((line_no, comment_start + marker + 1));
            } else if has_reason {
                lines.insert(line_no);
            } else {
                invalid.push(InvalidSuppression {
                    line: line_no,
                    column: comment_start + marker + 1,
                });
            }
        }

        Self {
            lines,
            scope_markers,
            scopes: Vec::new(),
            unattached_scopes: Vec::new(),
            invalid,
        }
    }

    /// Attach each `proa-perf-ok(scope)` marker to the item (fn, impl,
    /// struct, mod, ...) that starts within the next few lines, suppressing
    /// perf lints across the item's whole span. Unattached markers are
    /// reported via [`Self::push_invalid_diagnostics`].
    pub(crate) fn attach_item_scopes(&mut self, file: &File) {
        let mut item_ranges: Vec<(usize, usize)> = Vec::new();
        fn collect(items: &[Item], out: &mut Vec<(usize, usize)>) {
            for item in items {
                let span = item.span();
                out.push((span.start().line, span.end().line));
                match item {
                    Item::Mod(item_mod) => {
                        if let Some((_, inner)) = &item_mod.content {
                            collect(inner, out);
                        }
                    }
                    // Impl methods are scope targets of their own, so a
                    // marker above one method does not blanket the impl.
                    Item::Impl(item_impl) => {
                        for impl_item in &item_impl.items {
                            if let ImplItem::Fn(method) = impl_item {
                                let span = method.span();
                                out.push((span.start().line, span.end().line));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        collect(&file.items, &mut item_ranges);
        item_ranges.sort_unstable();

        for (line, column) in std::mem::take(&mut self.scope_markers) {
            // Preferred placement: directly above the item (a few lines of
            // attributes allowed).
            let mut attached = item_ranges
                .iter()
                .find(|(start, _)| *start > line && *start <= line + 4)
                .copied();
            // Fallback: the marker sits inside the item span. This covers
            // markers placed between an item's doc comments and its `fn`
            // keyword (syn spans start at the docs), or at the top of a
            // body. Pick the innermost containing item.
            if attached.is_none() {
                attached = item_ranges
                    .iter()
                    .filter(|(start, end)| *start <= line && line <= *end)
                    .max_by_key(|(start, _)| *start)
                    .copied();
            }
            match attached {
                Some((start, end)) => self.scopes.push(start..=end),
                None => self.unattached_scopes.push((line, column)),
            }
        }
    }

    pub(crate) fn is_suppressed(&self, line: usize, code: &str) -> bool {
        (code.starts_with("perf/")
            || matches!(
                code,
                "render/async-unneeded"
                    | "render/md-async-unneeded"
                    | "render/static-string-to-string"
                    // Composed MdContext writes are sometimes the only way to
                    // express a construct (template slots inside backticks
                    // stay literal); an annotated reason opts a scope out.
                    | "md/manual-write"
                    // Owned per-request view models are a legitimate shape
                    // for pages whose data arrives per request; an annotated
                    // reason opts a scope out.
                    | "render/owned-view-model"
            ))
            && (self.lines.contains(&line)
                || line
                    .checked_sub(1)
                    .is_some_and(|prev| self.lines.contains(&prev))
                || self.scopes.iter().any(|range| range.contains(&line)))
    }

    pub(crate) fn push_invalid_diagnostics(
        &self,
        path: &Path,
        ignored_ranges: &[RangeInclusive<usize>],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for invalid in &self.invalid {
            if line_in_ranges(invalid.line, ignored_ranges) {
                continue;
            }
            diagnostics.push(
                Diagnostic::new(
                    path,
                    invalid.line,
                    invalid.column,
                    Severity::Warning,
                    "perf/invalid-suppression",
                    "proa-perf-ok suppression comments must include a reason",
                )
                .with_help(
                    "write `// proa-perf-ok: reason` on the same or previous line, or `// proa-perf-ok(scope): reason` directly above an item",
                ),
            );
        }
        for (line, column) in &self.unattached_scopes {
            if line_in_ranges(*line, ignored_ranges) {
                continue;
            }
            diagnostics.push(
                Diagnostic::new(
                    path,
                    *line,
                    *column,
                    Severity::Warning,
                    "perf/invalid-suppression",
                    "proa-perf-ok(scope) must be placed directly above a fn/impl/struct item",
                )
                .with_help("move the comment to the line above the item it should cover"),
            );
        }
    }
}

pub(crate) fn line_comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut idx = 0;
    let mut in_string = false;

    while idx < bytes.len() {
        match bytes[idx] {
            b'\\' if in_string => idx += 2,
            b'"' => {
                in_string = !in_string;
                idx += 1;
            }
            // A `'` is only a char literal when a closing quote follows
            // (`'a'`, `'\n'`). Lifetimes (`<'a>`) have no closing quote and
            // must not flip scanner state, or the `//` after them is missed.
            b'\'' if !in_string => {
                if crate::source::looks_like_char_literal(bytes, idx) {
                    idx = crate::source::skip_char(bytes, idx + 1);
                } else {
                    idx += 1;
                }
            }
            b'/' if !in_string && bytes.get(idx + 1) == Some(&b'/') => {
                if matches!(bytes.get(idx + 2), Some(b'/' | b'!')) {
                    return None;
                }
                return Some(idx + 2);
            }
            _ => idx += 1,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_perf_marker_can_suppress_intentional_render_bridge_rules() {
        let source =
            "// proa-perf-ok(scope): core primitive supports async composition\nfn bridge() {}\n";
        let file = syn::parse_file(source).unwrap();
        let mut suppressions = SuppressionComments::new(source);
        suppressions.attach_item_scopes(&file);

        assert!(suppressions.is_suppressed(2, "render/async-unneeded"));
        assert!(!suppressions.is_suppressed(2, "render/sync-child-in-async"));
    }

    #[test]
    fn perf_marker_can_narrowly_suppress_intentional_string_signal_seeds() {
        let source = "fn render() {\n    // proa-perf-ok: String signal state must own callback values\n    let name = \"Ada\".to_string();\n}\n";
        let file = syn::parse_file(source).unwrap();
        let mut suppressions = SuppressionComments::new(source);
        suppressions.attach_item_scopes(&file);

        assert!(suppressions.is_suppressed(3, "render/static-string-to-string"));
        assert!(!suppressions.is_suppressed(3, "render/sync-child-in-async"));
        assert!(!suppressions.is_suppressed(3, "render/component-data-shape"));
    }
}
