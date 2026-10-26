// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The consolidation core: template validation, compile-time escaping,
//! static rendering, and the static/dynamic chunking walk shared by the
//! `html!` / `html_sync!` proc macros and downstream runtime-template
//! compilers.
//!
//! This module analyzes the HTML AST and consolidates static portions into
//! pre-escaped byte runs, emitting a linear event sequence through the
//! [`sink::ConsolidationSink`] seam. It lives in `proa_dsl` (a plain
//! library crate) rather than `proa_macros` because proc-macro crates
//! cannot export types. The macro's `TokenStream` emitter and any downstream
//! program emitter can consume this one core, so escaping, a11y/security
//! validation, and chunking decisions are made exactly once.
//!
//! Layout:
//! - this module: the validation pass and the static rendering/escaping
//!   helpers,
//! - [`walk`]: the shared consolidation walk,
//! - [`sink`]: the emitter seam ([`sink::ConsolidationSink`]).
//!
//! Emitters implement [`sink::ConsolidationSink`]: the `TokenStream`
//! emitter stays in `proa_macros::static_consolidation::emit_tokens`;
//! downstream compilers may implement their own data-oriented emitters.

use crate::parse::{Attribute, Element, HtmlRoot, MarkupName, Node};
use proc_macro2::TokenStream;
use quote::quote;
use std::{cell::Cell, fmt::Write as _};
use syn::Expr;

pub mod sink;
pub mod walk;

const PROA_DOCTYPE_MARKER: &str = "<!-- Rendered with Proa -->";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkupDialect {
    Html,
    Xml,
}

impl MarkupDialect {
    fn emitted_tag(self, el: &Element) -> String {
        let tag = el.tag_str();
        match self {
            MarkupDialect::Html => tag.to_lowercase(),
            MarkupDialect::Xml => tag,
        }
    }

    fn bool_attr_bytes(self, attr_name: &str) -> String {
        match self {
            MarkupDialect::Html => format!(" {attr_name}"),
            MarkupDialect::Xml => format!(" {attr_name}=\"\""),
        }
    }

    fn attr_name(self, name: &MarkupName) -> String {
        match self {
            MarkupDialect::Html => name.to_string().replace('_', "-"),
            MarkupDialect::Xml => name.as_markup_str().to_owned(),
        }
    }

    pub fn runtime_escape_tokens(self) -> TokenStream {
        match self {
            MarkupDialect::Html => quote! {},
            MarkupDialect::Xml => quote! {
                let __proa_prev_escape_dialect =
                    cx.__proa_enter_escape_dialect(::proa_core::shared::escape::EscapeDialect::Xml);
            },
        }
    }

    pub fn runtime_restore_tokens(self) -> TokenStream {
        match self {
            MarkupDialect::Html => quote! {},
            MarkupDialect::Xml => quote! {
                cx.__proa_restore_escape_dialect(__proa_prev_escape_dialect);
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Compile-time warnings via #[must_use]
// ---------------------------------------------------------------------------

/// A validation warning, carried as structured data so both consumers can
/// materialize it their own way: the macro path lowers each warning to a
/// `#[must_use]` struct (via [`Warning::into_tokens`]), while a
/// runtime-template compiler can map it into a serializable diagnostic.
#[derive(Debug, Clone)]
pub struct Warning {
    /// Span of the offending template node (line/column available via
    /// proc-macro2's `span-locations`, which this crate enables).
    pub span: proc_macro2::Span,
    /// Short stable title, e.g. `"a11y: img missing alt attribute"`.
    pub title: String,
    /// Full human-readable message.
    pub message: String,
}

/// Counter for generating unique warning struct names.
static WARNING_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Build a structured [`Warning`]. Kept under the historical name so the
/// validator call sites read unchanged; token materialization happens at
/// the [`validate_security`] boundary.
fn emit_warning(span: proc_macro2::Span, title: &str, message: &str) -> Warning {
    Warning {
        span,
        title: title.to_string(),
        message: message.to_string(),
    }
}

impl Warning {
    /// Generate a `#[must_use = "message"]` struct at the warning's span.
    /// This triggers rustc's `unused_must_use` lint, which displays the message with
    /// proper file/line/column info — the best we can do on stable Rust.
    ///
    /// The `#[warn(unused_must_use)]` inside the block ensures the warning fires
    /// even if the calling crate has `#![allow(unused_must_use)]`. Test targets
    /// suppress advisory diagnostics so warning-denied CI can exercise unsafe
    /// input handling without turning the expected warning into a hard error.
    ///
    /// TODO: Replace with `Diagnostic::span_warning()` once `proc_macro_diagnostic`
    /// stabilizes on stable Rust. Tracking issue:
    /// <https://github.com/rust-lang/rust/issues/54140>
    pub fn into_tokens(&self) -> TokenStream {
        let id = WARNING_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Turn the title into a valid Rust identifier for the struct name.
        // e.g. "a11y: prefer <a href> over <span role=\"link\">"
        //    → "a11y__prefer_a_href_over_span_role_link"
        let ident_str: String = self
            .title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .replace("__", "_")
            .trim_matches('_')
            .to_string();
        let struct_name = syn::Ident::new(&format!("_{ident_str}_{id}"), self.span);
        let msg = self.message.clone();
        quote! {
            {
                #[allow(non_camel_case_types)]
                #[must_use = #msg]
                struct #struct_name;
                #[cfg_attr(not(test), warn(unused_must_use))]
                #[cfg_attr(test, allow(unused_must_use))]
                #struct_name;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostic formatting
// ---------------------------------------------------------------------------

/// Format a structured diagnostic message for `#[must_use]` warnings.
/// Kept concise since rustc adds its own "note:" and "help:" lines around it.
fn diagnostic(title: &str, _note: &str, help: &str, spec: &str) -> String {
    format!("{title}. {help} ({spec})")
}

/// Shorthand for diagnostics without a help section.
fn diagnostic_note(title: &str, note: &str, spec: &str) -> String {
    format!("{title}. {note} ({spec})")
}

// ---------------------------------------------------------------------------
// OWASP security validation (runs at compile time during macro expansion)
// ---------------------------------------------------------------------------

/// URL-bearing attribute names that should be checked for dangerous schemes.
/// Kept in sync with `AttrKey::is_url_attr()` in proa_core — see CVE review
/// Gap A for the full set and the Angular CVE-2026-32635 precedent.
pub fn is_url_attr_name(name: &str) -> bool {
    const URL_ATTR_NAMES: &[&str] = &[
        "href",
        "src",
        "action",
        "formaction",
        "poster",
        "ping",
        "cite",
        "data",
        "background",
        "longdesc",
        "manifest",
        // SVG `xlink:href` is a single-URL attribute and a known
        // `javascript:` vector. Both spellings: `:` as
        // authored, and `-` after the macro's `_`->`-` normalization of
        // `xlink_href`. `srcset` is intentionally excluded: its
        // comma-separated `url descriptor, ...` grammar is not a single URL
        // and would defeat the whole-string scheme check.
        "xlink:href",
        "xlink-href",
    ];

    URL_ATTR_NAMES
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// True if rendering `node` emits a runtime render call for a dynamic `{expr}`
/// value, as opposed to purely compile-time-static text. Drives the decision
/// to switch content mode for `<script>` / `<style>` children.
fn node_has_dynamic_content(node: &Node) -> bool {
    match node {
        Node::Block(_) => true,
        Node::Fragment(children) => children.iter().any(node_has_dynamic_content),
        Node::Element(el) => el.children.iter().any(node_has_dynamic_content),
        Node::Text(_) | Node::Doctype(_) | Node::XmlDeclaration(_) => false,
    }
}

/// True for HTML event-handler attribute names (`onclick`, `onerror`,
/// `onmouseover`, `onload`, ...). These are JavaScript execution contexts: a
/// value interpolated into one runs as script, so attribute-value escaping
/// cannot make a dynamic value safe. The match is `on` followed by one or more
/// ASCII letters, which covers the standard handler set without flagging
/// unrelated names (no standard HTML attribute starts with `on` except event
/// handlers, and `data-on*` / `aria-*` do not start with `on`).
pub fn is_event_handler_attr_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() > 2
        && bytes[0].eq_ignore_ascii_case(&b'o')
        && bytes[1].eq_ignore_ascii_case(&b'n')
        && bytes[2..].iter().all(u8::is_ascii_alphabetic)
}

/// Canonical static/runtime URL policy. The source-compatible alias keeps
/// existing macro consumers on the single implementation owned by
/// `proa_core`.
pub use proa_html_policy::has_dangerous_url_scheme as is_dangerous_url_value;

/// Walk the AST and collect compile-time security errors/warnings.
///
/// Two classes of check:
///
/// 1. **Hard error — dangerous URL schemes** (`javascript:`, `vbscript:`, and
///    executable `data:` MIME types)
///    in static `href`/`src`/`action`/`formaction` attributes. The `javascript:`
///    pseudo-scheme allows arbitrary script execution on navigation; blocking it
///    at compile time eliminates an entire class of reflected XSS.
///    Reference: OWASP XSS Prevention Cheat Sheet, Rule #7 — "Avoid JavaScript URLs"
///    <https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html>
///
/// 2. **Cargo warning — dynamic `{expr}` inside `<script>` or `<style>`**.
///    HTML-context escaping (`&`, `<`, `>`) does not prevent injection within
///    JavaScript or CSS. OWASP defines these as separate encoding contexts
///    (Rule #3: JavaScript, Rule #4: CSS) that require their own encoders.
///    Proa is an HTML template engine, not a JS/CSS encoder, so we warn and
///    recommend `data-*` attributes + JSON serialization instead.
///    Reference: OWASP XSS Prevention Cheat Sheet, Rules #3 & #4
///    <https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html>
///
/// Ancestor context threaded through the validation walk.
struct AncestorCtx<'a> {
    /// Stack of ancestor tag names (outermost first).
    stack: &'a [String],
    /// Whether we're inside a <script> or <style> element.
    in_script_or_style: bool,
    /// Whether we're inside a raw-text or escapable-raw-text element
    /// (<textarea>, <xmp>, <noscript>, <noframes>).
    /// These have different parser semantics from normal flow content —
    /// dynamic interpolation requires different guarantees than HTML escape
    /// provides on its own. See CVE review Gap B.
    in_raw_text_element: bool,
    /// Whether we're inside an SVG/MathML/foreignObject subtree.
    /// HTML re-parses content in foreign-content mode, so namespace
    /// confusion can turn HTML-escaped content into executable markup
    /// (the DOMPurify bypass series; CVE review Gap E).
    in_foreign_content: bool,
}

impl<'a> AncestorCtx<'a> {
    fn root() -> Self {
        Self {
            stack: &[],
            in_script_or_style: false,
            in_raw_text_element: false,
            in_foreign_content: false,
        }
    }

    /// The immediate parent tag, if any.
    fn parent(&self) -> Option<&str> {
        self.stack.last().map(|s| s.as_str())
    }

    /// Whether any ancestor matches the given tag name.
    fn has_ancestor(&self, tag: &str) -> bool {
        self.stack.iter().any(|t| t == tag)
    }

    /// Whether any ancestor matches any of the given tag names.
    fn has_ancestor_any(&self, tags: &[&str]) -> bool {
        tags.iter().any(|t| self.has_ancestor(t))
    }
}

/// Raw-text / escapable-raw-text elements per HTML5 §13.2.5. Dynamic
/// interpolation inside these needs the same care as `<script>`/`<style>`:
/// HTML escaping is sufficient for RCDATA (`<textarea>`, `<title>`) but not
/// for the raw-text variants (`<xmp>`, `<noscript>`, `<noframes>`) where
/// entities are not decoded — the user would see literal `&lt;` in the output.
fn is_raw_text_element(tag: &str) -> bool {
    // `<title>` is ordinary RCDATA: the normal text writer escapes `<` and
    // `&`, so dynamic titles need no warning or custom render component.
    matches!(tag, "textarea" | "xmp" | "noscript" | "noframes")
}

/// Foreign content containers. Content inside is parsed in the SVG or MathML
/// namespace where browser parser quirks have historically enabled mutation
/// XSS bypasses (see DOMPurify bypass series referenced in CVE review Gap E).
fn is_foreign_content_root(tag: &str) -> bool {
    matches!(tag, "svg" | "math" | "foreignObject")
}

/// Validate security, a11y, and performance rules.
/// Returns `Err(errors)` if hard errors are found (compilation fails).
/// Returns `Ok(warnings)` with a TokenStream of `#[must_use]` warnings to be
/// included in the macro output (compilation succeeds but shows warnings).
pub fn validate_security(root: &HtmlRoot) -> Result<TokenStream, TokenStream> {
    let Validation { errors, warnings } = validate(root);
    if !errors.is_empty() {
        let compile_errors: Vec<TokenStream> =
            errors.iter().map(|e| e.to_compile_error()).collect();
        Err(quote! { #(#compile_errors)* })
    } else if warnings.is_empty() {
        Ok(quote! {})
    } else {
        let warnings: Vec<TokenStream> = warnings.iter().map(Warning::into_tokens).collect();
        Ok(quote! { #(#warnings)* })
    }
}

/// Structured result of the full validation pass (security, a11y,
/// content model, perf hints): errors abort, warnings advise. This is
/// the non-macro entry point; [`validate_security`] materializes the
/// same data as macro tokens.
#[derive(Default)]
pub struct Validation {
    pub errors: Vec<syn::Error>,
    pub warnings: Vec<Warning>,
}

/// A validation result produced under an aggregate diagnostic work budget.
///
/// `limit_reached` means validation stopped as soon as the configured number
/// of errors plus warnings had been produced. Callers must treat that as a
/// failed/incomplete analysis rather than accepting the partial result.
pub struct BoundedValidation {
    pub validation: Validation,
    pub limit_reached: bool,
}

struct BoundedDiagnostics<'budget, T> {
    values: Vec<T>,
    remaining: &'budget Cell<Option<usize>>,
}

impl<'budget, T> BoundedDiagnostics<'budget, T> {
    fn new(remaining: &'budget Cell<Option<usize>>) -> Self {
        Self {
            values: Vec::new(),
            remaining,
        }
    }

    fn push(&mut self, value: T) {
        match self.remaining.get() {
            Some(0) => {}
            Some(remaining) => {
                self.values.push(value);
                self.remaining.set(Some(remaining - 1));
            }
            None => self.values.push(value),
        }
    }

    fn exhausted(&self) -> bool {
        self.remaining.get() == Some(0)
    }

    fn into_vec(self) -> Vec<T> {
        self.values
    }
}

/// Run the full validation pass and return structured errors and
/// warnings.
pub fn validate(root: &HtmlRoot) -> Validation {
    validate_with_limit(root, None).validation
}

/// Run validation while admitting at most `maximum` aggregate diagnostics.
///
/// Unlike truncating [`validate`] after the fact, this budget is cooperative:
/// recursive validation walks stop immediately when the last slot is used.
pub fn validate_bounded(root: &HtmlRoot, maximum: usize) -> BoundedValidation {
    validate_with_limit(root, Some(maximum))
}

fn validate_with_limit(root: &HtmlRoot, maximum: Option<usize>) -> BoundedValidation {
    let remaining = Cell::new(maximum);
    let mut errors = BoundedDiagnostics::new(&remaining);
    let mut warnings = BoundedDiagnostics::new(&remaining);
    let ctx = AncestorCtx::root();
    for node in &root.nodes {
        if errors.exhausted() {
            break;
        }
        validate_node(node, &ctx, &mut errors, &mut warnings);
    }
    BoundedValidation {
        validation: Validation {
            errors: errors.into_vec(),
            warnings: warnings.into_vec(),
        },
        limit_reached: remaining.get() == Some(0),
    }
}

/// Validate XML-specific restrictions that can be known at macro expansion
/// time. HTML-only a11y/content-model checks intentionally do not run for
/// `xml!`, because XML vocabularies are application-defined.
pub fn validate_xml(root: &HtmlRoot) -> Result<TokenStream, TokenStream> {
    let mut errors = Vec::new();
    for node in &root.nodes {
        validate_xml_node(node, &mut errors);
    }

    if errors.is_empty() {
        Ok(quote! {})
    } else {
        let compile_errors: Vec<TokenStream> =
            errors.iter().map(|e| e.to_compile_error()).collect();
        Err(quote! { #(#compile_errors)* })
    }
}

fn validate_xml_node(node: &Node, errors: &mut Vec<syn::Error>) {
    match node {
        Node::Text(lit) => {
            if lit.value().contains('\0') {
                errors.push(syn::Error::new(
                    lit.span(),
                    "xml!: static text contains U+0000, which is forbidden in XML output",
                ));
            }
        }
        Node::Element(el) => {
            for attr in &el.attrs {
                if let Attribute::Static { value, .. } = attr {
                    if value.value().contains('\0') {
                        errors.push(syn::Error::new(
                            value.span(),
                            "xml!: static attribute value contains U+0000, which is forbidden in XML output",
                        ));
                    }
                }
            }
            for child in &el.children {
                validate_xml_node(child, errors);
            }
        }
        Node::Fragment(children) => {
            for child in children {
                validate_xml_node(child, errors);
            }
        }
        Node::Block(_) | Node::Doctype(_) | Node::XmlDeclaration(_) => {}
    }
}

fn validate_node(
    node: &Node,
    ctx: &AncestorCtx<'_>,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if errors.exhausted() {
        return;
    }
    match node {
        Node::Element(el) => validate_element(el, ctx, errors, warnings),
        Node::Fragment(children) => {
            for child in children {
                if errors.exhausted() {
                    break;
                }
                validate_node(child, ctx, errors, warnings);
            }
        }
        Node::Block(expr) => {
            validate_block_expr(expr, ctx, warnings);
        }
        _ => {}
    }
}

// proa-perf-ok(scope): compile-time diagnostic messages quote markup; nothing renders here
fn validate_block_expr(
    expr: &Expr,
    ctx: &AncestorCtx<'_>,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if warnings.exhausted() {
        return;
    }
    let expr_text = quote::quote!(#expr).to_string();
    let is_raw = expr_text.starts_with("StaticRaw") || expr_text.starts_with("UnsafeRaw");
    let is_unsafe_raw = expr_text.starts_with("UnsafeRaw");
    let ancestor_tag = ctx.stack.last().map(|s| s.as_str()).unwrap_or("unknown");
    let expr_short = if expr_text.len() > 60 {
        format!("{}...", &expr_text[..57])
    } else {
        expr_text.clone()
    };
    let span = match expr {
        Expr::Path(p) => p
            .path
            .segments
            .first()
            .map_or_else(proc_macro2::Span::call_site, |seg| seg.ident.span()),
        _ => proc_macro2::Span::call_site(),
    };

    // OWASP XSS Prevention, Rules #3 (JavaScript) & #4 (CSS):
    // HTML-context escaping is insufficient inside <script>/<style>.
    // These are distinct encoding contexts that require JS/CSS-specific
    // encoding, which Proa (an HTML engine) does not provide.
    //
    // Skip StaticRaw/UnsafeRaw — these are pre-trusted raw content by design.
    if ctx.in_script_or_style && !is_raw {
        warnings.push(emit_warning(
            span,
            "OWASP XSS: dynamic expr in script/style",
            &format!(
                "OWASP XSS: dynamic expression `{expr_short}` inside <{ancestor_tag}>. \
                 HTML escaping does NOT prevent injection in JS/CSS contexts. \
                 Prefer data-* attributes with JSON serialization."
            ),
        ));
        if warnings.exhausted() {
            return;
        }
    }

    // CVE review Gap B — raw-text and escapable-raw-text elements.
    //
    // `<textarea>` / `<title>` are RCDATA: standard HTML escape is sufficient
    // because `<` → `&lt;` prevents any `</textarea>` bytes from appearing.
    // Title interpolation uses normal escaping without a warning. For textarea
    // we still surface an advisory because initial values should
    // usually come from a typed `value=` attribute path, not a free-form text
    // child node. For `<xmp>` / `<noscript>` / `<noframes>` (raw text, no
    // entity decoding) the escape leaves literal `&lt;` in the output, which
    // is not an XSS but is a rendering bug worth surfacing.
    if ctx.in_raw_text_element
        && !ctx.in_script_or_style
        && !is_raw
        && !is_raw_text_suppressed(expr)
    {
        warnings.push(emit_warning(
            span,
            "raw-text element dynamic interpolation",
            &format!(
                "dynamic expression `{expr_short}` inside raw-text element <{ancestor_tag}>. \
                 HTML escape prevents tag breakout in RCDATA (<textarea>, <title>), but \
                 raw-text elements (<xmp>, <noscript>, <noframes>) do not decode entities \
                 so the output will contain literal `&lt;`/`&amp;`. Prefer a typed initial \
                 value (e.g. the `value` attribute) or pre-render the text outside the \
                 element (HTML5 §13.2.5.4)"
            ),
        ));
        if warnings.exhausted() {
            return;
        }
    }

    // CVE review Gap E — mXSS via namespace confusion in SVG/MathML.
    //
    // DOMPurify bypass series (2019–2020) demonstrated that HTML-escaped
    // content inside `<svg>` / `<math>` / `<foreignObject>` can become
    // executable after the browser re-parses it in foreign-content mode
    // (MathML `<mglyph>`, SVG `<annotation-xml>`, namespace flips). This is
    // only reachable via raw-HTML passthrough — warn there.
    if ctx.in_foreign_content && is_unsafe_raw && !is_foreign_content_suppressed(expr) {
        warnings.push(emit_warning(
            span,
            "mXSS: UnsafeRaw in foreign content",
            &format!(
                "UnsafeRaw `{expr_short}` inside foreign content <{ancestor_tag}>. \
                 Browsers re-parse SVG/MathML content in a different namespace, so \
                 HTML that looks inert in HTML context can become executable after \
                 a namespace flip (DOMPurify bypass series, 2019–2020). Sanitize the \
                 content with a SVG/MathML-aware sanitizer before passing to UnsafeRaw, \
                 or move the subtree out of the foreign-content container"
            ),
        ));
    }
}

fn is_named_wrapper_call(expr: &Expr, wrapper_name: &str) -> bool {
    if let Expr::Call(call) = expr {
        if let Expr::Path(path) = call.func.as_ref() {
            if let Some(seg) = path.path.segments.last() {
                return seg.ident == wrapper_name;
            }
        }
    }
    false
}

fn is_raw_text_suppressed(expr: &Expr) -> bool {
    is_named_wrapper_call(expr, "raw_text_ok")
}

fn is_foreign_content_suppressed(expr: &Expr) -> bool {
    is_named_wrapper_call(expr, "foreign_content_ok")
}

fn is_dynamic_style_suppressed(expr: &Expr) -> bool {
    is_named_wrapper_call(expr, "style_ok")
}

// ---------------------------------------------------------------------------
// Element classification helpers for content model validation
// ---------------------------------------------------------------------------

/// Elements that only accept phrasing content (inline) — no block-level children.
fn is_phrasing_only_parent(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "span"
            | "em"
            | "strong"
            | "small"
            | "s"
            | "cite"
            | "q"
            | "dfn"
            | "abbr"
            | "code"
            | "var"
            | "samp"
            | "kbd"
            | "sub"
            | "sup"
            | "i"
            | "b"
            | "u"
            | "mark"
            | "bdi"
            | "bdo"
            | "label"
            | "output"
            | "time"
            | "data"
            | "progress"
            | "meter"
    )
}

/// Flow content elements (block-level) that are NOT valid inside phrasing-only parents.
fn is_block_level(tag: &str) -> bool {
    matches!(
        tag,
        "div"
            | "p"
            | "section"
            | "article"
            | "aside"
            | "header"
            | "footer"
            | "nav"
            | "main"
            | "table"
            | "ul"
            | "ol"
            | "dl"
            | "blockquote"
            | "pre"
            | "form"
            | "fieldset"
            | "details"
            | "dialog"
            | "figure"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "address"
            | "search"
            | "menu"
            | "hgroup"
    )
}

/// Always-interactive elements (HTML §3.2.5.2.7).
fn is_always_interactive(tag: &str) -> bool {
    matches!(
        tag,
        "button" | "select" | "textarea" | "label" | "details" | "embed" | "iframe"
    )
}

/// Check if an element is interactive based on tag + attributes.
fn is_interactive(tag: &str, attrs: &ElementAttrs) -> bool {
    if is_always_interactive(tag) {
        return true;
    }
    match tag {
        "a" => attrs.has("href"),
        "input" => attrs.static_value("type") != Some("hidden"),
        "audio" | "video" => attrs.has("controls"),
        "img" => attrs.has("usemap"),
        _ => false,
    }
}

fn is_heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn is_sectioning(tag: &str) -> bool {
    matches!(tag, "article" | "aside" | "nav" | "section")
}

/// Collect attribute names (normalized: underscores → dashes, lowercased) from
/// an element. Returns a set of attribute names present, plus a lookup for
/// static attribute values (needed for conditional checks like `type="image"`).
struct ElementAttrs {
    /// All attribute names present on the element (static, dynamic, or boolean).
    names: Vec<String>,
    /// Static attribute values keyed by normalized name.
    static_values: Vec<(String, String)>,
}

impl ElementAttrs {
    fn from_element(el: &Element) -> Self {
        let mut names = Vec::new();
        let mut static_values = Vec::new();
        for attr in &el.attrs {
            match attr {
                Attribute::Static { name, value } => {
                    let n = name.to_string().replace('_', "-");
                    static_values.push((n.clone(), value.value()));
                    names.push(n);
                }
                Attribute::Dynamic { name, .. } => {
                    names.push(name.to_string().replace('_', "-"));
                }
                Attribute::Bool { name } => {
                    names.push(name.to_string().replace('_', "-"));
                }
            }
        }
        Self {
            names,
            static_values,
        }
    }

    fn has(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    fn has_any(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.has(n))
    }

    fn static_value(&self, name: &str) -> Option<&str> {
        self.static_values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn attr_name_span(attr: &Attribute) -> (String, proc_macro2::Span) {
    match attr {
        Attribute::Static { name, .. }
        | Attribute::Dynamic { name, .. }
        | Attribute::Bool { name } => (name.to_string().replace('_', "-"), name.span()),
    }
}

fn html_href_allowed_on_tag(tag: &str) -> bool {
    matches!(tag, "a" | "area" | "base" | "link") || tag.contains('-')
}

fn attr_requires_value(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "action"
            | "alt"
            | "autocomplete"
            | "charset"
            | "cite"
            | "class"
            | "content"
            | "data"
            | "dir"
            | "form"
            | "height"
            | "href"
            | "id"
            | "name"
            | "poster"
            | "rel"
            | "src"
            | "srcset"
            | "style"
            | "target"
            | "title"
            | "type"
            | "value"
            | "width"
    ) || name.starts_with("aria-")
}

fn validate_attribute_semantics(
    tag: &str,
    el: &Element,
    in_foreign_content: bool,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
) {
    for attr in &el.attrs {
        if errors.exhausted() {
            break;
        }
        let (attr_name, attr_span) = attr_name_span(attr);

        if attr_name == "href" && !in_foreign_content && !html_href_allowed_on_tag(tag) {
            errors.push(syn::Error::new(
                attr_span,
                diagnostic(
                    &format!("`href` is not valid on <{tag}>"),
                    "`href` only creates links on hyperlink-capable HTML elements",
                    "use <a href=\"...\"> for links, or move the URL into a data-* attribute",
                    "HTML §4.5.1, §4.8.3",
                ),
            ));
        }

        if matches!(attr, Attribute::Bool { .. }) && attr_requires_value(&attr_name) {
            errors.push(syn::Error::new(
                attr_span,
                diagnostic(
                    &format!("`{attr_name}` requires a value"),
                    "this attribute is not a boolean attribute, so writing it without `=...` does not produce valid HTML",
                    &format!("write {attr_name}=\"...\" or {attr_name}={{expr}}"),
                    "HTML §2.3.2",
                ),
            ));
        }
    }
}

fn validate_element(
    el: &Element,
    ctx: &AncestorCtx<'_>,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if errors.exhausted() {
        return;
    }
    let tag = el.tag_str().to_lowercase();
    let is_script_or_style = tag == "script" || tag == "style";
    let attrs = ElementAttrs::from_element(el);
    let span = el.name.span();

    // -----------------------------------------------------------------------
    // OWASP XSS Prevention, Rule #7: block executable URL schemes
    // -----------------------------------------------------------------------
    for attr in &el.attrs {
        if errors.exhausted() {
            return;
        }
        if let Attribute::Static { name, value } = attr {
            let attr_name = name.to_string().replace('_', "-");
            if is_url_attr_name(&attr_name) && is_dangerous_url_value(&value.value()) {
                errors.push(syn::Error::new(
                    value.span(),
                    diagnostic(
                        &format!("dangerous URL scheme in `{attr_name}` attribute"),
                        "`javascript:`, `vbscript:`, and executable `data:` URIs can execute active content on navigation, enabling Cross-Site Scripting (XSS)",
                        "use a safe scheme (https:, /, #) or pass data via data-* attributes",
                        "OWASP XSS Prevention Cheat Sheet, Rule #7",
                    ),
                ));
            }
        }
    }

    // -----------------------------------------------------------------------
    // CVE review Gap C — dynamic `srcdoc=` / `style=` values are second-order
    // contexts: attribute-value escaping is correct for the attribute itself,
    // but `srcdoc` content re-parses as HTML inside the iframe and `style`
    // content parses as CSS. An attacker who controls the value can still
    // XSS/exfiltrate even through a correctly-escaped attribute. Warn at
    // compile time so developers pick a typed alternative.
    // -----------------------------------------------------------------------
    for attr in &el.attrs {
        if errors.exhausted() {
            return;
        }
        if let Attribute::Dynamic { name, value } = attr {
            let attr_name = name.to_string().replace('_', "-");
            let guidance = match attr_name.as_str() {
                "srcdoc" => Some(
                    "`srcdoc` is a second-order HTML context — the value re-parses as HTML \
                     when the iframe mounts, so attribute escaping does not prevent <script> \
                     or event-handler injection. Prefer StaticRaw (compile-time HTML) or a \
                     data URL for isolated content",
                ),
                "style" if !is_dynamic_style_suppressed(value) => Some(
                    "`style` is a second-order CSS context — attribute escaping does not \
                     prevent `background:url(//attacker?leak=...)` data exfiltration or CSS \
                     expression-based attacks on legacy browsers. Prefer a class + typed \
                     recipe, or inline only static style strings",
                ),
                _ => None,
            };
            if let Some(help) = guidance {
                let expr_text = quote::quote!(#value).to_string();
                let expr_short = if expr_text.len() > 60 {
                    format!("{}...", &expr_text[..57])
                } else {
                    expr_text
                };
                warnings.push(emit_warning(
                    name.span(),
                    &format!("dynamic {attr_name} attribute"),
                    &format!("dynamic `{attr_name}={{{expr_short}}}`. {help} (CVE review Gap C)"),
                ));
            }
        }
    }

    // -----------------------------------------------------------------------
    // CVE review Gap Q — dynamic inline event handlers (`onclick={expr}`,
    // `onerror={expr}`, ...) interpolate their value into a JavaScript
    // execution context. Attribute-value escaping is applied, but it does not
    // neutralize script inside an `on*` handler, so an attacker-controlled
    // value is XSS on the event. Proa cannot scheme-check arbitrary JS, so we
    // warn at the call site. The idiomatic safe path is a delegated island
    // handler (`on_click`), and a strict Content-Security-Policy that forbids
    // inline event handlers is the defense in depth. This mirrors the warning
    // already emitted for dynamic children of `<script>`/`<style>`.
    // -----------------------------------------------------------------------
    for attr in &el.attrs {
        if errors.exhausted() {
            return;
        }
        if let Attribute::Dynamic { name, value } = attr {
            let attr_name = name.to_string().replace('_', "-");
            if is_event_handler_attr_name(&attr_name) {
                let expr_text = quote::quote!(#value).to_string();
                let expr_short = if expr_text.len() > 60 {
                    format!("{}...", &expr_text[..57])
                } else {
                    expr_text
                };
                warnings.push(emit_warning(
                    name.span(),
                    &format!("dynamic {attr_name} event handler"),
                    &format!(
                        "dynamic `{attr_name}={{{expr_short}}}` injects its value into a \
                         JavaScript execution context; attribute escaping does not prevent \
                         script execution when the value is attacker-controlled. Prefer a \
                         delegated island handler (e.g. `on_click`) and enforce a strict \
                         Content-Security-Policy that disallows inline event handlers \
                         (CVE review Gap Q)"
                    ),
                ));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Attribute semantics that the parser accepts syntactically but HTML does
    // not allow for regular HTML content.
    // -----------------------------------------------------------------------
    validate_attribute_semantics(
        &tag,
        el,
        ctx.in_foreign_content || is_foreign_content_root(&tag),
        errors,
    );
    if errors.exhausted() {
        return;
    }

    // -----------------------------------------------------------------------
    // Required attribute validation (HTML Living Standard)
    // -----------------------------------------------------------------------
    validate_required_attrs(&tag, &attrs, span, errors, warnings);
    if errors.exhausted() {
        return;
    }

    // -----------------------------------------------------------------------
    // Content model nesting validation (HTML Living Standard §3.2.5)
    // -----------------------------------------------------------------------
    validate_nesting(&tag, &attrs, ctx, span, errors);
    if errors.exhausted() {
        return;
    }
    validate_child_restrictions(el, &tag, errors);
    if errors.exhausted() {
        return;
    }
    validate_child_ordering(el, &tag, errors);
    if errors.exhausted() {
        return;
    }

    // -----------------------------------------------------------------------
    // Accessibility validation (WCAG 2.2, WAI-ARIA 1.2, ARIA in HTML)
    // -----------------------------------------------------------------------
    validate_a11y(el, &tag, &attrs, span, errors, warnings);
    if errors.exhausted() {
        return;
    }

    // -----------------------------------------------------------------------
    // Formatting hints
    // -----------------------------------------------------------------------
    validate_indentation(el, &tag, warnings);
    if errors.exhausted() {
        return;
    }

    // -----------------------------------------------------------------------
    // Performance hints (detect allocations in render path)
    // -----------------------------------------------------------------------
    #[cfg(not(feature = "no-perf-hints"))]
    validate_perf_hints(el, span, warnings);
    if errors.exhausted() {
        return;
    }

    // Recurse into children with updated ancestor context
    let mut new_stack: Vec<String> = ctx.stack.to_vec();
    new_stack.push(tag.clone());
    let child_ctx = AncestorCtx {
        stack: &new_stack,
        in_script_or_style: is_script_or_style || ctx.in_script_or_style,
        in_raw_text_element: ctx.in_raw_text_element || is_raw_text_element(&tag),
        in_foreign_content: ctx.in_foreign_content || is_foreign_content_root(&tag),
    };
    for child in &el.children {
        if errors.exhausted() {
            break;
        }
        validate_node(child, &child_ctx, errors, warnings);
    }
}

fn validate_indentation(
    el: &Element,
    parent_tag: &str,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if warnings.exhausted() {
        return;
    }
    let parent_pos = el.name.start_line_column();
    let opening_end_pos = el.opening_end_line_column();
    if parent_pos.line == 0 || opening_end_pos.line == 0 {
        return;
    }

    for child in &el.children {
        if warnings.exhausted() {
            break;
        }
        validate_child_indentation(parent_tag, parent_pos, opening_end_pos, child, warnings);
    }
}

fn validate_child_indentation(
    parent_tag: &str,
    parent_pos: proc_macro2::LineColumn,
    parent_opening_end_pos: proc_macro2::LineColumn,
    child: &Node,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if warnings.exhausted() {
        return;
    }
    match child {
        Node::Element(child_el) => {
            let child_pos = child_el.name.start_line_column();
            if child_pos.line > parent_opening_end_pos.line && child_pos.column == parent_pos.column
            {
                let child_tag = child_el.tag_str();
                let parent_col = parent_pos.column + 1;
                let child_col = child_pos.column + 1;
                warnings.push(emit_warning(
                    child_el.name.span(),
                    "format: nested markup indentation",
                    &format!(
                        "format: `<{child_tag}>` starts at column {child_col}, but its parent `<{parent_tag}>` starts at column {parent_col}. Nested markup should be indented deeper than its parent. rustfmt cannot currently rewrite Proa macro bodies; adjust the indentation manually."
                    ),
                ));
            }
        }
        Node::Fragment(children) => {
            for child in children {
                if warnings.exhausted() {
                    break;
                }
                validate_child_indentation(
                    parent_tag,
                    parent_pos,
                    parent_opening_end_pos,
                    child,
                    warnings,
                );
            }
        }
        Node::Text(_) | Node::Block(_) | Node::Doctype(_) | Node::XmlDeclaration(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Category 1: Self-nesting prohibitions
// Category 3: Block-in-inline violations
// Category 4: Interactive content nesting
// ---------------------------------------------------------------------------
// proa-perf-ok(scope): compile-time diagnostic messages quote markup; nothing renders here
fn validate_nesting(
    tag: &str,
    attrs: &ElementAttrs,
    ctx: &AncestorCtx<'_>,
    span: proc_macro2::Span,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
) {
    if errors.exhausted() {
        return;
    }
    // Category 1: Self-nesting prohibitions (HTML §4.5.1, §4.10.3, §4.10.4, etc.)
    match tag {
        "a" if ctx.has_ancestor("a") => {
            errors.push(syn::Error::new(span, diagnostic(
                "<a> cannot be nested inside another <a>",
                "the browser will auto-close the outer <a> when it encounters the inner one, creating two separate links instead of a nested one — this breaks the link structure and confuses screen readers",
                "move the inner link outside, or use a single <a> wrapping the full content",
                "HTML §4.5.1",
            )));
        }
        "button" if ctx.has_ancestor("button") => {
            errors.push(syn::Error::new(span, diagnostic(
                "<button> cannot be nested inside another <button>",
                "nested buttons create ambiguous click targets — the browser cannot determine which button the user intended to activate, and screen readers cannot convey the nesting",
                "extract the inner button to be a sibling, or restructure the UI",
                "HTML §4.10.6",
            )));
        }
        "form" if ctx.has_ancestor("form") => {
            errors.push(syn::Error::new(span, diagnostic(
                "<form> cannot be nested inside another <form>",
                "the browser will ignore the inner form's start tag, so its inputs submit with the outer form instead — this causes silent data loss or incorrect submissions",
                "use a single form, or split into separate non-nested forms",
                "HTML §4.10.3",
            )));
        }
        "label" if ctx.has_ancestor("label") => {
            errors.push(syn::Error::new(span, diagnostic(
                "<label> cannot be nested inside another <label>",
                "nested labels create ambiguous associations — a click on the inner label may activate either the inner or outer control depending on the browser",
                "use separate labels, each associated with one control via `for` or wrapping",
                "HTML §4.10.4",
            )));
        }
        "header" if ctx.has_ancestor_any(&["header", "footer"]) => {
            errors.push(syn::Error::new(span, diagnostic_note(
                "<header> cannot appear inside <header> or <footer>",
                "landmark nesting creates confusing navigation for screen reader users who use landmarks to jump between page sections",
                "HTML §4.3.8",
            )));
        }
        "footer" if ctx.has_ancestor_any(&["header", "footer"]) => {
            errors.push(syn::Error::new(span, diagnostic_note(
                "<footer> cannot appear inside <header> or <footer>",
                "landmark nesting creates confusing navigation for screen reader users who use landmarks to jump between page sections",
                "HTML §4.3.9",
            )));
        }
        "address" if ctx.has_ancestor("address") => {
            errors.push(syn::Error::new(
                span,
                diagnostic_note(
                    "<address> cannot be nested inside another <address>",
                    "the spec forbids nesting <address> elements",
                    "HTML §4.3.10",
                ),
            ));
        }
        "dfn" if ctx.has_ancestor("dfn") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<dfn> cannot be nested inside another <dfn>",
                    "a term definition should not contain another definition",
                    "restructure so each <dfn> defines a single term at one level",
                    "HTML §4.5.8",
                ),
            ));
        }
        _ => {}
    }
    if errors.exhausted() {
        return;
    }

    // <address> also prohibits heading and sectioning content descendants
    if ctx.has_ancestor("address") && (is_heading(tag) || is_sectioning(tag)) {
        errors.push(syn::Error::new(span, diagnostic(
            &format!("<{tag}> cannot appear inside <address>"),
            "<address> only allows phrasing content — headings and sectioning elements are not permitted",
            &format!("move the <{tag}> outside the <address> block"),
            "HTML §4.3.10",
        )));
    }
    if errors.exhausted() {
        return;
    }

    // Category 3: Block-in-inline (HTML §3.2.5)
    // Phrasing-only parents cannot contain block-level (flow) children.
    if let Some(parent) = ctx.parent() {
        if is_phrasing_only_parent(parent) && is_block_level(tag) {
            let note = if parent == "p" {
                format!("the browser will auto-close the <{parent}> before this <{tag}>, splitting your paragraph in two — this often breaks layout and styling")
            } else {
                "this creates an invalid DOM tree that browsers handle inconsistently and screen readers may misinterpret".to_string()
            };
            errors.push(syn::Error::new(span, diagnostic(
                &format!("<{tag}> cannot be a child of <{parent}> — <{parent}> only allows phrasing (inline) content"),
                &note,
                &format!("wrap the <{tag}> in a container that accepts flow content, or replace the <{parent}> with a <div>"),
                "HTML §3.2.5",
            )));
        }
    }
    if errors.exhausted() {
        return;
    }

    // Category 4: Interactive content nesting (HTML §3.2.5.2.7)
    // Interactive elements cannot appear inside other interactive elements.
    // Exception: <label> is allowed to wrap ONE labelable element (<input>, <select>,
    // <textarea>, <button>, <meter>, <output>, <progress>) per HTML §4.10.4.
    if is_interactive(tag, attrs) {
        let is_labelable = matches!(
            tag,
            "input" | "select" | "textarea" | "button" | "meter" | "output" | "progress"
        );
        for ancestor in ctx.stack.iter() {
            if errors.exhausted() {
                break;
            }
            if is_always_interactive(ancestor) {
                // Allow labelable elements inside <label> — this is the standard
                // pattern for associating a label with its form control.
                if ancestor == "label" && is_labelable {
                    continue;
                }
                errors.push(syn::Error::new(span, diagnostic(
                    &format!("<{tag}> cannot appear inside <{ancestor}> — interactive content cannot be nested"),
                    "the browser cannot determine which element should receive the user's interaction, and screen readers cannot convey the nesting",
                    &format!("move the <{tag}> outside the <{ancestor}>, or restructure the UI"),
                    "HTML §3.2.5.2.7",
                )));
                break; // Only report the innermost violation
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Category 2: Restricted children
// ---------------------------------------------------------------------------
fn validate_child_restrictions(
    el: &Element,
    tag: &str,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
) {
    if errors.exhausted() {
        return;
    }
    let allowed: Option<&[&str]> = match tag {
        "ul" | "ol" | "menu" => Some(&["li", "script", "template"]),
        "dl" => Some(&["dt", "dd", "div", "script", "template"]),
        "table" => Some(&[
            "caption", "colgroup", "thead", "tbody", "tfoot", "tr", "script", "template",
        ]),
        "thead" | "tbody" | "tfoot" => Some(&["tr", "script", "template"]),
        "tr" => Some(&["td", "th", "script", "template"]),
        "colgroup" => Some(&["col", "template"]),
        "select" => Some(&["option", "optgroup", "hr", "script", "template"]),
        "optgroup" => Some(&["option", "script", "template"]),
        "picture" => Some(&["source", "img", "script", "template"]),
        "hgroup" => Some(&[
            "h1", "h2", "h3", "h4", "h5", "h6", "p", "script", "template",
        ]),
        "head" => Some(&[
            "title", "base", "link", "meta", "style", "script", "noscript", "template",
        ]),
        _ => None,
    };

    if let Some(allowed_children) = allowed {
        for child in &el.children {
            if errors.exhausted() {
                break;
            }
            if let Node::Element(child_el) = child {
                let child_tag = child_el.tag_str().to_lowercase();
                if !allowed_children.contains(&child_tag.as_str()) {
                    let allowed_list = allowed_children
                        .iter()
                        .filter(|c| **c != "script" && **c != "template")
                        .map(|c| format!("<{c}>"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    errors.push(syn::Error::new(child_el.name.span(), diagnostic(
                        &format!("<{child_tag}> is not a valid child of <{tag}>"),
                        &format!("the browser may move this element outside <{tag}> or ignore it, producing a DOM tree that doesn't match your template"),
                        &format!("<{tag}> only accepts: {allowed_list}"),
                        "HTML §3.2.5",
                    )));
                }
            }
            // Node::Block and Node::Text are skipped — we can't validate dynamic
            // children or text nodes in restricted-child parents at compile time.
        }
    }
}

// ---------------------------------------------------------------------------
// Category 5: Child ordering constraints
// ---------------------------------------------------------------------------
fn validate_child_ordering(
    el: &Element,
    tag: &str,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
) {
    if errors.exhausted() {
        return;
    }
    let static_children: Vec<(usize, String, proc_macro2::Span)> = el
        .children
        .iter()
        .enumerate()
        .filter_map(|(i, child)| {
            if let Node::Element(child_el) = child {
                Some((i, child_el.tag_str().to_lowercase(), child_el.name.span()))
            } else {
                None
            }
        })
        .collect();

    if static_children.is_empty() {
        return;
    }

    match tag {
        // <table>: <caption> must be first; <colgroup> before row groups
        "table" => {
            for (i, (_, child_tag, child_span)) in static_children.iter().enumerate() {
                if errors.exhausted() {
                    return;
                }
                if child_tag == "caption" && i != 0 {
                    errors.push(syn::Error::new(*child_span, diagnostic(
                        "<caption> must be the first child of <table>",
                        "placing it after other table content may cause it to be ignored or mispositioned",
                        "move the <caption> to be the first element inside <table>",
                        "HTML §4.9.1",
                    )));
                }
            }
            let mut seen_row_content = false;
            for (_, child_tag, child_span) in &static_children {
                if errors.exhausted() {
                    return;
                }
                if matches!(child_tag.as_str(), "thead" | "tbody" | "tfoot" | "tr") {
                    seen_row_content = true;
                }
                if child_tag == "colgroup" && seen_row_content {
                    errors.push(syn::Error::new(*child_span, diagnostic(
                        "<colgroup> must appear before row content in <table>",
                        "column groups defined after <thead>/<tbody>/<tfoot>/<tr> may not be applied",
                        "move the <colgroup> before the first <thead>, <tbody>, <tfoot>, or <tr>",
                        "HTML §4.9.1",
                    )));
                }
            }
        }

        "fieldset" => {
            for (i, (_, child_tag, child_span)) in static_children.iter().enumerate() {
                if errors.exhausted() {
                    return;
                }
                if child_tag == "legend" && i != 0 {
                    errors.push(syn::Error::new(*child_span, diagnostic(
                        "<legend> must be the first child of <fieldset>",
                        "browsers only render <legend> as the fieldset's caption when it's the first child — placing it elsewhere makes it render as normal content and the fieldset loses its accessible label",
                        "move the <legend> to be the first element inside <fieldset>",
                        "HTML §4.10.15",
                    )));
                }
            }
        }

        "details" => {
            for (i, (_, child_tag, child_span)) in static_children.iter().enumerate() {
                if errors.exhausted() {
                    return;
                }
                if child_tag == "summary" && i != 0 {
                    errors.push(syn::Error::new(*child_span, diagnostic(
                        "<summary> must be the first child of <details>",
                        "browsers only render <summary> as the disclosure toggle when it's the first child — otherwise a default \"Details\" toggle is generated and your <summary> renders as regular content",
                        "move the <summary> to be the first element inside <details>",
                        "HTML §4.11.1",
                    )));
                }
            }
        }

        "figure" => {
            let last_idx = static_children.len() - 1;
            for (i, (_, child_tag, child_span)) in static_children.iter().enumerate() {
                if errors.exhausted() {
                    return;
                }
                if child_tag == "figcaption" && i != 0 && i != last_idx {
                    errors.push(syn::Error::new(*child_span, diagnostic(
                        "<figcaption> must be the first or last child of <figure>",
                        "assistive technology expects the caption at the start or end — placing it in the middle may break the association with the figure's content",
                        "move the <figcaption> to the first or last position inside <figure>",
                        "HTML §4.4.11",
                    )));
                }
            }
        }

        _ => {}
    }
}

/// Validate required attributes per the HTML Living Standard.
///
/// Each check references its spec section and follows the error message quality
/// standard: what's wrong, why it matters, how to fix, correct span.
fn validate_required_attrs(
    tag: &str,
    attrs: &ElementAttrs,
    span: proc_macro2::Span,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if errors.exhausted() {
        return;
    }
    match tag {
        // -----------------------------------------------------------------
        // <img> — requires (src or srcset) AND alt
        // HTML §4.8.3: https://html.spec.whatwg.org/multipage/embedded-content.html#the-img-element
        // WCAG 1.1.1: https://www.w3.org/TR/WCAG22/#non-text-content
        // -----------------------------------------------------------------
        "img" => {
            if !attrs.has_any(&["src", "srcset"]) {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<img> is missing `src` or `srcset` attribute",
                        "without it the browser renders a broken image icon",
                        "add src=\"/path/to/image.jpg\" or srcset=\"...\" with responsive sources",
                        "HTML §4.8.3",
                    ),
                ));
            }
            if !attrs.has("alt") {
                // Warning-level: decorative images may intentionally omit alt (though alt="" is preferred).
                warnings.push(emit_warning(
                    span,
                    "a11y: img missing alt attribute",
                    &diagnostic(
                        "a11y: <img> is missing `alt` attribute",
                        "screen readers cannot describe this image without alt text",
                        "add alt=\"description\" for informative images, or alt=\"\" for decorative images",
                        "WCAG 1.1.1, HTML §4.8.3",
                    ),
                ));
            }
        }

        // -----------------------------------------------------------------
        // <link> — requires (href or imagesrcset) AND (rel or itemprop)
        // HTML §4.2.4: https://html.spec.whatwg.org/multipage/semantics.html#the-link-element
        // -----------------------------------------------------------------
        "link" => {
            if !attrs.has_any(&["href", "imagesrcset"]) {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<link> is missing `href` or `imagesrcset`",
                        "without it the browser ignores the element entirely",
                        "add href=\"/path/to/resource\"",
                        "HTML §4.2.4",
                    ),
                ));
            }
            if !attrs.has_any(&["rel", "itemprop"]) {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<link> is missing `rel` attribute",
                        "without `rel` the browser doesn't know the relationship, so the element has no effect",
                        "add rel=\"stylesheet\", rel=\"icon\", rel=\"preload\", etc.",
                        "HTML §4.2.4",
                    ),
                ));
            }
        }

        // -----------------------------------------------------------------
        // <meta> — requires one of (name, http-equiv, charset, itemprop, property)
        //          AND content when name/http-equiv/itemprop/property is used
        // HTML §4.2.5: https://html.spec.whatwg.org/multipage/semantics.html#the-meta-element
        // HTML+RDFa §4: https://www.w3.org/TR/html-rdfa/#extensions-to-the-html5-syntax
        // With property, name/http-equiv/charset are not required; content is required.
        // Open Graph uses this HTML+RDFa extension: https://ogp.me/
        // -----------------------------------------------------------------
        "meta" => {
            let has_discriminant =
                attrs.has_any(&["name", "http-equiv", "charset", "itemprop", "property"]);
            if !has_discriminant {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<meta> is missing a required attribute",
                        "must have one of: name, http-equiv, charset, itemprop, or property — without one the element has no effect",
                        "example: <meta name=\"description\" content=\"...\" /> or <meta property=\"og:title\" content=\"...\" />",
                        "HTML §4.2.5",
                    ),
                ));
            }
            if attrs.has_any(&["name", "http-equiv", "itemprop", "property"])
                && !attrs.has("content")
            {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<meta> is missing the `content` attribute",
                        "the content attribute provides the metadata value — without it the element has no effect",
                        "add content=\"...\" with the appropriate value",
                        "HTML §4.2.5",
                    ),
                ));
            }
        }

        // -----------------------------------------------------------------
        // <base> — requires href or target (at least one)
        // HTML §4.2.3: https://html.spec.whatwg.org/multipage/semantics.html#the-base-element
        // -----------------------------------------------------------------
        "base" if !attrs.has_any(&["href", "target"]) => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<base> is missing `href` or `target`",
                    "without either attribute the element has no effect",
                    "add href=\"/\" or target=\"_blank\"",
                    "HTML §4.2.3",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <bdo> — requires dir
        // HTML §4.5.25: https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-bdo-element
        // -----------------------------------------------------------------
        "bdo" if !attrs.has("dir") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<bdo> is missing the `dir` attribute",
                    "the override direction is undefined — the element is meaningless without dir",
                    "add dir=\"ltr\" or dir=\"rtl\"",
                    "HTML §4.5.25",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <map> — requires name
        // HTML §4.8.14: https://html.spec.whatwg.org/multipage/image-maps.html#the-map-element
        // -----------------------------------------------------------------
        "map" if !attrs.has("name") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<map> is missing the `name` attribute",
                    "no image can reference this map without a name",
                    "add name=\"my-map\" and use <img usemap=\"#my-map\">",
                    "HTML §4.8.14",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <track> — requires src
        // HTML §4.8.12: https://html.spec.whatwg.org/multipage/media.html#the-track-element
        // -----------------------------------------------------------------
        "track" if !attrs.has("src") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<track> is missing the `src` attribute",
                    "without it the browser has no track data to load",
                    "add src=\"/captions/en.vtt\" pointing to a WebVTT file",
                    "HTML §4.8.12",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <data> — requires value
        // HTML §4.5.13: https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-data-element
        // -----------------------------------------------------------------
        "data" if !attrs.has("value") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<data> is missing the `value` attribute",
                    "without value it has no semantic benefit over <span>",
                    "add value=\"...\" with the machine-readable representation",
                    "HTML §4.5.13",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <object> — requires data
        // HTML §4.8.7: https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element
        // -----------------------------------------------------------------
        "object" if !attrs.has("data") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<object> is missing the `data` attribute",
                    "without it the browser has nothing to load",
                    "add data=\"/path/to/resource\"",
                    "HTML §4.8.7",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <area> — requires alt when href is present
        // HTML §4.8.15: https://html.spec.whatwg.org/multipage/image-maps.html#the-area-element
        // WCAG 1.1.1
        // -----------------------------------------------------------------
        "area" if attrs.has("href") && !attrs.has("alt") => {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "<area> with `href` is missing the `alt` attribute",
                    "screen reader users have no way to know what the link does without alt",
                    "add alt=\"description of the linked area\"",
                    "HTML §4.8.15, WCAG 1.1.1",
                ),
            ));
        }

        // -----------------------------------------------------------------
        // <input type="image"> — requires src AND alt
        // HTML §4.10.5.1.19
        // WCAG 1.1.1
        // -----------------------------------------------------------------
        "input" if attrs.static_value("type") == Some("image") => {
            if !attrs.has("src") {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<input type=\"image\"> is missing the `src` attribute",
                        "the browser renders a broken image icon as the submit button",
                        "add src=\"/path/to/button-image.png\"",
                        "HTML §4.10.5.1.19",
                    ),
                ));
            }
            if !attrs.has("alt") {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        "<input type=\"image\"> is missing the `alt` attribute",
                        "assistive technology cannot convey the button's purpose without alt",
                        "add alt=\"Submit\" or alt=\"Search\" describing the action",
                        "HTML §4.10.5.1.19, WCAG 1.1.1",
                    ),
                ));
            }
        }

        _ => {}
    }
}

// ===========================================================================
// Accessibility validation (WCAG 2.2, WAI-ARIA 1.2, ARIA in HTML)
// ===========================================================================

/// All valid non-abstract ARIA 1.2 roles.
const VALID_ARIA_ROLES: &[&str] = &[
    // Widget roles
    "alert",
    "alertdialog",
    "button",
    "checkbox",
    "combobox",
    "dialog",
    "grid",
    "gridcell",
    "link",
    "listbox",
    "log",
    "marquee",
    "menu",
    "menubar",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "progressbar",
    "radio",
    "radiogroup",
    "scrollbar",
    "searchbox",
    "separator",
    "slider",
    "spinbutton",
    "status",
    "switch",
    "tab",
    "tablist",
    "tabpanel",
    "textbox",
    "timer",
    "toolbar",
    "tooltip",
    "tree",
    "treegrid",
    "treeitem",
    // Document structure roles
    "application",
    "article",
    "blockquote",
    "caption",
    "cell",
    "code",
    "columnheader",
    "definition",
    "deletion",
    "directory",
    "document",
    "emphasis",
    "feed",
    "figure",
    "generic",
    "group",
    "heading",
    "img",
    "insertion",
    "list",
    "listitem",
    "math",
    "meter",
    "none",
    "note",
    "paragraph",
    "presentation",
    "row",
    "rowgroup",
    "rowheader",
    "strong",
    "subscript",
    "superscript",
    "table",
    "term",
    "time",
    // Landmark roles
    "banner",
    "complementary",
    "contentinfo",
    "form",
    "main",
    "navigation",
    "region",
    "search",
];

/// Abstract ARIA roles that must not be used directly in content.
const ABSTRACT_ARIA_ROLES: &[&str] = &[
    "command",
    "composite",
    "input",
    "landmark",
    "range",
    "roletype",
    "section",
    "sectionhead",
    "select",
    "structure",
    "widget",
    "window",
];

/// All valid ARIA 1.2 attribute names (without the "aria-" prefix).
const VALID_ARIA_ATTRS: &[&str] = &[
    // Global
    "atomic",
    "braillelabel",
    "brailleroledescription",
    "busy",
    "controls",
    "current",
    "describedby",
    "description",
    "details",
    "disabled",
    "dropeffect",
    "errormessage",
    "flowto",
    "grabbed",
    "haspopup",
    "hidden",
    "invalid",
    "keyshortcuts",
    "label",
    "labelledby",
    "live",
    "owns",
    "relevant",
    "roledescription",
    // Widget
    "autocomplete",
    "checked",
    "expanded",
    "level",
    "modal",
    "multiline",
    "multiselectable",
    "orientation",
    "placeholder",
    "pressed",
    "readonly",
    "required",
    "selected",
    "sort",
    "valuemax",
    "valuemin",
    "valuenow",
    "valuetext",
    // Relationship
    "activedescendant",
    "colcount",
    "colindex",
    "colspan",
    "posinset",
    "rowcount",
    "rowindex",
    "rowspan",
    "setsize",
];

/// Elements that prohibit all ARIA roles and attributes.
const ARIA_PROHIBITED_ELEMENTS: &[&str] = &["meta", "head", "style", "script", "col", "colgroup"];

/// Required ARIA attributes per role. Returns None if the role has no required attrs.
fn required_aria_attrs_for_role(role: &str) -> Option<&'static [&'static str]> {
    match role {
        "checkbox" | "menuitemcheckbox" | "menuitemradio" | "radio" | "switch" => {
            Some(&["aria-checked"])
        }
        "combobox" => Some(&["aria-controls", "aria-expanded"]),
        "heading" => Some(&["aria-level"]),
        "meter" | "scrollbar" | "slider" | "spinbutton" => Some(&["aria-valuenow"]),
        // separator only requires aria-valuenow when focusable — handled separately
        "option" => Some(&["aria-selected"]),
        _ => None,
    }
}

/// ARIA attributes with enumerated values. Returns None if the attr is free-form.
fn valid_aria_values(attr: &str) -> Option<&'static [&'static str]> {
    match attr {
        "autocomplete" => Some(&["inline", "list", "both", "none"]),
        "checked" => Some(&["true", "false", "mixed"]),
        "current" => Some(&["page", "step", "location", "date", "time", "true", "false"]),
        "dropeffect" => Some(&["copy", "execute", "link", "move", "none", "popup"]),
        "expanded" => Some(&["true", "false"]),
        "haspopup" => Some(&["true", "false", "menu", "listbox", "tree", "grid", "dialog"]),
        "hidden" => Some(&["true", "false"]),
        "invalid" => Some(&["true", "false", "grammar", "spelling"]),
        "live" => Some(&["assertive", "off", "polite"]),
        "modal" => Some(&["true", "false"]),
        "multiline" => Some(&["true", "false"]),
        "multiselectable" => Some(&["true", "false"]),
        "orientation" => Some(&["horizontal", "vertical", "undefined"]),
        "pressed" => Some(&["true", "false", "mixed"]),
        "readonly" => Some(&["true", "false"]),
        "relevant" => Some(&["additions", "additions text", "all", "removals", "text"]),
        "required" => Some(&["true", "false"]),
        "selected" => Some(&["true", "false"]),
        "sort" => Some(&["ascending", "descending", "none", "other"]),
        _ => None,
    }
}

/// Implicit ARIA role for an element (simplified — doesn't handle all conditional cases).
fn implicit_role(tag: &str, attrs: &ElementAttrs) -> Option<&'static str> {
    match tag {
        "a" if attrs.has("href") => Some("link"),
        "article" => Some("article"),
        "aside" => Some("complementary"),
        "button" => Some("button"),
        "details" => Some("group"),
        "dialog" => Some("dialog"),
        "form" => Some("form"),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some("heading"),
        "header" => Some("banner"),
        "footer" => Some("contentinfo"),
        "hr" => Some("separator"),
        "li" => Some("listitem"),
        "main" => Some("main"),
        "meter" => Some("meter"),
        "nav" => Some("navigation"),
        "ol" | "ul" => Some("list"),
        "option" => Some("option"),
        "output" => Some("status"),
        "progress" => Some("progressbar"),
        "search" => Some("search"),
        "select" => Some("combobox"),
        "summary" => Some("button"),
        "table" => Some("table"),
        "tbody" | "thead" | "tfoot" => Some("rowgroup"),
        "td" => Some("cell"),
        "textarea" => Some("textbox"),
        "th" => Some("columnheader"),
        "tr" => Some("row"),
        "img" => match attrs.static_value("alt") {
            Some("") => Some("presentation"),
            Some(_) => Some("img"),
            None => None,
        },
        "input" => {
            match attrs.static_value("type") {
                Some("button") | Some("image") | Some("reset") | Some("submit") => Some("button"),
                Some("checkbox") => Some("checkbox"),
                Some("radio") => Some("radio"),
                Some("range") => Some("slider"),
                Some("number") => Some("spinbutton"),
                Some("search") => Some("searchbox"),
                Some("email") | Some("tel") | Some("text") | Some("url") | None => Some("textbox"),
                Some("hidden") => None, // not exposed to accessibility tree
                _ => None,
            }
        }
        _ => None,
    }
}

/// Native element equivalents for common ARIA roles — used for "prefer native element" hints.
fn native_element_for_role(role: &str) -> Option<&'static str> {
    match role {
        "button" => Some("<button>"),
        "link" => Some("<a href>"),
        "heading" => Some("<h1>–<h6>"),
        "list" => Some("<ul> or <ol>"),
        "listitem" => Some("<li>"),
        "navigation" => Some("<nav>"),
        "main" => Some("<main>"),
        "complementary" => Some("<aside>"),
        "banner" => Some("<header>"),
        "contentinfo" => Some("<footer>"),
        "search" => Some("<search>"),
        "textbox" => Some("<input> or <textarea>"),
        "checkbox" => Some("<input type=\"checkbox\">"),
        "radio" => Some("<input type=\"radio\">"),
        "img" => Some("<img>"),
        "table" => Some("<table>"),
        "row" => Some("<tr>"),
        "cell" => Some("<td>"),
        "progressbar" => Some("<progress>"),
        "slider" => Some("<input type=\"range\">"),
        _ => None,
    }
}

/// Validate accessibility rules (WCAG 2.2, WAI-ARIA 1.2, ARIA in HTML).
fn validate_a11y(
    el: &Element,
    tag: &str,
    attrs: &ElementAttrs,
    span: proc_macro2::Span,
    errors: &mut BoundedDiagnostics<'_, syn::Error>,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if errors.exhausted() {
        return;
    }
    // Opt-out: `<div _a11y_ok ...>` skips a11y validation for this
    // element. The attribute is stripped from the emitted HTML by
    // `render_static_element`. Use only when mirroring external
    // markup byte-for-byte (e.g., the eBay SRP demo's
    // `<div role="heading">` which intentionally tracks eBay's
    // production output rather than authoring fresh accessible
    // markup); for newly authored elements, fix the underlying
    // accessibility issue instead.
    //
    // The `ElementAttrs` constructor normalizes `_` → `-` for
    // attribute names (HTML convention), so the source-level
    // `_a11y_ok` is stored as `-a11y-ok`. The leading dash is
    // intentional: HTML attribute names can't start with `-`, so
    // the form is unambiguously a framework opt-out.
    if attrs.has("-a11y-ok") {
        return;
    }

    // -------------------------------------------------------------------
    // P0: ARIA role validation (WAI-ARIA 1.2 §5.4)
    // -------------------------------------------------------------------
    if let Some(role_value) = attrs.static_value("role") {
        // Check each role in the value (space-separated token list)
        for role in role_value.split_whitespace() {
            if errors.exhausted() {
                return;
            }
            if ABSTRACT_ARIA_ROLES.contains(&role) {
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        &format!("a11y: \"{role}\" is an abstract ARIA role and cannot be used in content"),
                        "abstract roles exist only as base concepts for the ARIA taxonomy — they are never valid on an element",
                        "use a concrete role instead (e.g., \"button\", \"checkbox\", \"slider\")",
                        "WAI-ARIA 1.2 §5.4",
                    ),
                ));
            } else if !VALID_ARIA_ROLES.contains(&role) {
                // Try to suggest a close match
                let suggestion = VALID_ARIA_ROLES
                    .iter()
                    .filter(|r| {
                        r.starts_with(&role[..role.len().clamp(1, 3)])
                            || role.starts_with(&r[..r.len().clamp(1, 3)])
                    })
                    .take(3)
                    .map(|r| format!("\"{r}\""))
                    .collect::<Vec<_>>();
                let hint = if suggestion.is_empty() {
                    String::new()
                } else {
                    format!(" Did you mean {}?", suggestion.join(", "))
                };
                errors.push(syn::Error::new(
                    span,
                    diagnostic(
                        &format!("a11y: \"{role}\" is not a valid ARIA role"),
                        "screen readers rely on valid roles to convey element semantics — an unrecognized role is ignored",
                        &format!("use one of the 82 roles defined in WAI-ARIA 1.2.{hint}"),
                        "WAI-ARIA 1.2 §5.4",
                    ),
                ));
            }
        }

        // P1: Required ARIA attributes per role (WAI-ARIA 1.2 §5.4)
        let first_role = role_value.split_whitespace().next().unwrap_or("");
        if let Some(required) = required_aria_attrs_for_role(first_role) {
            for req_attr in required {
                if errors.exhausted() {
                    return;
                }
                if !attrs.has(req_attr) {
                    errors.push(syn::Error::new(
                        span,
                        diagnostic(
                            &format!("a11y: role=\"{first_role}\" requires the `{req_attr}` attribute"),
                            &format!("assistive technology uses this attribute to convey the widget's state — without it, the {first_role} appears broken to screen reader users"),
                            &format!("add {req_attr}=\"...\" with the appropriate value"),
                            "WAI-ARIA 1.2 §5.4",
                        ),
                    ));
                }
            }
        }
        // separator only requires aria-valuenow when focusable (has tabindex)
        if first_role == "separator" && attrs.has("tabindex") && !attrs.has("aria-valuenow") {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    "a11y: focusable role=\"separator\" (has tabindex) requires the `aria-valuenow` attribute",
                    "a focusable separator acts as a splitter/resize handle — assistive technology needs aria-valuenow to convey the current position",
                    "add aria-valuenow=\"...\" or remove tabindex if this is a static divider",
                    "WAI-ARIA 1.2 §5.4",
                ),
            ));
        }

        // P2: Redundant role — explicit role matches implicit role (HTML-AAM)
        if let Some(implicit) = implicit_role(tag, attrs) {
            if role_value == implicit {
                warnings.push(emit_warning(
                    span,
                    &format!("a11y: redundant role={role_value} on {tag}"),
                    &diagnostic(
                        &format!("a11y: <{tag} role=\"{role_value}\"> is redundant"),
                        &format!("<{tag}> already has an implicit ARIA role of \"{implicit}\" — the explicit role adds noise"),
                        "remove the role attribute",
                        "HTML-AAM 1.0",
                    ),
                ));
            }
        }

        // P2: Prefer native element over div/span with ARIA role
        if matches!(tag, "div" | "span") {
            if let Some(native) = native_element_for_role(first_role) {
                warnings.push(emit_warning(
                    span,
                    &format!("a11y: prefer {native} over {tag} role={first_role}"),
                    &diagnostic(
                        &format!("a11y: prefer {native} over <{tag} role=\"{first_role}\">"),
                        "native HTML elements provide built-in keyboard support, focus management, and accessibility semantics that ARIA roles alone cannot replicate",
                        &format!("replace <{tag} role=\"{first_role}\"> with {native}"),
                        "WCAG 4.1.2",
                    ),
                ));
            }
        }
    }
    if errors.exhausted() {
        return;
    }

    // -------------------------------------------------------------------
    // P0: ARIA attribute name validation (WAI-ARIA 1.2 §6)
    // P1: ARIA attribute value validation (WAI-ARIA 1.2 §6.2)
    // -------------------------------------------------------------------
    for attr in &el.attrs {
        if errors.exhausted() {
            return;
        }
        let (attr_name, static_val) = match attr {
            Attribute::Static { name, value } => {
                (name.to_string().replace('_', "-"), Some(value.value()))
            }
            Attribute::Dynamic { name, .. } => (name.to_string().replace('_', "-"), None),
            Attribute::Bool { name } => (name.to_string().replace('_', "-"), None),
        };

        if let Some(aria_suffix) = attr_name.strip_prefix("aria-") {
            // Validate attribute name
            if !VALID_ARIA_ATTRS.contains(&aria_suffix) {
                // Try to suggest a close match
                let suggestion = VALID_ARIA_ATTRS
                    .iter()
                    .filter(|a| {
                        a.starts_with(&aria_suffix[..aria_suffix.len().clamp(1, 4)])
                            || aria_suffix.starts_with(&a[..a.len().clamp(1, 4)])
                    })
                    .take(3)
                    .map(|a| format!("\"aria-{a}\""))
                    .collect::<Vec<_>>();
                let hint = if suggestion.is_empty() {
                    String::new()
                } else {
                    format!(" Did you mean {}?", suggestion.join(", "))
                };
                let attr_span = match attr {
                    Attribute::Static { name, .. }
                    | Attribute::Dynamic { name, .. }
                    | Attribute::Bool { name } => name.span(),
                };
                errors.push(syn::Error::new(
                    attr_span,
                    diagnostic(
                        &format!("a11y: \"aria-{aria_suffix}\" is not a valid ARIA attribute"),
                        "ARIA defines 48 attributes — using an invalid one has no effect and may indicate a typo",
                        &format!("check the attribute name for typos.{hint}"),
                        "WAI-ARIA 1.2 §6",
                    ),
                ));
            }

            // Validate enumerated values (only for static attributes)
            if let Some(value) = &static_val {
                if let Some(valid_values) = valid_aria_values(aria_suffix) {
                    if !valid_values.contains(&value.as_str()) {
                        let options = valid_values.join(", ");
                        let attr_span = match attr {
                            Attribute::Static { value, .. } => value.span(),
                            _ => span,
                        };
                        errors.push(syn::Error::new(
                            attr_span,
                            diagnostic(
                                &format!(
                                    "a11y: \"{value}\" is not a valid value for aria-{aria_suffix}"
                                ),
                                "screen readers may ignore or misinterpret an invalid value",
                                &format!("use one of the valid values: {options}"),
                                "WAI-ARIA 1.2 §6.2",
                            ),
                        ));
                    }
                }
            }
        }
    }
    if errors.exhausted() {
        return;
    }

    // -------------------------------------------------------------------
    // P2: ARIA prohibited on certain elements (ARIA in HTML §3)
    // -------------------------------------------------------------------
    if ARIA_PROHIBITED_ELEMENTS.contains(&tag) {
        let has_role = attrs.has("role");
        let has_aria = attrs.names.iter().any(|n| n.starts_with("aria-"));

        if has_role || has_aria {
            errors.push(syn::Error::new(
                span,
                diagnostic(
                    &format!("a11y: <{tag}> does not allow ARIA roles or attributes"),
                    "this element is not exposed to the accessibility tree, so ARIA attributes have no effect and indicate a structural error",
                    "remove the role and aria-* attributes from this element",
                    "ARIA in HTML §3",
                ),
            ));
        }
    }
    if errors.exhausted() {
        return;
    }

    // -------------------------------------------------------------------
    // P1: tabindex > 0 (WCAG 2.1.1)
    // -------------------------------------------------------------------
    if let Some(tabindex_str) = attrs.static_value("tabindex") {
        if let Ok(tabindex) = tabindex_str.parse::<i32>() {
            if tabindex > 0 {
                warnings.push(emit_warning(
                    span,
                    &format!("a11y: tabindex={tabindex} overrides tab order"),
                    &diagnostic(
                        &format!("a11y: tabindex=\"{tabindex}\" overrides the natural document tab order"),
                        "positive tabindex values force elements to the front of the tab sequence, creating a confusing navigation experience for keyboard and screen reader users",
                        "use tabindex=\"0\" to add to natural order, or tabindex=\"-1\" for programmatic focus only",
                        "WCAG 2.1.1",
                    ),
                ));
                if errors.exhausted() {
                    return;
                }
            }
            // Non-interactive element with tabindex="0" but no role
            if tabindex == 0 && !is_always_interactive(tag) && !attrs.has("role") {
                match tag {
                    "a" if attrs.has("href") => {}                   // already interactive
                    "input" | "select" | "textarea" | "button" => {} // native interactive
                    _ => {
                        warnings.push(emit_warning(
                            span,
                            &format!("a11y: {tag} has tabindex=0 but no role"),
                            &diagnostic(
                                &format!("a11y: <{tag}> has tabindex=\"0\" but no ARIA role"),
                                "adding tabindex makes an element focusable, but without a role, screen readers announce it as a generic element — users won't know what it does",
                                "add role=\"button\", role=\"link\", or another appropriate role",
                                "WCAG 4.1.2",
                            ),
                        ));
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // P1: <html> without lang (WCAG 3.1.1)
    // -------------------------------------------------------------------
    if tag == "html" && !attrs.has("lang") {
        warnings.push(emit_warning(
            span,
            "a11y: html missing lang attribute",
            &diagnostic(
                "a11y: <html> is missing the `lang` attribute",
                "screen readers use lang to select the correct pronunciation rules — without it, the screen reader guesses the language, often incorrectly",
                "add lang=\"en\" (or the appropriate BCP 47 language tag)",
                "WCAG 3.1.1",
            ),
        ));
        if errors.exhausted() {
            return;
        }
    }

    // -------------------------------------------------------------------
    // P2: accesskey (WCAG 2.1.1, 4.1.2)
    // -------------------------------------------------------------------
    if attrs.has("accesskey") {
        warnings.push(emit_warning(
            span,
            &format!("a11y: {tag} uses accesskey"),
            &diagnostic(
                &format!("a11y: <{tag}> uses the `accesskey` attribute"),
                "access keys create hidden keyboard shortcuts that conflict across browsers, operating systems, and assistive technologies",
                "remove the accesskey attribute — most a11y guidelines recommend avoiding them",
                "eslint-plugin-jsx-a11y: no-access-key",
            ),
        ));
    }
}

// ===========================================================================
// Performance hints (detect allocations in the render hot path)
// ===========================================================================

/// Check if an expression is wrapped in `perf_ok(...)` — suppresses perf hints.
#[cfg(not(feature = "no-perf-hints"))]
fn is_perf_suppressed(expr: &Expr) -> bool {
    if let Expr::Call(call) = expr {
        if let Expr::Path(path) = call.func.as_ref() {
            if let Some(seg) = path.path.segments.last() {
                return seg.ident == "perf_ok";
            }
        }
    }
    false
}

/// Emit a perf warning via `#[must_use]` struct at the given span.
#[cfg(not(feature = "no-perf-hints"))]
fn perf_warn(
    warnings: &mut BoundedDiagnostics<'_, Warning>,
    span: proc_macro2::Span,
    title: &str,
    _note: &str,
    help: &str,
) {
    if warnings.exhausted() {
        return;
    }
    let msg = format!(
        "perf: {title}. {help} (docs/guides/performance). Wrap in perf_ok() to suppress if intentional"
    );
    warnings.push(emit_warning(span, &format!("perf: {title}"), &msg));
}

/// Check an expression for common allocation patterns.
#[cfg(not(feature = "no-perf-hints"))]
fn check_expr_perf(
    expr: &Expr,
    span: proc_macro2::Span,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if warnings.exhausted() || is_perf_suppressed(expr) {
        return;
    }

    match expr {
        // format!() — always allocates a String
        Expr::Macro(m) => {
            if let Some(seg) = m.mac.path.segments.last() {
                let name = seg.ident.to_string();
                if name == "format" {
                    perf_warn(
                        warnings,
                        span,
                        "format!() inside html! allocates a String on every render",
                        "this creates a heap allocation in the render hot path",
                        "for text content, use inline segments: \"$\"{price}\" / night\" — \
                         for attributes, pre-compute the value before rendering",
                    );
                } else if name == "vec" {
                    perf_warn(
                        warnings,
                        span,
                        "vec![] inside html! allocates a Vec on every render",
                        "heap-allocates a vector in the render hot path",
                        "use a const array or slice: &[1, 2, 3] instead of vec![1, 2, 3]",
                    );
                }
            }
        }

        // .to_string(), .to_owned(), .join(), .collect()
        Expr::MethodCall(m) => {
            let method = m.method.to_string();
            match method.as_str() {
                "to_string" => perf_warn(
                    warnings,
                    span,
                    ".to_string() inside html! allocates a String on every render",
                    "primitives (integers, floats) implement WebRender directly — \
                     no conversion to String needed",
                    "remove .to_string() and pass the value directly: {count} not {count.to_string()}",
                ),
                "to_owned" => perf_warn(
                    warnings,
                    span,
                    ".to_owned() inside html! clones into a new String on every render",
                    "&str implements WebRender directly — cloning to String is unnecessary",
                    "use the &str directly: {name} not {name.to_owned()}",
                ),
                "join" => perf_warn(
                    warnings,
                    span,
                    ".join() inside html! collects into a String on every render",
                    "allocates a String to concatenate the elements",
                    "use a template for loop with a renderable final expression to write each element directly to the buffer",
                ),
                "collect" => perf_warn(
                    warnings,
                    span,
                    ".collect() inside html! allocates a collection on every render",
                    "creates a heap-allocated Vec (or similar) in the render hot path",
                    "iterate the original iterator directly instead of collecting first",
                ),
                _ => {}
            }
        }

        // String::from()
        Expr::Call(call) => {
            if let Expr::Path(path) = call.func.as_ref() {
                let segs: Vec<_> = path
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                if segs == ["String", "from"] {
                    perf_warn(
                        warnings,
                        span,
                        "String::from() inside html! allocates on every render",
                        "creates a heap-allocated String from a &str",
                        "use the &str directly — &str implements WebRender",
                    );
                }
            }
        }

        // String literal in block position: {"literal"} instead of "literal"
        Expr::Lit(lit) => {
            if matches!(lit.lit, syn::Lit::Str(_)) {
                perf_warn(
                    warnings,
                    span,
                    "string literal in {\"...\"} is runtime-escaped — use a text node instead",
                    "a string literal in braces triggers runtime escape scanning, \
                     but a bare text node \"...\" is escaped at compile time and \
                     compiled into the static byte run",
                    "change {\"text\"} to \"text\" (remove the braces)",
                );
            }
        }

        _ => {}
    }
}

/// Validate performance hints for an element's children and attributes.
#[cfg(not(feature = "no-perf-hints"))]
fn validate_perf_hints(
    el: &Element,
    span: proc_macro2::Span,
    warnings: &mut BoundedDiagnostics<'_, Warning>,
) {
    if warnings.exhausted() {
        return;
    }
    // Phase 1: Check child expressions
    for child in &el.children {
        if warnings.exhausted() {
            return;
        }
        if let Node::Block(expr) = child {
            check_expr_perf(expr, span, warnings);
        }
    }

    // Phase 2: Check dynamic attribute values
    for attr in &el.attrs {
        if warnings.exhausted() {
            return;
        }
        if let Attribute::Dynamic { value, .. } = attr {
            // String literal in dynamic attribute: class={"value"} instead of class="value"
            if !is_perf_suppressed(value) {
                if let Expr::Lit(lit) = value.as_ref() {
                    if matches!(lit.lit, syn::Lit::Str(_)) {
                        perf_warn(
                            warnings,
                            span,
                            "dynamic attribute with a literal value breaks static consolidation",
                            "class={\"value\"} triggers a runtime attribute write, \
                             but class=\"value\" is compiled into the static byte run at zero cost",
                            "change attr={\"value\"} to attr=\"value\" (remove the braces)",
                        );
                    }
                }
            }

            // Also check for format!(), .to_string(), etc. in attribute values
            check_expr_perf(value, span, warnings);
        }
    }
}

/// Check if a node is fully static (can be rendered at compile time)
pub fn is_static_node(node: &Node) -> bool {
    match node {
        // Text literals are always static - we can escape them at compile time
        Node::Text(_) => true,
        Node::Element(el) => is_static_element(el),
        Node::Block(_) => false, // Expressions are never static
        Node::Fragment(children) => children.iter().all(is_static_node),
        Node::Doctype(_) | Node::XmlDeclaration(_) => true,
    }
}

/// Check if an element is fully static
fn is_static_element(el: &Element) -> bool {
    // All attributes must be static (literal values, not expressions)
    // We can escape attribute values at compile time
    let attrs_static = el
        .attrs
        .iter()
        .all(|attr| matches!(attr, Attribute::Static { .. } | Attribute::Bool { .. }));

    if !attrs_static {
        return false;
    }

    // All children must be static
    el.children.iter().all(is_static_node)
}

/// Escape text for HTML content at compile time
pub fn escape_text_compile_time(s: &str, out: &mut String, dialect: MarkupDialect) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{00A0}' if dialect == MarkupDialect::Html => out.push_str("&nbsp;"),
            '\0' if dialect == MarkupDialect::Html => out.push('\u{FFFD}'),
            '\0' => debug_assert!(
                false,
                "xml! static validation should reject U+0000 before codegen"
            ),
            _ => out.push(c),
        }
    }
}

/// Escape text for attribute values at compile time
pub fn escape_attr_compile_time(s: &str, out: &mut String, dialect: MarkupDialect) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\u{00A0}' if dialect == MarkupDialect::Html => out.push_str("&nbsp;"),
            '\0' if dialect == MarkupDialect::Html => out.push('\u{FFFD}'),
            '\0' => debug_assert!(
                false,
                "xml! static validation should reject U+0000 before codegen"
            ),
            _ => out.push(c),
        }
    }
}

/// Render a static node to a string (at macro expansion time)
pub fn render_static_node(node: &Node, out: &mut String, dialect: MarkupDialect) {
    match node {
        Node::Text(lit) => {
            // Escape text at compile time
            escape_text_compile_time(&lit.value(), out, dialect);
        }
        Node::Element(el) => {
            render_static_element(el, out, dialect);
        }
        Node::Fragment(children) => {
            for child in children {
                render_static_node(child, out, dialect);
            }
        }
        Node::Doctype(content) => {
            out.push_str(content);
            if dialect == MarkupDialect::Html {
                out.push_str(PROA_DOCTYPE_MARKER);
            }
        }
        Node::XmlDeclaration(declaration) => {
            write!(out, "{declaration}").expect("writing XML declaration to String cannot fail");
        }
        Node::Block(_) => unreachable!("Block nodes are not static"),
    }
}

/// What action to take for reverse-tabnabbing defence on an `<a target="_blank">`.
///
/// Prevents reverse tabnabbing: when `target="_blank"` opens a new window, the
/// opened page can access `window.opener` and redirect the original tab to a
/// phishing page. `noopener` severs that reference; `noreferrer` additionally
/// strips the Referer header.
///
/// Modern browsers (HTML spec PR #6188, 2021) default to noopener, but linters
/// (eslint-plugin-react, axe-core) still flag it and older browsers lack the default.
///
/// Reference: OWASP HTML5 Security Cheat Sheet — Tabnabbing
/// <https://cheatsheetseries.owasp.org/cheatsheets/HTML5_Security_Cheat_Sheet.html#tabnabbing>
enum NoopenerAction {
    /// No action needed (not `<a target="_blank">`, dynamic attrs, or already has `noopener`).
    None,
    /// No `rel` attribute present — inject `rel="noopener noreferrer"`.
    Inject,
    /// A static `rel` exists but lacks `noopener` — append ` noopener noreferrer` to its value.
    /// Contains the existing `rel` value.
    Merge(String),
}

fn noopener_rel_action(tag: &str, attrs: &[Attribute]) -> NoopenerAction {
    if tag != "a" {
        return NoopenerAction::None;
    }
    let mut has_target_blank = false;
    let mut existing_rel: Option<String> = None;
    for attr in attrs {
        match attr {
            Attribute::Static { name, value } => {
                let n = name.to_string().replace('_', "-");
                if n == "target" && value.value() == "_blank" {
                    has_target_blank = true;
                } else if n == "rel" {
                    let v = value.value();
                    if v.to_ascii_lowercase().contains("noopener") {
                        // Already has noopener — nothing to do.
                        return NoopenerAction::None;
                    }
                    existing_rel = Some(v);
                }
            }
            Attribute::Dynamic { name, .. } => {
                let n = name.to_string().replace('_', "-");
                // If target or rel is dynamic, we can't decide at compile time —
                // fall back to the runtime check in elem.rs.
                if n == "target" || n == "rel" {
                    return NoopenerAction::None;
                }
            }
            _ => {}
        }
    }
    if !has_target_blank {
        return NoopenerAction::None;
    }
    match existing_rel {
        None => NoopenerAction::Inject,
        Some(v) => NoopenerAction::Merge(v),
    }
}

/// Whether `attr` has the given name (after the parser's `_`→`-`
/// flattening). Used to detect framework opt-out attrs (e.g.
/// `_a11y_ok`) that must be filtered before HTML emission. Compare
/// `want` in either underscore or dashed form; both forms are
/// normalized to dashed before matching.
fn attr_name_is(attr: &Attribute, want: &str) -> bool {
    let name = match attr {
        Attribute::Static { name, .. }
        | Attribute::Dynamic { name, .. }
        | Attribute::Bool { name } => name.to_string(),
    };
    name.replace('_', "-") == want.replace('_', "-")
}

/// Render a static element to a string
// proa-perf-ok(scope): this IS the template system's compile-time static renderer (pre-escaped runs)
fn render_static_element(el: &Element, out: &mut String, dialect: MarkupDialect) {
    // Use `tag_str()` so hyphenated custom-element tags (`rsjs-island`)
    // round-trip into the emitted HTML with their dash intact; the parser
    // stores them with `-` flattened to `_`.
    let tag = dialect.emitted_tag(el);

    out.push('<');
    out.push_str(&tag);

    // Reverse tabnabbing defense — see noopener_rel_action() for OWASP reference.
    // When merging, skip the existing `rel` attr here and write the merged value below.
    let noopener = if dialect == MarkupDialect::Html {
        noopener_rel_action(&tag, &el.attrs)
    } else {
        NoopenerAction::None
    };
    let skip_rel = matches!(noopener, NoopenerAction::Merge(_));
    for attr in &el.attrs {
        // Strip the `_a11y_ok` opt-out attribute from output. Its
        // only purpose is to silence `validate_a11y`; it must not
        // reach the rendered HTML.
        if dialect == MarkupDialect::Html && attr_name_is(attr, "_a11y_ok") {
            continue;
        }
        if skip_rel {
            if let Attribute::Static { name, .. } = attr {
                if name.to_string().replace('_', "-") == "rel" {
                    continue;
                }
            }
        }
        render_static_attr(attr, out, dialect);
    }

    match noopener {
        NoopenerAction::None => {}
        NoopenerAction::Inject => {
            out.push_str(" rel=\"noopener noreferrer\"");
        }
        NoopenerAction::Merge(existing) => {
            out.push_str(" rel=\"");
            escape_attr_compile_time(&existing, out, dialect);
            out.push_str(" noopener noreferrer\"");
        }
    }

    if (dialect == MarkupDialect::Xml && el.self_closing)
        || (dialect == MarkupDialect::Html && is_svg_void_element(tag.as_str()))
    {
        out.push_str("/>");
        return;
    }

    out.push('>');

    if !el.self_closing {
        for child in &el.children {
            render_static_node(child, out, dialect);
        }
        out.push_str("</");
        out.push_str(&tag);
        out.push('>');
    }
}

/// SVG leaf elements the DSL parser treats as void (`proa_dsl`'s
/// `is_void_element` SVG subset). HTML serialization must emit them
/// XML-style (`<path/>`): inside `<svg>` foreign content the HTML parser
/// honours the self-closing slash, while a bare `<path>` start tag swallows
/// the following sibling leaves as children, so only the first one renders.
fn is_svg_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "path"
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

/// Render a static attribute to a string
fn render_static_attr(attr: &Attribute, out: &mut String, dialect: MarkupDialect) {
    match attr {
        Attribute::Static { name, value } => {
            let attr_name = dialect.attr_name(name);
            out.push(' ');
            out.push_str(&attr_name);
            out.push_str("=\"");
            // Escape attribute value at compile time
            escape_attr_compile_time(&value.value(), out, dialect);
            out.push('"');
        }
        Attribute::Bool { name } => {
            let attr_name = dialect.attr_name(name);
            out.push_str(&dialect.bool_attr_bytes(&attr_name));
        }
        Attribute::Dynamic { .. } => unreachable!("Dynamic attributes are not static"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::HtmlRoot;

    #[test]
    fn dynamic_title_is_accepted_while_raw_text_warnings_remain() {
        let title: HtmlRoot = syn::parse_str("<title>{page_title}</title>").unwrap();
        let validation = validate(&title);
        assert!(validation.errors.is_empty());
        assert!(validation.warnings.is_empty());
        for tag in ["textarea", "xmp", "noscript", "noframes"] {
            let root: HtmlRoot = syn::parse_str(&format!("<{tag}>{{value}}</{tag}>")).unwrap();
            let warnings = validate_security(&root).unwrap().to_string();
            assert!(
                warnings.contains("raw_text_element_dynamic_interpolation"),
                "{tag}: {warnings}"
            );
        }
    }

    #[test]
    fn indentation_warning_for_unindented_child_element() {
        let root: HtmlRoot = syn::parse_str(
            r#"<div>
<span>"x"</span>
</div>"#,
        )
        .unwrap();

        let warnings = validate_security(&root).unwrap().to_string();

        assert!(warnings.contains("nested_markup_indentation"));
    }

    #[test]
    fn indentation_allows_child_element_indented_past_parent() {
        let root: HtmlRoot = syn::parse_str(
            r#"<div>
    <span>"x"</span>
</div>"#,
        )
        .unwrap();

        let warnings = validate_security(&root).unwrap().to_string();

        assert!(!warnings.contains("nested_markup_indentation"));
    }

    #[test]
    fn bounded_validation_caps_aggregate_diagnostics_and_stops() {
        let root: HtmlRoot =
            syn::parse_str(r#"<div><img/><img/><img/><img/><img/><img/><img/><img/></div>"#)
                .unwrap();

        let bounded = validate_bounded(&root, 1);

        assert!(bounded.limit_reached);
        assert_eq!(
            bounded.validation.errors.len() + bounded.validation.warnings.len(),
            1
        );

        let zero = validate_bounded(&root, 0);
        assert!(zero.limit_reached);
        assert!(zero.validation.errors.is_empty());
        assert!(zero.validation.warnings.is_empty());
    }
}
