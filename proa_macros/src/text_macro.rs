// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::static_consolidation::{
    escape_attr_compile_time, escape_text_compile_time, MarkupDialect,
};
use proc_macro::TokenStream;
use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{Expr, LitStr};

pub(crate) use proa_dsl::text::{
    parse_expr, parse_input, IntegerFormatPlan, ParsedText, Segment, TextMacroInput,
};

pub fn expand(input: TokenStream) -> TokenStream {
    match syn::parse::<TextMacroInput>(input) {
        Ok(input) => expand_inner(input).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_inner(input: TextMacroInput) -> TokenStream2 {
    let parsed = match parse_input(input) {
        Ok(parsed) => parsed,
        Err(err) => return err.to_compile_error(),
    };

    generate_text_value(parsed.segments, parsed.args, parsed.span)
}

fn generate_text_value(segments: Vec<Segment>, args: Vec<Expr>, span: Span) -> TokenStream2 {
    let type_idents: Vec<_> = (0..args.len())
        .map(|i| format_ident!("__ProaTextArg{}", i, span = span))
        .collect();
    let field_idents: Vec<_> = (0..args.len())
        .map(|i| format_ident!("__proa_arg_{}", i, span = span))
        .collect();
    let struct_ident = format_ident!("__ProaText", span = span);
    let struct_generics = if type_idents.is_empty() {
        quote! {}
    } else {
        quote! { <#(#type_idents),*> }
    };
    let impl_generics = if type_idents.is_empty() {
        quote! { L, B }
    } else {
        quote! { #(#type_idents,)* L, B }
    };
    let attr_impl_generics = if type_idents.is_empty() {
        quote! {}
    } else {
        quote! { <#(#type_idents),*> }
    };

    let field_defs = field_idents
        .iter()
        .zip(type_idents.iter())
        .map(|(field, ty)| quote! { #field: #ty });
    let wrapped_args = wrap_format_args(&segments, &args, span);
    let field_inits = field_idents
        .iter()
        .zip(wrapped_args.iter())
        .map(|(field, expr)| quote! { #field: #expr });

    let text_statements = segments
        .iter()
        .map(|segment| render_text_segment(segment, &field_idents, span));
    let attr_statements = segments
        .iter()
        .map(|segment| render_attr_segment(segment, &field_idents, span));

    let sync_bounds = segment_bounds(&segments, &type_idents, RenderContext::Text);
    let attr_bounds = segment_bounds(&segments, &type_idents, RenderContext::Attr);
    let attr_where = if attr_bounds.is_empty() {
        quote! {}
    } else {
        quote! {
            where
                #(#attr_bounds,)*
        }
    };

    quote! {{
        struct #struct_ident #struct_generics {
            #(#field_defs),*
        }

        impl<#impl_generics> ::proa_core::WebRenderSync<L, B> for #struct_ident #struct_generics
        where
            B: ::proa_core::WriteBuf,
            L: ::proa_core::DataLoader,
            #(#sync_bounds,)*
        {
            #[inline(always)]
            fn render(self, cx: &mut ::proa_core::WebContext<L, B>) -> Result<(), ::proa_core::WriteError> {
                #(#text_statements)*
                Ok(())
            }
        }

        impl<#impl_generics> ::proa_core::WebRender<L, B> for #struct_ident #struct_generics
        where
            B: ::proa_core::WriteBuf,
            L: ::proa_core::DataLoader,
            Self: ::proa_core::WebRenderSync<L, B>,
        {
            fn render(
                self,
                cx: &mut ::proa_core::WebContext<L, B>,
            ) -> ::proa_core::RenderOutcome<impl ::core::future::Future<Output = Result<(), ::proa_core::WriteError>>>
            {
                ::proa_core::RenderOutcome::<::std::future::Ready<Result<(), ::proa_core::WriteError>>>::Done(
                    ::proa_core::WebRenderSync::render(self, cx),
                )
            }
        }

        impl #attr_impl_generics ::proa_core::shared::attr_value::AttrValue for #struct_ident #struct_generics
        #attr_where
        {
            #[inline(always)]
            fn should_render(&self) -> bool {
                true
            }

            #[inline(always)]
            fn is_boolean_attr(&self) -> bool {
                false
            }

            #[inline(always)]
            fn render_attr_value<B: ::proa_core::WriteBuf, L: ::proa_core::DataLoader>(
                &self,
                cx: &mut ::proa_core::WebContext<L, B>,
            ) -> Result<(), ::proa_core::WriteError> {
                #(#attr_statements)*
                Ok(())
            }

            #[inline(always)]
            fn render_url_attr<B: ::proa_core::WriteBuf, L: ::proa_core::DataLoader>(
                &self,
                name_prefix: &'static [u8],
                _name_bool: &'static [u8],
                cx: &mut ::proa_core::WebContext<L, B>,
            ) -> Result<(), ::proa_core::WriteError> {
                // A stored text! value uses one streaming type and does not
                // expose a complete raw value through `url_str()`. Direct,
                // placeholder-free use inside html! is validated and folded
                // before this type is generated; stored values fail closed so
                // a scheme assembled across placeholders cannot bypass the
                // whole-URL guard.
                debug_assert!(
                    false,
                    "a stored text!(...) value cannot be rendered as a URL attribute because its complete value cannot be validated; keep a placeholder-free literal directly inline in html!, build a validated URL value, or use a typed URL helper"
                );
                ::proa_core::WriteBuf::extend_static(&mut cx.out, name_prefix)?;
                ::proa_core::WriteBuf::extend_static(&mut cx.out, b"\"")
            }
        }

        #struct_ident {
            #(#field_inits),*
        }
    }}
}

enum RenderContext {
    Text,
    Attr,
}

fn segment_bounds(
    segments: &[Segment],
    type_idents: &[proc_macro2::Ident],
    context: RenderContext,
) -> Vec<TokenStream2> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Static(_) => None,
            Segment::Bare { index } => {
                let ty = &type_idents[*index];
                match context {
                    RenderContext::Text => Some(quote! { #ty: ::proa_core::WebRenderSync<L, B> }),
                    RenderContext::Attr => {
                        Some(quote! { #ty: ::proa_core::shared::attr_value::AttrValue })
                    }
                }
            }
            Segment::Formatted {
                index,
                fixed_precision: Some(precision),
                ..
            } => {
                let ty = &type_idents[*index];
                Some(quote! {
                    #ty: ::proa_core::shared::numeric::__ProaFixedFormatValue<#precision>
                })
            }
            Segment::Formatted {
                index,
                integer_plan: Some(_),
                ..
            } => {
                let ty = &type_idents[*index];
                Some(quote! {
                    #ty: ::proa_core::shared::numeric::__ProaIntegerFormatValue
                })
            }
            Segment::Formatted { index, bound, .. } => {
                let ty = &type_idents[*index];
                let bound = bound.tokens();
                Some(quote! { #ty: #bound })
            }
        })
        .collect()
}

fn render_text_segment(
    segment: &Segment,
    field_idents: &[proc_macro2::Ident],
    span: Span,
) -> TokenStream2 {
    match segment {
        Segment::Static(chunk) => {
            let mut escaped = String::new();
            escape_text_compile_time(chunk, &mut escaped, MarkupDialect::Html);
            static_write(&escaped)
        }
        Segment::Bare { index } => {
            let field = &field_idents[*index];
            quote! {
                ::proa_core::WebRenderSync::render(self.#field, cx)?;
            }
        }
        Segment::Formatted {
            index,
            fixed_precision: Some(precision),
            ..
        } => {
            let field = &field_idents[*index];
            quote! {
                ::proa_core::shared::numeric::__ProaFixedFormatValue::<#precision>::__proa_render_fixed_text(
                    self.#field,
                    cx,
                )?;
            }
        }
        Segment::Formatted {
            index,
            integer_plan: Some(_),
            ..
        } => {
            let field = &field_idents[*index];
            quote! {
                ::proa_core::shared::numeric::__ProaIntegerFormatValue::__proa_render_integer_text(
                    self.#field,
                    cx,
                )?;
            }
        }
        Segment::Formatted { index, format, .. } => {
            let field = &field_idents[*index];
            let format = LitStr::new(format, span);
            quote! {
                ::proa_core::shared::text::__proa_render_fmt_text(::core::format_args!(#format, self.#field), cx)?;
            }
        }
    }
}

fn render_attr_segment(
    segment: &Segment,
    field_idents: &[proc_macro2::Ident],
    span: Span,
) -> TokenStream2 {
    match segment {
        Segment::Static(chunk) => {
            let mut escaped = String::new();
            escape_attr_compile_time(chunk, &mut escaped, MarkupDialect::Html);
            static_write(&escaped)
        }
        Segment::Bare { index } => {
            let field = &field_idents[*index];
            quote! {
                ::proa_core::shared::attr_value::AttrValue::render_attr_value(&self.#field, cx)?;
            }
        }
        Segment::Formatted {
            index,
            fixed_precision: Some(precision),
            ..
        } => {
            let field = &field_idents[*index];
            quote! {
                ::proa_core::shared::numeric::__ProaFixedFormatValue::<#precision>::__proa_render_fixed_attr(
                    &self.#field,
                    cx,
                )?;
            }
        }
        Segment::Formatted {
            index,
            integer_plan: Some(_),
            ..
        } => {
            let field = &field_idents[*index];
            quote! {
                ::proa_core::shared::numeric::__ProaIntegerFormatValue::__proa_render_integer_attr(
                    &self.#field,
                    cx,
                )?;
            }
        }
        Segment::Formatted { index, format, .. } => {
            let field = &field_idents[*index];
            let format = LitStr::new(format, span);
            quote! {
                ::proa_core::shared::attr_value::__proa_render_fmt_attr(::core::format_args!(#format, self.#field), cx)?;
            }
        }
    }
}

pub(crate) fn wrap_format_args(
    segments: &[Segment],
    args: &[Expr],
    span: Span,
) -> Vec<TokenStream2> {
    enum Strategy<'a> {
        Bare,
        Fixed(usize),
        Integer(IntegerFormatPlan, &'a str),
    }

    let mut strategies = (0..args.len()).map(|_| Strategy::Bare).collect::<Vec<_>>();
    for segment in segments {
        match segment {
            Segment::Formatted {
                index,
                fixed_precision: Some(precision),
                ..
            } => strategies[*index] = Strategy::Fixed(*precision),
            Segment::Formatted {
                index,
                format,
                integer_plan: Some(plan),
                ..
            } => strategies[*index] = Strategy::Integer(*plan, format),
            _ => {}
        }
    }

    args.iter()
        .zip(strategies)
        .map(|(expr, strategy)| match strategy {
            Strategy::Fixed(precision) => quote! {{
                use ::proa_core::shared::numeric::__ProaWrapFixed as _;
                let mut __proa_fixed_arg = ::core::option::Option::Some(#expr);
                (&mut __proa_fixed_arg).__proa_wrap_fixed::<#precision>()
            }},
            Strategy::Integer(plan, format) => {
                let plan = plan.tokens();
                let format = LitStr::new(format, span);
                quote! {{
                    use ::proa_core::shared::numeric::__ProaWrapInteger as _;
                    let mut __proa_integer_arg = ::core::option::Option::Some(#expr);
                    (&mut __proa_integer_arg).__proa_wrap_integer(
                        #plan,
                        |__proa_value, __proa_writer| {
                            ::core::fmt::Write::write_fmt(
                                __proa_writer,
                                ::core::format_args!(#format, __proa_value),
                            )
                        },
                    )
                }}
            }
            Strategy::Bare => quote! { #expr },
        })
        .collect()
}

fn static_write(escaped: &str) -> TokenStream2 {
    if escaped.is_empty() {
        return quote! {};
    }

    let bytes = Literal::byte_string(escaped.as_bytes());
    quote! {
        ::proa_core::WriteBuf::extend_static(&mut cx.out, #bytes)?;
    }
}
