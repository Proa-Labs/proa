// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Map, Value};
use syn::{braced, bracketed, parse::Parse, parse::ParseStream, Lit, Token};

pub(crate) struct JsonLiteral(pub Value);

impl Parse for JsonLiteral {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let value = if input.peek(syn::token::Brace) {
            let fields;
            braced!(fields in input);
            let mut object = Map::new();
            while !fields.is_empty() {
                let key: syn::LitStr = fields.parse()?;
                fields.parse::<Token![:]>()?;
                let value: JsonLiteral = fields.parse()?;
                if object.insert(key.value(), value.0).is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate JSON object key"));
                }
                if !fields.is_empty() {
                    fields.parse::<Token![,]>()?;
                }
            }
            Value::Object(object)
        } else if input.peek(syn::token::Bracket) {
            let elements;
            bracketed!(elements in input);
            let mut array = Vec::new();
            while !elements.is_empty() {
                array.push(elements.parse::<JsonLiteral>()?.0);
                if !elements.is_empty() {
                    elements.parse::<Token![,]>()?;
                }
            }
            Value::Array(array)
        } else if input.peek(syn::Ident) {
            let ident: syn::Ident = input.parse()?;
            if ident != "null" {
                return Err(syn::Error::new(
                    ident.span(),
                    "const_json! accepts JSON literals only; serialize runtime values with a typed serializer",
                ));
            }
            Value::Null
        } else {
            let negative = input.parse::<Option<Token![-]>>()?;
            let literal: Lit = input.parse()?;
            match literal {
                Lit::Str(value) if negative.is_none() => Value::String(value.value()),
                Lit::Bool(value) if negative.is_none() => Value::Bool(value.value),
                Lit::Int(ref value) if value.suffix().is_empty() => {
                    parse_number(&literal, negative.is_some())?
                }
                Lit::Float(ref value) if value.suffix().is_empty() => {
                    parse_number(&literal, negative.is_some())?
                }
                _ => return Err(syn::Error::new_spanned(literal, "expected a JSON literal")),
            }
        };
        Ok(Self(value))
    }
}

fn parse_number(literal: &Lit, negative: bool) -> syn::Result<Value> {
    use quote::ToTokens;
    let sign = if negative { "-" } else { "" };
    let source = format!("{sign}{}", literal.to_token_stream());
    let value: Value = serde_json::from_str(&source).map_err(|error| {
        syn::Error::new_spanned(literal, format!("invalid JSON number: {error}"))
    })?;
    if matches!(literal, Lit::Int(_)) {
        let in_range = if negative {
            source.parse::<i64>().is_ok()
        } else {
            source.parse::<u64>().is_ok()
        };
        if !in_range {
            return Err(syn::Error::new_spanned(
                literal,
                "integer outside the supported i64/u64 range",
            ));
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::JsonLiteral;

    #[test]
    fn rejects_runtime_expressions_duplicate_keys_and_non_json_literals() {
        for source in [
            r#"{"a": runtime_value}"#,
            r#"{"a": 1, "a": 2}"#,
            r#"{"a": 0xff}"#,
            r#"{"a": 10_u32}"#,
            r#"{"a": 'x'}"#,
            r#"{"a": -true}"#,
            r#"{"a": 1e999}"#,
            r#"{"a": 18446744073709551616}"#,
            r#"{"a": -9223372036854775809}"#,
            r#"{"a": [1 2]}"#,
        ] {
            assert!(syn::parse_str::<JsonLiteral>(source).is_err(), "{source}");
        }
    }
}
