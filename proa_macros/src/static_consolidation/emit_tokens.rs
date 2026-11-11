// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `TokenStream` emitter: the first [`ConsolidationSink`] implementation
//! for reuse by runtime-template compilers.
//!
//! Everything `quote!`-shaped lives here: byte-literal writes, `AttrValue`
//! render calls, control-flow lowering of `{expr}` blocks into
//! typed head/body dispatch and direct Done/Pending handling, raw-text
//! content-mode guards, and the sync/async/XML closure wrappers around the
//! statement sequence. The walk in [`super::walk`] never sees any of it.

use crate::parse::HtmlRoot;
use crate::text_macro::{self, ParsedText, Segment};
use proa_dsl::consolidate::sink::{ConsolidationSink, DynamicAttr, RawTextKind};
use proa_dsl::consolidate::walk::consolidate_root;
use proa_dsl::consolidate::{
    escape_attr_compile_time, escape_text_compile_time, is_dangerous_url_value, is_static_node,
    render_static_node, MarkupDialect,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Expr;

/// Unique-suffix counter for the `let __proa_raw_text_mode_N = ...` guard
/// bindings emitted around `<script>` / `<style>` children, so nested raw-text
/// elements (degenerate but syntactically possible) do not shadow each other.
static RAW_TEXT_MODE_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Fresh identifier for a content-mode guard binding.
fn raw_text_mode_guard_ident() -> syn::Ident {
    let id = RAW_TEXT_MODE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    syn::Ident::new(
        &format!("__proa_raw_text_mode_{id}"),
        proc_macro2::Span::call_site(),
    )
}

/// Fresh identifier for a raw-text child render result binding.
fn raw_text_mode_result_ident() -> syn::Ident {
    let id = RAW_TEXT_MODE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    syn::Ident::new(
        &format!("__proa_raw_text_result_{id}"),
        proc_macro2::Span::call_site(),
    )
}

/// Whether the emitted code drives children through the sync
/// (`WebRenderSync`) or async (`WebRender`) render path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmitMode {
    Sync,
    Async,
    SendAsync,
}

/// One entered (and not yet closed) raw-text region.
struct RawTextFrame {
    /// Content-mode guard binding, allocated at region entry so nested
    /// regions number outer-before-inner exactly as the pre-seam code did.
    guard: syn::Ident,
    /// Child render result binding.
    result: syn::Ident,
    /// The statements of the enclosing region, restored on exit.
    parent_statements: Vec<TokenStream>,
}

/// [`ConsolidationSink`] that materializes the walk's event sequence as the
/// statement list of a render closure.
struct TokenStreamSink {
    mode: EmitMode,
    dialect: MarkupDialect,
    hot: bool,
    hot_slot: usize,
    /// Static bytes are held until a genuinely dynamic operation is emitted.
    /// This lets an inline literal `text!(...)` join the surrounding HTML run
    /// instead of forcing an extra `extend_static` boundary.
    pending_static: String,
    statements: Vec<TokenStream>,
    raw_text_stack: Vec<RawTextFrame>,
}

impl TokenStreamSink {
    fn new(mode: EmitMode, dialect: MarkupDialect) -> Self {
        Self {
            mode,
            dialect,
            hot: false,
            hot_slot: 0,
            pending_static: String::new(),
            statements: Vec::new(),
            raw_text_stack: Vec::new(),
        }
    }

    fn flush_static(&mut self) {
        let proa_core = proa_dsl::paths::runtime("proa_core");
        if self.pending_static.is_empty() {
            return;
        }

        let statement = if self.hot {
            // Hot reload swaps this slot for bytes read at run time, which the
            // writer validates; the fallback stays a byte literal to match.
            let html_bytes = proc_macro2::Literal::byte_string(self.pending_static.as_bytes());
            let slot = syn::Index::from(self.hot_slot);
            self.hot_slot += 1;
            quote! { #proa_core::dev_hot::write_slot(&mut cx.out, __proa_dev_static.as_ref(), #slot, #html_bytes)?; }
        } else {
            let html_text = proc_macro2::Literal::string(&self.pending_static);
            quote! { #proa_core::WriteBuf::extend_static_str(&mut cx.out, #html_text)?; }
        };
        self.statements.push(statement);
        self.pending_static.clear();
    }

    fn push_statement(&mut self, statement: TokenStream) {
        self.flush_static();
        self.statements.push(statement);
    }

    fn finish(mut self) -> SinkOutput {
        debug_assert!(
            self.raw_text_stack.is_empty(),
            "unbalanced begin_raw_text/end_raw_text"
        );

        if self.statements.is_empty() && !self.hot {
            return SinkOutput::Static(self.pending_static);
        }

        self.flush_static();
        SinkOutput::Dynamic(self.statements)
    }
}

enum SinkOutput {
    Static(String),
    Dynamic(Vec<TokenStream>),
}

#[derive(Clone, Copy)]
enum InlineTextContext {
    Text,
    Attr,
}

/// Append a placeholder-free inline `text!(...)` to the outer static run.
fn append_inline_static_text(out: &mut String, parsed: &ParsedText, context: InlineTextContext) {
    debug_assert!(parsed.args.is_empty());
    for segment in &parsed.segments {
        let Segment::Static(chunk) = segment else {
            unreachable!("placeholder-free text! contained a dynamic segment");
        };
        match context {
            InlineTextContext::Text => escape_text_compile_time(chunk, out, MarkupDialect::Html),
            InlineTextContext::Attr => escape_attr_compile_time(chunk, out, MarkupDialect::Html),
        }
    }
}

fn inline_static_text_value(parsed: &ParsedText) -> String {
    debug_assert!(parsed.args.is_empty());
    let mut value = String::new();
    for segment in &parsed.segments {
        let Segment::Static(chunk) = segment else {
            unreachable!("placeholder-free text! contained a dynamic segment");
        };
        value.push_str(chunk);
    }
    value
}

fn dynamic_attr_name(name_prefix: &str) -> &str {
    name_prefix
        .strip_prefix(' ')
        .and_then(|name| name.strip_suffix("=\""))
        .unwrap_or("URL")
}

/// Lower inline `text!(...)` directly into the surrounding HTML render body.
///
/// Arguments are bound before any bytes belonging to the invocation are
/// written. Besides evaluating every expression exactly once, this preserves
/// standalone `text!`'s observable ordering: constructing `__ProaText`
/// evaluates all arguments before its `render` / `render_attr` call begins.
/// The caller flushes preceding outer HTML before inserting this block.
fn generate_inline_text(
    parsed: ParsedText,
    context: InlineTextContext,
    attr_name_prefix: Option<proc_macro2::Literal>,
    mode: EmitMode,
) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let args_ident = format_ident!("__proa_text_args", span = parsed.span);
    let args = text_macro::wrap_format_args(&parsed.segments, &parsed.args, parsed.span);

    let prefix = attr_name_prefix.map(|prefix| {
        quote! {
            #proa_core::WriteBuf::extend_static_str(&mut cx.out, #prefix)?;
        }
    });

    let segment_statements = parsed.segments.iter().map(|segment| match segment {
        Segment::Static(chunk) => {
            let mut escaped = String::new();
            match context {
                InlineTextContext::Text => {
                    escape_text_compile_time(chunk, &mut escaped, MarkupDialect::Html)
                }
                InlineTextContext::Attr => {
                    escape_attr_compile_time(chunk, &mut escaped, MarkupDialect::Html)
                }
            }
            static_write_statement(&escaped)
        }
        Segment::Bare { index } => {
            let index = syn::Index::from(*index);
            match context {
                InlineTextContext::Text => quote! {
                    #proa_core::head_dispatch::sync(#args_ident.#index, cx)?;
                },
                InlineTextContext::Attr => quote! {
                    #proa_core::shared::attr_value::AttrValue::render_attr_value(&#args_ident.#index, cx)?;
                },
            }
        }
        Segment::Formatted {
            index,
            fixed_precision: Some(precision),
            ..
        } => {
            let index = syn::Index::from(*index);
            match context {
                InlineTextContext::Text => quote! {
                    #proa_core::shared::numeric::__ProaFixedFormatValue::<#precision>::__proa_render_fixed_text(
                        #args_ident.#index,
                        cx,
                    )?;
                },
                InlineTextContext::Attr => quote! {
                    #proa_core::shared::numeric::__ProaFixedFormatValue::<#precision>::__proa_render_fixed_attr(
                        &#args_ident.#index,
                        cx,
                    )?;
                },
            }
        }
        Segment::Formatted {
            index,
            integer_plan: Some(_),
            ..
        } => {
            let index = syn::Index::from(*index);
            match context {
                InlineTextContext::Text => quote! {
                    #proa_core::shared::numeric::__ProaIntegerFormatValue::__proa_render_integer_text(
                        #args_ident.#index,
                        cx,
                    )?;
                },
                InlineTextContext::Attr => quote! {
                    #proa_core::shared::numeric::__ProaIntegerFormatValue::__proa_render_integer_attr(
                        &#args_ident.#index,
                        cx,
                    )?;
                },
            }
        }
        Segment::Formatted { index, format, .. } => {
            let index = syn::Index::from(*index);
            let format = syn::LitStr::new(format, parsed.span);
            match context {
                InlineTextContext::Text => quote! {
                    #proa_core::shared::text::__proa_render_fmt_text(
                        ::core::format_args!(#format, #args_ident.#index),
                        cx,
                    )?;
                },
                InlineTextContext::Attr => quote! {
                    #proa_core::shared::attr_value::__proa_render_fmt_attr(
                        ::core::format_args!(#format, #args_ident.#index),
                        cx,
                    )?;
                },
            }
        }
    });

    let suffix = matches!(context, InlineTextContext::Attr).then(|| {
        quote! {
            #proa_core::WriteBuf::extend_static_str(&mut cx.out, "\"")?;
        }
    });

    let body = quote! {
        {
            // A tuple matches standalone text!'s left-to-right argument
            // evaluation and declaration-order field destruction, including
            // when a later render operation returns early with `?`.
            let #args_ident = (#(#args,)*);
            #prefix
            #(#segment_statements)*
            #suffix
        }
    };
    if mode == EmitMode::SendAsync {
        quote! {
            cx.__proa_with_stateless_context(|cx| -> Result<(), #proa_core::WriteError> {
                #body
                Ok(())
            })?;
        }
    } else {
        body
    }
}

fn static_write_statement(escaped: &str) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    if escaped.is_empty() {
        return quote! {};
    }

    let text = proc_macro2::Literal::string(escaped);
    quote! {
        #proa_core::WriteBuf::extend_static_str(&mut cx.out, #text)?;
    }
}

impl ConsolidationSink for TokenStreamSink {
    fn static_run(&mut self, pre_escaped: &str) {
        self.pending_static.push_str(pre_escaped);
    }

    fn dynamic_node(&mut self, expr: &Expr) {
        if self.dialect == MarkupDialect::Html {
            if let Some(parsed) = text_macro::parse_expr(expr) {
                match parsed {
                    Ok(parsed) if parsed.args.is_empty() => {
                        append_inline_static_text(
                            &mut self.pending_static,
                            &parsed,
                            InlineTextContext::Text,
                        );
                    }
                    Ok(parsed) => {
                        self.push_statement(generate_inline_text(
                            parsed,
                            InlineTextContext::Text,
                            None,
                            self.mode,
                        ));
                    }
                    Err(err) => self.push_statement(err.to_compile_error()),
                }
                return;
            }
        }

        self.flush_static();
        match self.mode {
            EmitMode::Sync => generate_block_expr(expr, &mut self.statements),
            EmitMode::Async => generate_block_expr_async(expr, &mut self.statements),
            EmitMode::SendAsync => generate_block_expr_send_async(expr, &mut self.statements),
        }
    }

    fn dynamic_attr(&mut self, attr: DynamicAttr<'_>) {
        let proa_core = proa_dsl::paths::runtime("proa_core");
        if self.dialect == MarkupDialect::Html {
            if let Some(parsed) = text_macro::parse_expr(attr.value) {
                let parsed = match parsed {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        self.push_statement(err.to_compile_error());
                        return;
                    }
                };

                if attr.is_url_position {
                    if parsed.args.is_empty() {
                        // A placeholder-free text! has one complete value at
                        // expansion time. Apply the same dangerous-scheme
                        // policy as a normal static URL attribute before its
                        // escaped bytes join the surrounding static run.
                        let value = inline_static_text_value(&parsed);
                        if is_dangerous_url_value(&value) {
                            let attr_name = dynamic_attr_name(attr.name_prefix);
                            let error = syn::Error::new(
                                parsed.span,
                                format!(
                                    "dangerous URL scheme in `{attr_name}` attribute. use a safe scheme (https:, /, #) or pass data via data-* attributes (OWASP XSS Prevention Cheat Sheet, Rule #7)"
                                ),
                            );
                            self.push_statement(error.to_compile_error());
                            return;
                        }
                    } else {
                        // A composite text! streams its pieces and cannot
                        // expose the combined unescaped value through
                        // AttrValue::url_str(). Reject it so a dangerous
                        // scheme cannot be assembled across placeholders.
                        let error = syn::Error::new_spanned(
                            attr.value,
                            "text!(...) with placeholders cannot be used directly in a URL-bearing attribute because its combined value cannot be validated; build a validated URL value before html! or use a typed URL helper",
                        );
                        self.push_statement(error.to_compile_error());
                        return;
                    }
                }

                // Hot manifests preserve the walk's dynamic-attribute boundary,
                // even when text! has no placeholders. Emit that attribute
                // separately so subsequent static slots keep their indices.
                if parsed.args.is_empty() && !self.hot {
                    self.pending_static.push_str(attr.name_prefix);
                    append_inline_static_text(
                        &mut self.pending_static,
                        &parsed,
                        InlineTextContext::Attr,
                    );
                    self.pending_static.push('"');
                } else {
                    let name_prefix = proc_macro2::Literal::string(attr.name_prefix);
                    self.push_statement(generate_inline_text(
                        parsed,
                        InlineTextContext::Attr,
                        Some(name_prefix),
                        self.mode,
                    ));
                }
                return;
            }
        }

        let name_with_eq_lit = proc_macro2::Literal::string(attr.name_prefix);
        let name_bool_lit = proc_macro2::Literal::string(attr.bool_form);
        let value = attr.value;

        // URL-bearing attributes route through `render_url_attr`, which
        // blocks `javascript:` / `vbscript:` / executable `data:` schemes in
        // the dynamic value at runtime (see the walk for the classification).
        let render_method = if attr.is_url_position {
            quote! { render_url_attr }
        } else {
            quote! { render_attr }
        };

        // Single call handles the entire attribute: prefix (` name="`),
        // value, and closing quote (`"`). This replaces 3 separate
        // extend_static calls with one trait method call that each
        // AttrValue impl optimizes.
        let statement = quote! {
            {
                use #proa_core::shared::attr_value::AttrValue;
                static NAME_PREFIX: &str = #name_with_eq_lit;
                static NAME_BOOL: &str = #name_bool_lit;
                AttrValue::#render_method(&(#value), NAME_PREFIX, NAME_BOOL, cx)?;
            }
        };
        if self.mode == EmitMode::SendAsync {
            self.push_statement(quote! {
                cx.__proa_with_stateless_context(|cx| -> Result<(), #proa_core::WriteError> {
                    #statement
                    Ok(())
                })?;
            });
        } else {
            self.push_statement(statement);
        }
    }

    fn begin_raw_text(&mut self, _kind: RawTextKind) {
        self.flush_static();
        self.raw_text_stack.push(RawTextFrame {
            guard: raw_text_mode_guard_ident(),
            result: raw_text_mode_result_ident(),
            parent_statements: std::mem::take(&mut self.statements),
        });
    }

    fn end_raw_text(&mut self, kind: RawTextKind) {
        let proa_core = proa_dsl::paths::runtime("proa_core");
        self.flush_static();
        let frame = self
            .raw_text_stack
            .pop()
            .expect("end_raw_text without begin_raw_text");
        let child_statements = std::mem::replace(&mut self.statements, frame.parent_statements);
        let guard = frame.guard;
        let result = frame.result;
        let variant = match kind {
            RawTextKind::Script => quote! { Script },
            RawTextKind::Style => quote! { Style },
        };
        let mode_path = quote! { #proa_core::shared::mode::ContentMode::#variant };
        match self.mode {
            EmitMode::Sync => self.push_statement(quote! {
                let #guard = cx.__proa_enter_mode(#mode_path);
                let #result: Result<(), #proa_core::shared::write_buf::WriteError> = (|| {
                    #(#child_statements)*
                    Ok(())
                })();
                cx.__proa_restore_mode(#guard);
                #result?;
            }),
            EmitMode::Async | EmitMode::SendAsync => self.push_statement(quote! {
                let #guard = cx.__proa_enter_mode(#mode_path);
                let #result: Result<(), #proa_core::shared::write_buf::WriteError> = async {
                    #(#child_statements)*
                    Ok(())
                }.await;
                cx.__proa_restore_mode(#guard);
                #result?;
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Generate optimized code for an HTML root that returns an `OptimizedHtml`.
///
/// Like `html!`, this returns a composable value that implements `WebRender`.
/// The difference is that static portions are consolidated into single byte
/// literals at compile time, dramatically reducing runtime overhead.
///
/// For fully-static trees, generates a single static byte array write.
/// For mixed content, generates a sequence of static byte writes and dynamic renders.
pub fn generate_optimized(root: &HtmlRoot) -> TokenStream {
    generate_optimized_with_dialect(root, MarkupDialect::Html)
}

pub fn generate_optimized_hot(
    root: &HtmlRoot,
    path: &str,
    id: &str,
    shape: &str,
    slots: usize,
) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let mut sink = TokenStreamSink::new(EmitMode::Sync, MarkupDialect::Html);
    sink.hot = true;
    consolidate_root(root, MarkupDialect::Html, &mut sink);
    let SinkOutput::Dynamic(statements) = sink.finish() else {
        unreachable!()
    };
    quote! {
        #proa_core::shared::html_node::optimized_html_from_fn(|cx| -> Result<(), #proa_core::WriteError> {
            let __proa_dev_static = #proa_core::dev_hot::load(#path, #id, #shape, #slots);
            #(#statements)*
            Ok(())
        })
    }
}

pub fn generate_optimized_xml(root: &HtmlRoot) -> TokenStream {
    generate_optimized_with_dialect(root, MarkupDialect::Xml)
}

fn generate_optimized_with_dialect(root: &HtmlRoot, dialect: MarkupDialect) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    if let Some(static_html) = fully_static_tokens(root, dialect) {
        return static_html;
    }

    // Has dynamic content: consolidate through the sink seam.
    let mut sink = TokenStreamSink::new(EmitMode::Sync, dialect);
    consolidate_root(root, dialect, &mut sink);
    let statements = match sink.finish() {
        SinkOutput::Static(html) => return static_html_tokens(&html),
        SinkOutput::Dynamic(statements) => statements,
    };

    if dialect == MarkupDialect::Html {
        return quote! {
            #proa_core::shared::html_node::optimized_html_from_fn(|cx| -> Result<(), #proa_core::shared::write_buf::WriteError> {
                #(#statements)*
                Ok(())
            })
        };
    }

    let enter_dialect = dialect.runtime_escape_tokens();
    let restore_dialect = dialect.runtime_restore_tokens();

    quote! {
        #proa_core::shared::html_node::optimized_html_from_fn(|cx| -> Result<(), #proa_core::shared::write_buf::WriteError> {
            #enter_dialect
            let __proa_render_result: Result<(), #proa_core::shared::write_buf::WriteError> = (|| {
                #(#statements)*
                Ok(())
            })();
            #restore_dialect
            __proa_render_result
        })
    }
}

/// Generate async-capable code for an HTML root with a typed continuation.
///
/// This is the implementation behind the public `html!` macro. Static runs are
/// still consolidated into byte literals at compile time, but interpolated
/// child expressions are driven through `WebRender` so they can participate in
/// async rendering.
pub fn generate_optimized_async(root: &HtmlRoot) -> TokenStream {
    generate_optimized_async_with_dialect(root, MarkupDialect::Html)
}

pub fn generate_optimized_async_xml(root: &HtmlRoot) -> TokenStream {
    generate_optimized_async_with_dialect(root, MarkupDialect::Xml)
}

/// Generate a compiler-proved `Send` async HTML fragment.
pub fn generate_optimized_send_async(root: &HtmlRoot) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    if let Some(static_html) = fully_static_tokens(root, MarkupDialect::Html) {
        return static_html;
    }

    let mut sink = TokenStreamSink::new(EmitMode::SendAsync, MarkupDialect::Html);
    consolidate_root(root, MarkupDialect::Html, &mut sink);
    let statements = match sink.finish() {
        SinkOutput::Static(html) => return static_html_tokens(&html),
        SinkOutput::Dynamic(statements) => statements,
    };

    quote! {
        #proa_core::shared::html_node::send_async_html_from_fn(::std::boxed::Box::new(|cx| {
            ::std::boxed::Box::pin(async move {
                #(#statements)*
                Ok(())
            })
        }))
    }
}

fn generate_optimized_async_with_dialect(root: &HtmlRoot, dialect: MarkupDialect) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    if let Some(static_html) = fully_static_tokens(root, dialect) {
        return static_html;
    }

    // Has dynamic content: consolidate through the sink seam. Note the
    // async emitter historically leaked the `_a11y_ok` opt-out attribute
    // into dynamic-path output (unlike sync); the walk now strips it
    // unconditionally on both paths, fixing that leak.
    let mut sink = TokenStreamSink::new(EmitMode::Async, dialect);
    consolidate_root(root, dialect, &mut sink);
    let statements = match sink.finish() {
        SinkOutput::Static(html) => return static_html_tokens(&html),
        SinkOutput::Dynamic(statements) => statements,
    };

    if dialect == MarkupDialect::Html {
        return quote! {
            #proa_core::shared::async_render::async_render(async move |cx| {
                #(#statements)*
                Ok(())
            })
        };
    }

    let enter_dialect = dialect.runtime_escape_tokens();
    let restore_dialect = dialect.runtime_restore_tokens();

    quote! {
        #proa_core::shared::async_render::async_render(async move |cx| {
            #enter_dialect
            let __proa_render_result: Result<(), #proa_core::shared::write_buf::WriteError> = async {
                #(#statements)*
                Ok(())
            }.await;
            #restore_dialect
            __proa_render_result
        })
    }
}

/// The fully-static fast path shared by the sync and async entry points:
/// when every root node is static, the whole tree collapses to a single
/// pre-escaped byte literal.
fn fully_static_tokens(root: &HtmlRoot, dialect: MarkupDialect) -> Option<TokenStream> {
    if !root.nodes.iter().all(is_static_node) {
        return None;
    }
    let mut html = String::new();
    for node in &root.nodes {
        render_static_node(node, &mut html, dialect);
    }
    Some(static_html_tokens(&html))
}

fn static_html_tokens(html: &str) -> TokenStream {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    let html_text = proc_macro2::Literal::string(html);
    quote! {
        #proa_core::shared::html_node::static_html(#html_text)
    }
}

// All formats and execution modes share one Rust slot/control-flow transform.
fn generate_block_expr(expr: &Expr, statements: &mut Vec<TokenStream>) {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    statements.push(crate::render_control::lower(expr, &|expr| {
        quote! {
            #proa_core::head_dispatch::sync(#expr, cx)?;
        }
    }));
}

fn generate_block_expr_async(expr: &Expr, statements: &mut Vec<TokenStream>) {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    statements.push(crate::render_control::lower(expr, &|expr| {
        quote! {
            match #proa_core::head_dispatch::begin(#expr, cx) {
                #proa_core::RenderOutcome::Done(__proa_result) => __proa_result?,
                #proa_core::RenderOutcome::Pending(__proa_future) => __proa_future.await?,
            };
        }
    }));
}

fn generate_block_expr_send_async(expr: &Expr, statements: &mut Vec<TokenStream>) {
    let proa_core = proa_dsl::paths::runtime("proa_core");
    statements.push(crate::render_control::lower(expr, &|expr| {
        quote! {
            match #proa_core::head_dispatch::send_begin(#expr, cx) {
                #proa_core::RenderOutcome::Done(__proa_result) => __proa_result?,
                #proa_core::RenderOutcome::Pending(__proa_future) => __proa_future.await?,
            };
        }
    }));
}

#[cfg(test)]
mod hot_tests {
    use super::*;

    #[test]
    fn hot_inline_attributes_preserve_manifest_slot_boundaries() {
        for source in [
            r#""page"; <p title={text!("fixed")}>"prefix"{self.value}"suffix"</p>"#,
            r#""page"; <p title={text!("fixed")} data-label={text!("second")}>"all static"</p>"#,
            r#""page"; <p title={text!("")} data-value={self.value}>"suffix"</p>"#,
            r#""page"; <a href={text!("/safe?x=1&y=2")}>"link"</a>"#,
            r#""page"; <p title={text!("value: {}", self.value)}>"suffix"</p>"#,
        ] {
            let input: proa_dsl::dev_hot::HotInput = syn::parse_str(source).unwrap();
            let template = proa_dsl::dev_hot::compile(&input).unwrap();
            let hot = generate_optimized_hot(
                &input.root,
                "/tmp/manifest.json",
                "page",
                &template.shape,
                template.slots.len(),
            )
            .to_string();
            assert_eq!(
                hot.matches("write_slot").count(),
                template.slots.len(),
                "{source}",
            );
        }
    }

    #[test]
    fn ordinary_emission_keeps_literal_writes_and_hot_emission_keeps_dynamic_children() {
        let root: HtmlRoot =
            syn::parse_str(r#"<p class="example">"prefix"{self.value}"suffix"</p>"#).unwrap();
        let normal = generate_optimized(&root).to_string();
        assert!(!normal.contains("dev_hot"));
        let hot =
            generate_optimized_hot(&root, "/tmp/manifest.json", "page", "shape", 2).to_string();
        assert!(hot.contains("self . value"));
        assert_eq!(hot.matches("write_slot").count(), 2);
        assert_eq!(hot.matches("dev_hot :: load").count(), 1);
        let all_static: HtmlRoot = syn::parse_str(r#"<p>"static"</p>"#).unwrap();
        assert_eq!(
            generate_optimized_hot(&all_static, "/tmp/m.json", "id", "shape", 1)
                .to_string()
                .matches("write_slot")
                .count(),
            1
        );
    }
}
