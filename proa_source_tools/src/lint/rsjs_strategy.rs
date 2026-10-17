// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Debug, Clone)]
pub struct RsjsStrategyCacheOptions {
    pub root: PathBuf,
    pub require_fresh: bool,
}

#[derive(Debug)]
pub(crate) struct RsjsStrategyCache {
    pub(crate) sidecars: Vec<crate::rsjs::StrategySidecarSummary>,
    pub(crate) require_fresh: bool,
}

#[derive(Debug)]
pub(crate) enum RsjsStrategyMatch<'a> {
    Found(&'a crate::rsjs::StrategySidecarSummary),
    Missing,
    Stale(Vec<&'a crate::rsjs::StrategySidecarSummary>),
    Ambiguous(Vec<&'a crate::rsjs::StrategySidecarSummary>),
}

impl RsjsStrategyCache {
    pub(crate) fn load(options: &RsjsStrategyCacheOptions) -> Self {
        Self {
            sidecars: crate::rsjs::strategy_sidecar_summaries(&options.root),
            require_fresh: options.require_fresh,
        }
    }

    pub(crate) fn lookup_impl(&self, item_impl: &ItemImpl) -> RsjsStrategyMatch<'_> {
        let source_fingerprint = rsjs_input_fingerprint_for_impl(item_impl);
        if let Some(fingerprint) = &source_fingerprint {
            let fingerprint_matches: Vec<_> = self
                .sidecars
                .iter()
                .filter(|sidecar| {
                    sidecar.input_fingerprint.as_deref() == Some(fingerprint.as_str())
                        && sidecar_source_matches_impl(sidecar.source_anchor.as_ref(), item_impl)
                })
                .collect();
            if fingerprint_matches.len() == 1 {
                return RsjsStrategyMatch::Found(fingerprint_matches[0]);
            }
            if fingerprint_matches.len() > 1 {
                return RsjsStrategyMatch::Ambiguous(fingerprint_matches);
            }
        }

        let self_ty = item_impl.self_ty.to_token_stream().to_string();
        let exact: Vec<_> = self
            .sidecars
            .iter()
            .filter(|sidecar| sidecar.struct_path == self_ty)
            .collect();
        if exact.len() == 1 {
            if sidecar_is_stale_for_impl(exact[0], source_fingerprint.as_deref(), item_impl) {
                return RsjsStrategyMatch::Stale(exact);
            }
            return RsjsStrategyMatch::Found(exact[0]);
        }
        if exact.len() > 1 {
            if source_fingerprint.is_some()
                && exact
                    .iter()
                    .all(|sidecar| sidecar.input_fingerprint.is_some())
            {
                return RsjsStrategyMatch::Stale(exact);
            }
            return RsjsStrategyMatch::Ambiguous(exact);
        }

        let Some(struct_name) = type_path_last_ident(&item_impl.self_ty) else {
            return RsjsStrategyMatch::Missing;
        };
        let named: Vec<_> = self
            .sidecars
            .iter()
            .filter(|sidecar| sidecar.struct_name == struct_name)
            .collect();
        if named.len() == 1 {
            if sidecar_is_stale_for_impl(named[0], source_fingerprint.as_deref(), item_impl) {
                return RsjsStrategyMatch::Stale(named);
            }
            RsjsStrategyMatch::Found(named[0])
        } else if named.len() > 1 {
            if source_fingerprint.is_some()
                && named
                    .iter()
                    .all(|sidecar| sidecar.input_fingerprint.is_some())
            {
                return RsjsStrategyMatch::Stale(named);
            }
            RsjsStrategyMatch::Ambiguous(named)
        } else {
            RsjsStrategyMatch::Missing
        }
    }
}

pub(crate) fn sidecar_is_stale_for_impl(
    sidecar: &crate::rsjs::StrategySidecarSummary,
    source_fingerprint: Option<&str>,
    item_impl: &ItemImpl,
) -> bool {
    let fingerprint_stale = match (sidecar.input_fingerprint.as_deref(), source_fingerprint) {
        (Some(sidecar), Some(source)) => sidecar != source,
        _ => false,
    };
    let source_anchor_stale = sidecar
        .source_anchor
        .as_ref()
        .is_some_and(|anchor| !sidecar_source_matches_impl(Some(anchor), item_impl));
    fingerprint_stale || source_anchor_stale
}

pub(crate) fn sidecar_source_matches_impl(
    anchor: Option<&crate::rsjs::StrategySourceAnchor>,
    item_impl: &ItemImpl,
) -> bool {
    let Some(anchor) = anchor else {
        return true;
    };
    let start = item_impl.impl_token.span.start();
    let line_matches = anchor.impl_start_line.is_none_or(|line| line == start.line);
    let column_matches = anchor
        .impl_start_column
        .is_none_or(|column| column == start.column + 1);
    line_matches && column_matches
}

pub(crate) fn rsjs_input_fingerprint_for_impl(item_impl: &ItemImpl) -> Option<String> {
    let rsjs_attr = item_impl
        .attrs
        .iter()
        .find(|attr| has_attr(std::slice::from_ref(attr), &["rsjs"]))?;
    let attr_tokens = rsjs_attr_argument_tokens(rsjs_attr)?;
    let mut item_without_rsjs_attr = item_impl.clone();
    item_without_rsjs_attr
        .attrs
        .retain(|attr| !has_attr(std::slice::from_ref(attr), &["rsjs"]));
    Some(rsjs_strategy_input_fingerprint(
        &attr_tokens,
        &item_without_rsjs_attr.to_token_stream(),
    ))
}

pub(crate) fn rsjs_attr_argument_tokens(attr: &SynAttribute) -> Option<TokenStream> {
    match &attr.meta {
        syn::Meta::Path(_) => Some(TokenStream::new()),
        syn::Meta::List(list) => Some(list.tokens.clone()),
        syn::Meta::NameValue(_) => None,
    }
}

pub(crate) fn rsjs_strategy_input_fingerprint(
    attr_tokens: &TokenStream,
    impl_tokens: &TokenStream,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rsjs.strategy.input.v1");
    hasher.update(b"\0attr\0");
    hasher.update(normalized_token_string(attr_tokens).as_bytes());
    hasher.update(b"\0impl\0");
    hasher.update(normalized_token_string(impl_tokens).as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

pub(crate) fn normalized_token_string(tokens: &TokenStream) -> String {
    tokens
        .to_string()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}
