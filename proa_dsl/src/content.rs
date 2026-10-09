// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared syntax for content-coverage intent. These markers describe intent;
//! `proa lint` performs the comparison, not the compiler macros.

use syn::parse::{Parse, ParseStream};
use syn::{Expr, LitStr, Path, Token, Type};

/// Policy on one render implementation or its entry method.
#[derive(Clone, Debug, Default)]
pub struct ContentPolicy {
    pub skip: bool,
    pub reason: Option<LitStr>,
    pub source: Option<Type>,
    pub from: Option<LitStr>,
    pub require: Vec<LitStr>,
}

impl Parse for ContentPolicy {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut policy = Self::default();
        let mut seen = std::collections::HashSet::new();
        while !input.is_empty() {
            let option: syn::Ident = input.parse()?;
            if !seen.insert(option.to_string()) {
                return Err(syn::Error::new(
                    option.span(),
                    format!("duplicate content option `{option}`"),
                ));
            }
            match option.to_string().as_str() {
                "skip" => policy.skip = true,
                "reason" => {
                    input.parse::<Token![=]>()?;
                    policy.reason = Some(nonempty(input.parse()?, "reason")?);
                }
                "source" => {
                    input.parse::<Token![=]>()?;
                    policy.source = Some(input.parse()?);
                }
                "from" => {
                    input.parse::<Token![=]>()?;
                    policy.from = Some(nonempty(input.parse()?, "source target")?);
                }
                "require" => {
                    input.parse::<Token![=]>()?;
                    if input.peek(syn::token::Bracket) {
                        let list;
                        syn::bracketed!(list in input);
                        let items =
                            syn::punctuated::Punctuated::<LitStr, Token![,]>::parse_terminated(
                                &list,
                            )?;
                        for item in items {
                            policy.require.push(nonempty(item, "required target")?);
                        }
                    } else {
                        policy
                            .require
                            .push(nonempty(input.parse()?, "required target")?);
                    }
                    if policy.require.is_empty() {
                        return Err(input.error("`require` needs at least one target"));
                    }
                }
                _ => {
                    return Err(syn::Error::new(
                        option.span(),
                        "expected `skip`, `reason`, `source`, `from`, or `require`",
                    ))
                }
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        if policy.skip
            && (policy.source.is_some() || policy.from.is_some() || !policy.require.is_empty())
        {
            return Err(input.error("`skip` exempts the whole view; use it alone or with `reason`"));
        }
        Ok(policy)
    }
}

/// Select exactly one contribution by local/field provenance, child, or role.
#[derive(Clone, Debug)]
pub enum OmitSelector {
    Value(Expr),
    Child(Path),
    Role(LitStr),
}

#[derive(Clone, Debug)]
pub struct OmitContent {
    pub selector: OmitSelector,
    pub reason: Option<LitStr>,
    pub from: Option<LitStr>,
}

mod keyword {
    syn::custom_keyword!(child);
    syn::custom_keyword!(role);
}

impl Parse for OmitContent {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let selector = if input.peek(keyword::child)
            && (input.peek2(syn::Ident)
                || input.peek2(Token![::])
                || input.peek2(Token![Self])
                || input.peek2(Token![self])
                || input.peek2(Token![super])
                || input.peek2(Token![crate]))
        {
            input.parse::<keyword::child>()?;
            {
                let path: Path = input.parse()?;
                if path.segments.iter().any(|segment| segment.ident == "Self") {
                    return Err(syn::Error::new_spanned(
                        path,
                        "select a named child type path; use the component name instead of `Self`",
                    ));
                }
                if path
                    .segments
                    .iter()
                    .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
                {
                    return Err(syn::Error::new_spanned(
                        path,
                        "select the child type path without generic arguments",
                    ));
                }
                OmitSelector::Child(path)
            }
        } else if input.peek(keyword::role) && input.peek2(LitStr) {
            input.parse::<keyword::role>()?;
            OmitSelector::Role(nonempty(input.parse()?, "content role")?)
        } else {
            let value: Expr = input.parse()?;
            if !is_value_path(&value) {
                return Err(syn::Error::new_spanned(value, "omit a local or field path, such as `omit_content!(self.image)`; use `child Type` or `role \"name\"` for other contributions"));
            }
            OmitSelector::Value(value)
        };
        let mut omission = Self {
            selector,
            reason: None,
            from: None,
        };
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            if input.peek(LitStr) {
                if omission.reason.is_some() {
                    return Err(input.error("duplicate omission reason"));
                }
                omission.reason = Some(nonempty(input.parse()?, "reason")?);
            } else {
                let option: syn::Ident = input.parse()?;
                if option != "from" {
                    return Err(syn::Error::new(
                        option.span(),
                        "expected an optional reason string or `from = \"target\"`",
                    ));
                }
                if omission.from.is_some() {
                    return Err(input.error("duplicate source target"));
                }
                input.parse::<Token![=]>()?;
                omission.from = Some(nonempty(input.parse()?, "source target")?);
            }
        }
        Ok(omission)
    }
}

/// Give a rendered expression an explicit semantic identity across targets.
#[derive(Clone, Debug)]
pub struct ContentPart {
    pub role: LitStr,
    pub value: Expr,
}

impl Parse for ContentPart {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let role = nonempty(input.parse()?, "content role")?;
        input.parse::<Token![,]>()?;
        let value = input.parse()?;
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
        Ok(Self { role, value })
    }
}

fn nonempty(value: LitStr, label: &str) -> syn::Result<LitStr> {
    if value.value().trim().is_empty() {
        Err(syn::Error::new(
            value.span(),
            format!("{label} must not be empty"),
        ))
    } else {
        Ok(value)
    }
}

fn is_value_path(value: &Expr) -> bool {
    match value {
        Expr::Path(path) => path.qself.is_none(),
        Expr::Field(field) => is_value_path(&field.base),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_accepts_both_exemption_and_pairing() {
        assert!(syn::parse_str::<ContentPolicy>("skip").unwrap().skip);
        let policy = syn::parse_str::<ContentPolicy>(
            "source = HtmlProduct, from = \"html\", require = [\"text\", \"json\"]",
        )
        .unwrap();
        assert_eq!(policy.require.len(), 2);
        assert!(policy.source.is_some());
    }
    #[test]
    fn malformed_policy_is_actionable() {
        for input in [
            "skip, source = Product",
            "skip, skip",
            "reason = \"\"",
            "require = []",
            "skpi",
        ] {
            assert!(syn::parse_str::<ContentPolicy>(input).is_err(), "{input}");
        }
    }
    #[test]
    fn all_omission_scopes_parse() {
        for input in [
            "self.image",
            "notice, \"Browser-only\"",
            "child crate::Notice",
            "role \"restock\", from = \"html\"",
        ] {
            syn::parse_str::<OmitContent>(input).unwrap();
        }
    }
    #[test]
    fn omission_never_accepts_evaluated_calls() {
        for input in [
            "expensive()",
            "self.image()",
            "values[0]",
            "notice, \"\"",
            "notice, from = \"html\", from = \"html\"",
        ] {
            assert!(syn::parse_str::<OmitContent>(input).is_err(), "{input}");
        }
    }
}

#[cfg(test)]
mod selector_name_tests {
    use super::*;
    #[test]
    fn child_and_role_remain_valid_local_variable_names() {
        for input in [
            "child",
            "role",
            "child.image",
            "role, \"reason\"",
            "child, from=\"html\"",
        ] {
            assert!(matches!(
                syn::parse_str::<OmitContent>(input).unwrap().selector,
                OmitSelector::Value(_)
            ));
        }
        for input in [
            "child crate::Notice",
            "child ::module::Notice",
            "role \"notice\"",
        ] {
            assert!(!matches!(
                syn::parse_str::<OmitContent>(input).unwrap().selector,
                OmitSelector::Value(_)
            ));
        }
    }
}
