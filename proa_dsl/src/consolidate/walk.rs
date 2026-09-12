// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The shared consolidation walk.
//!
//! Walks a validated template tree and emits a linear event sequence into a
//! [`ConsolidationSink`]. This module owns all semantic decisions:
//!
//! - which nodes fold into static runs and when runs flush (exactly at
//!   dynamic boundaries),
//! - compile-time escaping of static text and attribute values,
//! - the reverse-tabnabbing (`noopener`) defense on static anchors,
//! - stripping the `_a11y_ok` validation opt-out attribute,
//! - raw-text (`<script>`/`<style>`) content-mode boundaries,
//! - URL-position classification of dynamic attributes.
//!
//! Emitters implement [`ConsolidationSink`] and never re-derive any of the
//! above. The validation pass in the parent module runs before the walk and
//! is independent of it.

use super::sink::{ConsolidationSink, DynamicAttr, RawTextKind};
use super::{
    attr_name_is, escape_attr_compile_time, is_static_node, node_has_dynamic_content,
    noopener_rel_action, render_static_node, MarkupDialect, NoopenerAction,
};
use crate::parse::{Attribute, Element, HtmlRoot, Node};

/// Consolidate a whole (mixed static/dynamic) template into `sink`.
///
/// The caller is responsible for the fully-static fast path; this walk
/// still handles fully static input correctly (one `static_run` event).
pub fn consolidate_root<S: ConsolidationSink>(
    root: &HtmlRoot,
    dialect: MarkupDialect,
    sink: &mut S,
) {
    let mut static_run = String::new();
    for node in &root.nodes {
        walk_node(node, &mut static_run, dialect, sink);
    }
    flush_static_run(&mut static_run, sink);
}

/// Flush the accumulated static run into the sink, if non-empty.
fn flush_static_run<S: ConsolidationSink>(static_run: &mut String, sink: &mut S) {
    if !static_run.is_empty() {
        sink.static_run(static_run);
        static_run.clear();
    }
}

/// Walk a single node. Static content accumulates in `static_run`; dynamic
/// content flushes the run and emits its own events.
fn walk_node<S: ConsolidationSink>(
    node: &Node,
    static_run: &mut String,
    dialect: MarkupDialect,
    sink: &mut S,
) {
    if is_static_node(node) {
        // Accumulate static content (compile-time escaped).
        render_static_node(node, static_run, dialect);
        return;
    }
    match node {
        Node::Block(expr) => {
            flush_static_run(static_run, sink);
            sink.dynamic_node(expr);
        }
        Node::Element(el) => {
            // Elements handle their own static/dynamic logic. Don't flush
            // here - let the element accumulate static parts.
            walk_element(el, static_run, dialect, sink);
        }
        Node::Fragment(children) => {
            for child in children {
                walk_node(child, static_run, dialect, sink);
            }
        }
        _ => {
            // Text and Doctype are always static, handled above.
        }
    }
}

/// Walk an element with potentially mixed static/dynamic content.
// proa-perf-ok(scope): the consolidation walk assembles compile-time pre-escaped static runs; this is the template system itself
fn walk_element<S: ConsolidationSink>(
    el: &Element,
    static_run: &mut String,
    dialect: MarkupDialect,
    sink: &mut S,
) {
    // `tag_str()` keeps hyphenated custom-element tags intact when emitting
    // raw HTML; the parser flattens `-` to `_` for storage.
    let tag = dialect.emitted_tag(el);

    // Opening tag
    static_run.push('<');
    static_run.push_str(&tag);

    // Reverse tabnabbing defense — see noopener_rel_action() for OWASP reference.
    // When merging, skip the existing `rel` attr during the loop and write the
    // merged value below so we never emit two `rel=` attributes.
    let noopener = if dialect == MarkupDialect::Html {
        noopener_rel_action(&tag, &el.attrs)
    } else {
        NoopenerAction::None
    };
    let skip_rel = matches!(noopener, NoopenerAction::Merge(_));

    // Attributes
    for attr in &el.attrs {
        // Strip the `_a11y_ok` opt-out attribute from output on both sync
        // and async paths. Its only purpose is to silence `validate_a11y`
        // (see the companion guard in `render_static_element`); it is never
        // real markup.
        if dialect == MarkupDialect::Html && attr_name_is(attr, "_a11y_ok") {
            continue;
        }
        match attr {
            Attribute::Static { name, value } => {
                let attr_name = dialect.attr_name(name);
                // Skip the existing `rel` when we will write a merged value below.
                if skip_rel && attr_name == "rel" {
                    continue;
                }
                static_run.push(' ');
                static_run.push_str(&attr_name);
                static_run.push_str("=\"");
                // Escape attribute value at compile time
                escape_attr_compile_time(&value.value(), static_run, dialect);
                static_run.push('"');
            }
            Attribute::Bool { name } => {
                let attr_name = dialect.attr_name(name);
                static_run.push_str(&dialect.bool_attr_bytes(&attr_name));
            }
            Attribute::Dynamic { name, value } => {
                flush_static_run(static_run, sink);

                let attr_name = dialect.attr_name(name);
                let name_prefix = format!(" {}=\"", attr_name);
                let bool_form = dialect.bool_attr_bytes(&attr_name);

                // URL-bearing attributes (`href`, `src`, `action`, ...) must
                // route through runtime dangerous-scheme validation so
                // `javascript:` / `vbscript:` / executable `data:` schemes in
                // the dynamic value are blocked. Static literals are already
                // rejected at compile time in `validate_element`; this closes
                // the dynamic gap so `href={user_value}` is not a silent XSS
                // sink (CVE review Gap O).
                let is_url_position =
                    dialect == MarkupDialect::Html && super::is_url_attr_name(&attr_name);

                sink.dynamic_attr(DynamicAttr {
                    name_prefix: &name_prefix,
                    bool_form: &bool_form,
                    value,
                    is_url_position,
                });
            }
        }
    }

    match noopener {
        NoopenerAction::None => {}
        NoopenerAction::Inject => {
            static_run.push_str(" rel=\"noopener noreferrer\"");
        }
        NoopenerAction::Merge(existing) => {
            static_run.push_str(" rel=\"");
            escape_attr_compile_time(&existing, static_run, dialect);
            static_run.push_str(" noopener noreferrer\"");
        }
    }

    if (dialect == MarkupDialect::Xml && el.self_closing)
        || (dialect == MarkupDialect::Html && super::is_svg_void_element(tag.as_str()))
    {
        static_run.push_str("/>");
        return;
    }

    static_run.push('>');

    // Children (if not self-closing)
    if !el.self_closing {
        // `<script>` / `<style>` are raw-text elements. When they contain
        // dynamic children, switch the runtime content mode so interpolated
        // text is escaped with the raw-text sentinel masking (`</script`,
        // `<script`, `<!--`) instead of plain HTML-text escaping, which would
        // not stop a `</script>` breakout. Static-only scripts skip the switch
        // to preserve static consolidation.
        if let Some(kind) = raw_text_kind(&tag, dialect, el) {
            flush_static_run(static_run, sink);
            sink.begin_raw_text(kind);
            for child in &el.children {
                walk_node(child, static_run, dialect, sink);
            }
            flush_static_run(static_run, sink);
            sink.end_raw_text(kind);
        } else {
            for child in &el.children {
                walk_node(child, static_run, dialect, sink);
            }
        }

        // Closing tag
        static_run.push_str("</");
        static_run.push_str(&tag);
        static_run.push('>');
    }
}

/// Returns the raw-text kind for an element (`<script>` / `<style>`) that
/// has dynamic children and therefore needs a runtime content-mode switch
/// for sentinel masking, or `None` otherwise. Only HTML-dialect
/// `script`/`style` qualify; XML has no raw-text element states.
fn raw_text_kind(tag: &str, dialect: MarkupDialect, el: &Element) -> Option<RawTextKind> {
    if dialect != MarkupDialect::Html {
        return None;
    }
    let kind = match tag {
        "script" => RawTextKind::Script,
        "style" => RawTextKind::Style,
        _ => return None,
    };
    if !el.children.iter().any(node_has_dynamic_content) {
        return None;
    }
    Some(kind)
}
