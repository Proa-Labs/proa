// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared syntax for content-coverage intent. These markers describe intent;
//! `proa lint` performs the comparison, not the compiler macros.

use syn::parse::{Parse, ParseStream};
use syn::{Expr, LitStr, Token, Type};

/// Policy on one render implementation or its entry method.
#[derive(Clone, Debug, Default)]
pub struct ContentPolicy {
    pub skip: bool,
    pub reason: Option<LitStr>,
    pub source: Option<Type>,
    pub from: Option<LitStr>,
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
                _ => {
                    return Err(syn::Error::new(
                        option.span(),
                        "expected `skip`, `reason`, `source`, or `from`",
                    ))
                }
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        if policy.skip && (policy.source.is_some() || policy.from.is_some()) {
            return Err(input.error("`skip` exempts the whole view; use it alone or with `reason`"));
        }
        Ok(policy)
    }
}

/// Select exactly one contribution by local/field provenance, child, or role.
#[derive(Clone, Debug)]
pub enum OmitSelector {
    Value(Expr),
    Child(Type),
    Role(LitStr),
}

#[derive(Clone, Debug)]
pub struct OmitContent {
    pub selector: OmitSelector,
    pub reason: Option<LitStr>,
    pub from: Option<LitStr>,
}

impl Parse for OmitContent {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let selector = parse_selector(input)?;
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

fn parse_selector(input: ParseStream) -> syn::Result<OmitSelector> {
    if input.peek(LitStr) {
        return Ok(OmitSelector::Role(nonempty(
            input.parse()?,
            "content role",
        )?));
    }
    // Bare paths are types. Fields and borrowed locals are values, so the
    // compiler and linter never have to guess from identifier capitalization.
    let probe = input.fork();
    if let Ok(value) = probe.parse::<Expr>() {
        if probe.is_empty() || probe.peek(Token![,]) {
            match &value {
                Expr::Field(_) | Expr::Reference(_) if is_value_path(&value) => {
                    return Ok(OmitSelector::Value(input.parse()?));
                }
                Expr::Path(_) => {}
                _ => return Err(syn::Error::new_spanned(value, SELECTOR_HELP)),
            }
        }
    }
    let ty: Type = input.parse().map_err(|_| input.error(SELECTOR_HELP))?;
    if !matches!(ty, Type::Path(_)) {
        return Err(syn::Error::new_spanned(ty, SELECTOR_HELP));
    }
    Ok(OmitSelector::Child(ty))
}

const SELECTOR_HELP: &str = "expected a child type (`Impact<'_>`), field (`self.image`), borrowed local (`&notice`), or named part (`\"stock-status\"`)";

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
        Expr::Reference(reference) => is_value_path(&reference.expr),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_accepts_both_exemption_and_pairing() {
        assert!(syn::parse_str::<ContentPolicy>("skip").unwrap().skip);
        let policy =
            syn::parse_str::<ContentPolicy>("source = HtmlProduct, from = \"html\"").unwrap();
        assert!(policy.source.is_some());
        assert_eq!(policy.from.unwrap().value(), "html");
    }
    #[test]
    fn malformed_policy_is_actionable() {
        for input in [
            "skip, source = Product",
            "skip, skip",
            "reason = \"\"",
            "require = []",
            "require = \"markdown\"",
            "require = [\"markdown\", \"plain\"]",
            "skpi",
        ] {
            assert!(syn::parse_str::<ContentPolicy>(input).is_err(), "{input}");
        }
    }
    #[test]
    fn all_omission_scopes_parse() {
        for input in [
            "self.image",
            "&notice, \"Browser-only\"",
            "crate::Notice",
            "Notice<'_>",
            "Notice<'_, String>",
            "\"restock\", from = \"html\"",
        ] {
            syn::parse_str::<OmitContent>(input).unwrap();
        }
    }
    #[test]
    fn omission_never_accepts_evaluated_calls() {
        for input in [
            "expensive()",
            "child Notice",
            "role \"notice\"",
            "child::<Notice>(value)",
            "child::<Notice>()",
            "&expensive()",
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
            "&child",
            "&role",
            "child.image",
            "&role, \"reason\"",
            "&child, from=\"html\"",
        ] {
            assert!(matches!(
                syn::parse_str::<OmitContent>(input).unwrap().selector,
                OmitSelector::Value(_)
            ));
        }
        for input in [
            "crate::Notice",
            "module::Notice<String>",
            "lowercase_type",
            "\"notice\"",
        ] {
            assert!(!matches!(
                syn::parse_str::<OmitContent>(input).unwrap().selector,
                OmitSelector::Value(_)
            ));
        }
    }

    #[test]
    fn type_and_value_selectors_do_not_guess_from_names() {
        for input in ["Impact", "impact", "crate::Impact<'_>"] {
            assert!(matches!(
                syn::parse_str::<OmitContent>(input).unwrap().selector,
                OmitSelector::Child(_)
            ));
        }
        for input in ["&Impact", "&impact", "self.image", "value.image"] {
            assert!(matches!(
                syn::parse_str::<OmitContent>(input).unwrap().selector,
                OmitSelector::Value(_)
            ));
        }
    }
}
