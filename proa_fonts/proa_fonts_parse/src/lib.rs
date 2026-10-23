// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared grammar for `google_font!` / `local_font!`.
//!
//! Both the proc macro (`proa_fonts_macros`) and the build-time collector
//! (`proa_fonts_build`) parse the exact same call tokens through this crate, so
//! the id the macro emits and the id the collector generates are always equal.
//! Keeping one parser is what makes the "declare in code" model sound: there is
//! no second grammar to drift.

pub use proa_fonts_types::request::{FontRequest, Generic, RequestSource};
use proa_fonts_types::FontStyle;
use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, ExprArray, Ident, Lit, LitStr, Token};

/// Parse a `google_font!(...)` argument list into a [`FontRequest`].
pub fn parse_google_font(tokens: TokenStream) -> syn::Result<FontRequest> {
    parse(RequestSource::Google, tokens)
}

/// Parse a `local_font!(...)` argument list into a [`FontRequest`].
pub fn parse_local_font(tokens: TokenStream) -> syn::Result<FontRequest> {
    parse(RequestSource::Local, tokens)
}

fn parse(source: RequestSource, tokens: TokenStream) -> syn::Result<FontRequest> {
    syn::parse2::<FontMacroInput>(tokens)?.into_request(source)
}

/// A family literal followed by `key = value` options.
struct FontMacroInput {
    family: LitStr,
    options: Vec<(Ident, Expr)>,
}

impl Parse for FontMacroInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let family: LitStr = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "expected a font family name as the first argument, e.g. `google_font!(\"Geist\", ...)`",
            )
        })?;

        let mut options = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break; // tolerate a trailing comma
            }
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: Expr = input.parse()?;
            options.push((key, value));
        }
        Ok(Self { family, options })
    }
}

impl FontMacroInput {
    fn into_request(self, source: RequestSource) -> syn::Result<FontRequest> {
        let mut req = match source {
            RequestSource::Google => FontRequest::google(self.family.value()),
            RequestSource::Local => FontRequest::local(self.family.value(), Vec::new()),
        };

        for (key, value) in self.options {
            match key.to_string().as_str() {
                "subsets" => req.subsets = string_list(&value)?,
                "weights" => req.weights = weight_list(&value)?,
                "styles" => req.styles = style_list(&value)?,
                "variable_font" => req.variable_font = bool_value(&value)?,
                "variable" => req.variable = Some(string_value(&value)?),
                "generic" => req.generic = generic_value(&value)?,
                "display" => req.display = Some(string_value(&value)?),
                "fallback" | "fallback_local" => req.fallback_local = Some(string_value(&value)?),
                "extra_stack" => req.extra_stack = string_list(&value)?,
                "files" => req.files = string_list(&value)?,
                "preload" => req.preload = string_list(&value)?,
                "license" => req.license = Some(string_value(&value)?),
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!(
                            "unknown font option `{other}`. Valid options: subsets, weights, styles, \
                             variable_font, variable, generic, display, fallback, extra_stack, files, \
                             preload, license"
                        ),
                    ))
                }
            }
        }

        if source == RequestSource::Local && req.files.is_empty() {
            return Err(syn::Error::new_spanned(
                self.family,
                "local_font! requires `files = [\"...\"]` naming at least one font file",
            ));
        }
        Ok(req)
    }
}

fn as_array(value: &Expr) -> syn::Result<&ExprArray> {
    match value {
        Expr::Array(arr) => Ok(arr),
        _ => Err(syn::Error::new_spanned(
            value,
            "expected a `[...]` list, e.g. `[latin, latin-ext]`",
        )),
    }
}

/// Accept either a bare word (`latin`) or a string literal (`"latin"`),
/// lowercased.
fn token_string(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Path(path) => path
            .path
            .get_ident()
            .map(|id| id.to_string().to_ascii_lowercase())
            .ok_or_else(|| syn::Error::new_spanned(expr, "expected a bare word or a string")),
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(s) => Ok(s.value()),
            _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
        },
        _ => Err(syn::Error::new_spanned(
            expr,
            "expected a bare word (e.g. `latin`) or a string literal",
        )),
    }
}

fn string_list(value: &Expr) -> syn::Result<Vec<String>> {
    as_array(value)?.elems.iter().map(token_string).collect()
}

fn weight_list(value: &Expr) -> syn::Result<Vec<u16>> {
    as_array(value)?
        .elems
        .iter()
        .map(|e| match e {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Int(i) => i.base10_parse::<u16>(),
                _ => Err(syn::Error::new_spanned(
                    e,
                    "expected an integer weight, e.g. 400",
                )),
            },
            _ => Err(syn::Error::new_spanned(
                e,
                "expected an integer weight, e.g. 400",
            )),
        })
        .collect()
}

fn style_list(value: &Expr) -> syn::Result<Vec<FontStyle>> {
    as_array(value)?
        .elems
        .iter()
        .map(|e| match token_string(e)?.as_str() {
            "normal" => Ok(FontStyle::Normal),
            "italic" | "oblique" => Ok(FontStyle::Italic),
            other => Err(syn::Error::new_spanned(
                e,
                format!("unknown style `{other}` (expected `normal` or `italic`)"),
            )),
        })
        .collect()
}

fn generic_value(value: &Expr) -> syn::Result<Generic> {
    match token_string(value)?.as_str() {
        "sans" | "sans-serif" => Ok(Generic::Sans),
        "serif" => Ok(Generic::Serif),
        "mono" | "monospace" => Ok(Generic::Mono),
        other => Err(syn::Error::new_spanned(
            value,
            format!("unknown generic `{other}` (expected `sans`, `serif`, or `mono`)"),
        )),
    }
}

fn string_value(value: &Expr) -> syn::Result<String> {
    match value {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(s) => Ok(s.value()),
            _ => Err(syn::Error::new_spanned(value, "expected a string literal")),
        },
        _ => Err(syn::Error::new_spanned(value, "expected a string literal")),
    }
}

fn bool_value(value: &Expr) -> syn::Result<bool> {
    match value {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Bool(b) => Ok(b.value),
            _ => Err(syn::Error::new_spanned(value, "expected `true` or `false`")),
        },
        _ => Err(syn::Error::new_spanned(value, "expected `true` or `false`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote as q;

    #[test]
    fn parses_a_full_google_declaration() {
        let r = parse_google_font(q!(
            "Geist",
            subsets = [latin, "latin-ext"],
            weights = [400, 700],
            styles = [normal, italic],
            variable = "--font-sans",
            generic = sans,
            extra_stack = ["system-ui"]
        ))
        .unwrap();
        assert_eq!(r.family, "Geist");
        assert_eq!(
            r.subsets,
            vec!["latin".to_string(), "latin-ext".to_string()]
        );
        assert_eq!(r.weights, vec![400, 700]);
        assert_eq!(r.styles, vec![FontStyle::Normal, FontStyle::Italic]);
        assert_eq!(r.variable.as_deref(), Some("--font-sans"));
        assert_eq!(r.generic, Generic::Sans);
        assert_eq!(r.extra_stack, vec!["system-ui".to_string()]);
    }

    #[test]
    fn variable_mono_font() {
        let r = parse_google_font(q!(
            "Geist Mono",
            variable_font = true,
            generic = mono,
            variable = "--font-mono"
        ))
        .unwrap();
        assert!(r.variable_font);
        assert_eq!(r.generic, Generic::Mono);
    }

    #[test]
    fn local_requires_files() {
        assert!(parse_local_font(q!("Brand")).is_err());
        let ok = parse_local_font(q!("Brand", files = ["fonts/Brand.woff2"])).unwrap();
        assert_eq!(ok.source, RequestSource::Local);
        assert_eq!(ok.files, vec!["fonts/Brand.woff2".to_string()]);
    }

    #[test]
    fn unknown_option_is_helpful() {
        let err = parse_google_font(q!("Geist", weight = [400])).unwrap_err();
        assert!(err.to_string().contains("unknown font option `weight`"));
    }
}
