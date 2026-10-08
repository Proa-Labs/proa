// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared syntax and literal format plans for `text!` consumers.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, LitStr, Result, Token};

pub struct TextMacroInput {
    pub format: LitStr,
    pub args: Vec<Expr>,
}

impl Parse for TextMacroInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let format = input.parse()?;
        let mut args = Vec::new();

        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            args.push(input.parse()?);
        }

        if !input.is_empty() {
            return Err(input.error("text!: expected `,` followed by Rust expressions"));
        }

        Ok(Self { format, args })
    }
}

#[derive(Clone, Copy)]
pub enum FormatBound {
    Display,
    Debug,
    LowerHex,
    UpperHex,
    Octal,
    Binary,
    Pointer,
    LowerExp,
    UpperExp,
}

#[derive(Clone, Copy)]
pub struct IntegerFormatPlan {
    pub fill: char,
    pub align: u8,
    pub sign_plus: bool,
    pub alternate: bool,
    pub zero_pad: bool,
    pub width: usize,
    pub precision: Option<usize>,
    pub kind: u8,
}

impl IntegerFormatPlan {
    pub fn tokens(self) -> TokenStream2 {
        let Self {
            fill,
            align,
            sign_plus,
            alternate,
            zero_pad,
            width,
            precision,
            kind,
        } = self;
        let precision = match precision {
            Some(precision) => quote! { ::core::option::Option::Some(#precision) },
            None => quote! { ::core::option::Option::None },
        };
        quote! {
            ::proa_core::shared::numeric::__ProaIntegerFormatPlan::__proa_new(
                #fill,
                #align,
                #sign_plus,
                #alternate,
                #zero_pad,
                #width,
                #precision,
                #kind,
            )
        }
    }
}

impl FormatBound {
    pub fn tokens(self) -> TokenStream2 {
        match self {
            Self::Display => quote! { ::core::fmt::Display },
            Self::Debug => quote! { ::core::fmt::Debug },
            Self::LowerHex => quote! { ::core::fmt::LowerHex },
            Self::UpperHex => quote! { ::core::fmt::UpperHex },
            Self::Octal => quote! { ::core::fmt::Octal },
            Self::Binary => quote! { ::core::fmt::Binary },
            Self::Pointer => quote! { ::core::fmt::Pointer },
            Self::LowerExp => quote! { ::core::fmt::LowerExp },
            Self::UpperExp => quote! { ::core::fmt::UpperExp },
        }
    }
}

pub enum Segment {
    Static(String),
    Bare {
        index: usize,
    },
    Formatted {
        index: usize,
        format: String,
        bound: FormatBound,
        fixed_precision: Option<usize>,
        integer_plan: Option<IntegerFormatPlan>,
    },
}

/// A successfully parsed `text!(...)` invocation.
///
/// This is shared by the standalone proc macro and the `html!` /
/// `html_sync!` intrinsic lowering path. Keeping the parser here ensures the
/// inline fast path accepts exactly the same template grammar as standalone
/// `text!` instead of growing a subtly different parser.
pub struct ParsedText {
    pub template: LitStr,
    pub segments: Vec<Segment>,
    pub args: Vec<Expr>,
    pub span: Span,
}

pub fn parse_input(input: TextMacroInput) -> Result<ParsedText> {
    let span = input.format.span();
    let segments = parse_segments(&input.format.value(), input.args.len(), span)?;
    Ok(ParsedText {
        template: input.format,
        segments,
        args: input.args,
        span,
    })
}

/// Parse an expression when it is a direct `text!(...)` invocation.
///
/// Only an unqualified `text!(...)` or the canonical
/// `proa_macros::text!(...)` path (with an optional leading `::`) is intrinsic.
/// This deliberately excludes unrelated namespaced macros such as
/// `foreign_dsl::text!(...)`. `None` means the expression is not intrinsic;
/// `Some(Err(_))` means it is intrinsic with invalid input and should surface
/// the standalone macro's diagnostic.
pub fn parse_expr(expr: &Expr) -> Option<Result<ParsedText>> {
    let mut expr = expr;
    loop {
        expr = match expr {
            Expr::Group(group) => group.expr.as_ref(),
            Expr::Paren(paren) => paren.expr.as_ref(),
            expr => expr,
        };
        if !matches!(expr, Expr::Group(_) | Expr::Paren(_)) {
            break;
        }
    }
    let Expr::Macro(expr_macro) = expr else {
        return None;
    };
    if !is_intrinsic_path(&expr_macro.mac.path) {
        return None;
    }

    Some(syn::parse2::<TextMacroInput>(expr_macro.mac.tokens.clone()).and_then(parse_input))
}

fn is_intrinsic_path(path: &syn::Path) -> bool {
    let mut segments = path.segments.iter();
    match (segments.next(), segments.next(), segments.next()) {
        (Some(text), None, None) => {
            path.leading_colon.is_none()
                && text.ident == "text"
                && matches!(text.arguments, syn::PathArguments::None)
        }
        (Some(crate_name), Some(text), None) => {
            crate_name.ident == "proa_macros"
                && matches!(crate_name.arguments, syn::PathArguments::None)
                && text.ident == "text"
                && matches!(text.arguments, syn::PathArguments::None)
        }
        _ => false,
    }
}

/// Append a static chunk, coalescing with a preceding static segment so
/// escaped braces do not split one static run into several
/// `extend_static` calls.
fn push_static(segments: &mut Vec<Segment>, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    if let Some(Segment::Static(last)) = segments.last_mut() {
        last.push_str(chunk);
    } else {
        segments.push(Segment::Static(chunk.to_owned()));
    }
}

fn parse_segments(template: &str, arg_count: usize, span: Span) -> Result<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut placeholders = 0usize;
    let mut chunk_start = 0usize;
    let mut iter = template.char_indices().peekable();

    while let Some((idx, ch)) = iter.next() {
        match ch {
            '{' => {
                // `{{` is a literal `{`, matching `format!` and runtime templates
                // template dialect's `text!`.
                if matches!(iter.peek(), Some((_, '{'))) {
                    push_static(&mut segments, &template[chunk_start..=idx]);
                    iter.next();
                    chunk_start = idx + 2;
                    continue;
                }

                if chunk_start < idx {
                    push_static(&mut segments, &template[chunk_start..idx]);
                }

                let content_start = idx + ch.len_utf8();
                let mut close_idx = None;

                for (inner_idx, inner_ch) in iter.by_ref() {
                    match inner_ch {
                        '{' => {
                            return Err(text_error(
                                span,
                                "malformed text! placeholder: nested `{` is not supported",
                            ));
                        }
                        '}' => {
                            close_idx = Some(inner_idx);
                            break;
                        }
                        _ => {}
                    }
                }

                let close_idx = close_idx.ok_or_else(|| {
                    text_error(span, "malformed text! placeholder: missing closing `}`")
                })?;
                let content = &template[content_start..close_idx];
                let index = placeholders;
                placeholders += 1;

                if content.is_empty() {
                    segments.push(Segment::Bare { index });
                } else if content.starts_with(':') {
                    validate_format_spec(content, span)?;
                    segments.push(Segment::Formatted {
                        index,
                        format: format!("{{{content}}}"),
                        bound: format_bound(content),
                        fixed_precision: parse_fixed_precision(content),
                        integer_plan: parse_integer_format_plan(content),
                    });
                } else {
                    return Err(text_error(
                        span,
                        "named and positional placeholders are not supported in text! v1; use `{}` for Proa render/attr behavior or `{:...}` for Rust fmt streamed through Proa escaping",
                    ));
                }

                chunk_start = close_idx + '}'.len_utf8();
            }
            '}' => {
                // `}}` is a literal `}`, matching `format!` and runtime templates
                // template dialect's `text!`.
                if matches!(iter.peek(), Some((_, '}'))) {
                    push_static(&mut segments, &template[chunk_start..=idx]);
                    iter.next();
                    chunk_start = idx + 2;
                    continue;
                }
                return Err(text_error(
                    span,
                    "malformed text! placeholder: unmatched `}`",
                ));
            }
            _ => {}
        }
    }

    if chunk_start < template.len() {
        push_static(&mut segments, &template[chunk_start..]);
    }

    if placeholders != arg_count {
        return Err(text_error(
            span,
            &format!(
                "text!: placeholder count ({placeholders}) does not match argument count ({arg_count}); `{}` uses Proa render/attr behavior, while `{{:...}}` uses Rust fmt streamed through Proa escaping",
                "{}"
            ),
        ));
    }

    Ok(segments)
}

fn validate_format_spec(spec: &str, span: Span) -> Result<()> {
    // `*` and `$` are also valid fill characters (`{:*^8}`, `{:$^8}`).
    // Only `.*` and a `$` outside the leading fill position consume another
    // formatting argument.
    let dollar_capture = spec.match_indices('$').any(|(index, _)| {
        index != 1 || !matches!(spec.as_bytes().get(2), Some(b'<') | Some(b'>') | Some(b'^'))
    });
    if dollar_capture || spec.contains(".*") {
        return Err(text_error(
            span,
            "text!: format specs with named/positional width or precision captures are not supported in v1",
        ));
    }
    Ok(())
}

/// Recognize the common fixed-decimal spelling (`{:.2}`, `{:.6}`, ...).
/// More elaborate Rust format specs retain the generic `core::fmt` path.
fn parse_fixed_precision(spec: &str) -> Option<usize> {
    let digits = spec.strip_prefix(":.")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Parse the literal subset of Rust's format grammar supported by primitive
/// integers. The original `format_args!` expression is still emitted as the
/// generic fallback, so Rust remains the final validator for every spec.
fn parse_integer_format_plan(spec: &str) -> Option<IntegerFormatPlan> {
    const ALIGN_NONE: u8 = 0;
    const ALIGN_LEFT: u8 = 1;
    const ALIGN_RIGHT: u8 = 2;
    const ALIGN_CENTER: u8 = 3;

    const KIND_DISPLAY: u8 = 0;
    const KIND_BINARY: u8 = 1;
    const KIND_OCTAL: u8 = 2;
    const KIND_LOWER_HEX: u8 = 3;
    const KIND_UPPER_HEX: u8 = 4;
    const KIND_LOWER_EXP: u8 = 5;
    const KIND_UPPER_EXP: u8 = 6;

    let mut rest = spec.strip_prefix(':')?;
    let (kind, without_kind) = match rest.chars().last() {
        Some('b') => (KIND_BINARY, &rest[..rest.len() - 1]),
        Some('o') => (KIND_OCTAL, &rest[..rest.len() - 1]),
        Some('x') => (KIND_LOWER_HEX, &rest[..rest.len() - 1]),
        Some('X') => (KIND_UPPER_HEX, &rest[..rest.len() - 1]),
        Some('e') => (KIND_LOWER_EXP, &rest[..rest.len() - 1]),
        Some('E') => (KIND_UPPER_EXP, &rest[..rest.len() - 1]),
        Some('?') | Some('p') => return None,
        _ => (KIND_DISPLAY, rest),
    };
    rest = without_kind;

    let mut fill = ' ';
    let mut align = ALIGN_NONE;
    let mut chars = rest.char_indices();
    if let Some((_, first)) = chars.next() {
        if let Some((second_index, second)) = chars.next() {
            if let Some(parsed_align) =
                parse_alignment(second, ALIGN_LEFT, ALIGN_RIGHT, ALIGN_CENTER)
            {
                fill = first;
                align = parsed_align;
                rest = &rest[second_index + second.len_utf8()..];
            } else if let Some(parsed_align) =
                parse_alignment(first, ALIGN_LEFT, ALIGN_RIGHT, ALIGN_CENTER)
            {
                align = parsed_align;
                rest = &rest[first.len_utf8()..];
            }
        } else if let Some(parsed_align) =
            parse_alignment(first, ALIGN_LEFT, ALIGN_RIGHT, ALIGN_CENTER)
        {
            align = parsed_align;
            rest = &rest[first.len_utf8()..];
        }
    }

    let sign_plus = if let Some(after) = rest.strip_prefix('+') {
        rest = after;
        true
    } else {
        if let Some(after) = rest.strip_prefix('-') {
            rest = after;
        }
        false
    };
    let alternate = if let Some(after) = rest.strip_prefix('#') {
        rest = after;
        true
    } else {
        false
    };
    let zero_pad = if let Some(after) = rest.strip_prefix('0') {
        rest = after;
        true
    } else {
        false
    };

    let width_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let width = if width_len == 0 {
        0
    } else {
        rest[..width_len].parse().ok()?
    };
    rest = &rest[width_len..];

    let precision = if let Some(after_dot) = rest.strip_prefix('.') {
        let precision_len = after_dot.bytes().take_while(u8::is_ascii_digit).count();
        if precision_len == 0 {
            return None;
        }
        let precision = after_dot[..precision_len].parse().ok()?;
        rest = &after_dot[precision_len..];
        Some(precision)
    } else {
        None
    };

    rest.is_empty().then_some(IntegerFormatPlan {
        fill,
        align,
        sign_plus,
        alternate,
        zero_pad,
        width,
        precision,
        kind,
    })
}

#[inline]
fn parse_alignment(value: char, left: u8, right: u8, center: u8) -> Option<u8> {
    match value {
        '<' => Some(left),
        '>' => Some(right),
        '^' => Some(center),
        _ => None,
    }
}

fn format_bound(spec: &str) -> FormatBound {
    let rest = &spec[1..];
    if rest.ends_with('?') {
        return FormatBound::Debug;
    }

    match rest.chars().last() {
        Some('x') => FormatBound::LowerHex,
        Some('X') => FormatBound::UpperHex,
        Some('o') => FormatBound::Octal,
        Some('b') => FormatBound::Binary,
        Some('p') => FormatBound::Pointer,
        Some('e') => FormatBound::LowerExp,
        Some('E') => FormatBound::UpperExp,
        _ => FormatBound::Display,
    }
}

fn text_error(span: Span, message: &str) -> syn::Error {
    syn::Error::new(span, message)
}
