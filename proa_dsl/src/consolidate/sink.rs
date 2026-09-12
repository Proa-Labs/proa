// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The consolidation sink: the seam between the shared consolidation walk
//! and a concrete emitter.
//!
//! The walk in [`super::walk`] owns everything semantic about a template:
//! which nodes fold into static runs, when runs flush, compile-time
//! escaping, the noopener defense, raw-text (`<script>`/`<style>`) content
//! modes, and URL-position attribute classification. A
//! [`ConsolidationSink`] receives the resulting linear event sequence and
//! decides how to materialize it. Implementations: the `TokenStream`
//! emitter in `proa_macros::static_consolidation::emit_tokens` (the macro
//! path), plus any downstream program emitter that serializes the sequence
//! as data.
//!
//! Contract for implementations: nothing in this trait may return or
//! store `proc_macro2::TokenStream` / `quote` types. Spans and `syn::Expr`
//! references are acceptable inputs (a non-macro emitter can lower or reject
//! them), but the event vocabulary itself must stay representation-neutral.

use syn::Expr;

/// A dynamic attribute boundary (`name={expr}`).
///
/// The walk pre-renders both spellings of the attribute name so emitters do
/// not re-implement dialect rules:
///
/// - `name_prefix` is the ` name="` byte prefix written before a
///   present-value render (note the leading space and trailing quote).
/// - `bool_form` is the boolean/valueless spelling: ` name` in HTML,
///   ` name=""` in XML.
///
/// How the two are used (prefix + value + closing quote vs. bool form vs.
/// skipped entirely for `Option::None` / `false` values) is decided by the
/// value's `AttrValue` impl at render time, so the sink forwards both.
pub struct DynamicAttr<'a> {
    /// ` name="` prefix, ready to write before the escaped value.
    pub name_prefix: &'a str,
    /// Boolean spelling of the attribute (` name` or ` name=""`).
    pub bool_form: &'a str,
    /// The attribute value expression.
    pub value: &'a Expr,
    /// True when the attribute is URL-bearing (`href`, `src`, `action`,
    /// ...) in HTML, so the emitted render must apply runtime
    /// dangerous-scheme validation (`render_url_attr`). The walk owns this
    /// classification; emitters must honor it (CVE review Gap O).
    pub is_url_position: bool,
}

/// Raw-text element kinds that require a runtime content-mode switch when
/// they contain dynamic children (sentinel masking of `</script`,
/// `<script`, `<!--`). The walk decides *when* a switch is needed; the
/// emitter decides *how* the mode change is materialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawTextKind {
    Script,
    Style,
}

/// Receiver for the consolidation walk's linear event sequence.
///
/// Events arrive in document order. Static bytes between two dynamic
/// boundaries always arrive as a single `static_run` call (the walk flushes
/// its accumulator exactly at dynamic boundaries), so a fully static
/// template produces exactly one event.
pub trait ConsolidationSink {
    /// A run of static bytes, already compile-time escaped (and with all
    /// static security/a11y fixups applied). Must be written verbatim.
    fn static_run(&mut self, pre_escaped: &str);

    /// A dynamic `{expr}` block in node (child) position. The expression is
    /// arbitrary Rust from the template author; lowering it (control-flow
    /// recursion, render-call wrapping, or dialect validation for non-Rust
    /// backends) is emitter policy.
    fn dynamic_node(&mut self, expr: &Expr);

    /// A dynamic attribute on the current element. The surrounding static
    /// run has already been flushed.
    fn dynamic_attr(&mut self, attr: DynamicAttr<'_>);

    /// Enter a raw-text (`<script>`/`<style>`) region with dynamic
    /// children. All events until the matching `end_raw_text` belong to the
    /// element's children and must render under the given content mode.
    /// Regions can nest (degenerate but syntactically possible); calls are
    /// properly paired and LIFO.
    fn begin_raw_text(&mut self, kind: RawTextKind);

    /// Leave the raw-text region opened by the matching `begin_raw_text`.
    fn end_raw_text(&mut self, kind: RawTextKind);
}
