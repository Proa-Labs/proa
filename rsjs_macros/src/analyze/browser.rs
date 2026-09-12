// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Console, browser read/effect, and encoding call tables.
//!
//! Browser helper recognition is registry-first: paths are resolved
//! against [`rsjs_ir::browser_registry`] before the legacy
//! hand-written tables below. The `window`, `local_storage` /
//! `session_storage`, `clipboard`, and `navigator` families already
//! live in the registry; the tables in this module cover only the
//! not-yet-migrated families (`location`, `history`, `document`,
//! `webauthn`, `url`, `encoding`).
//!
//! New browser helpers MUST be declared in
//! `rsjs_ir::browser_registry` instead of extending these tables;
//! existing families migrate opportunistically.
use super::*;

use rsjs_ir::browser_registry;
pub(super) use rsjs_ir::browser_registry::BrowserHelperSpec;

/// Split a call path into `(module, method)`, accepting both the
/// bare `module::method` and the `rsjs::module::method` spellings.
fn helper_path_segments(path: &syn::Path) -> Option<(String, String)> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match segments.as_slice() {
        [module, method] => Some((module.clone(), method.clone())),
        [crate_name, module, method] if crate_name == "rsjs" => {
            Some((module.clone(), method.clone()))
        }
        _ => None,
    }
}

/// Resolve a call path to a registry-declared read helper.
pub(super) fn registry_read_spec(path: &syn::Path) -> Option<&'static BrowserHelperSpec> {
    let (module, method) = helper_path_segments(path)?;
    browser_registry::find_read(&module, &method)
}

/// Resolve a call path to a registry-declared effect helper.
pub(super) fn registry_effect_spec(path: &syn::Path) -> Option<&'static BrowserHelperSpec> {
    let (module, method) = helper_path_segments(path)?;
    browser_registry::find_effect(&module, &method)
}

pub(super) fn console_call_level(path: &syn::Path) -> Option<ConsoleLevel> {
    if path.segments.len() != 2 {
        return None;
    }
    let mut segments = path.segments.iter();
    let module = &segments.next()?.ident;
    if module != "console" {
        return None;
    }
    let method = &segments.next()?.ident;
    match method.to_string().as_str() {
        "log" => Some(ConsoleLevel::Log),
        "info" => Some(ConsoleLevel::Info),
        "debug" => Some(ConsoleLevel::Debug),
        "warn" => Some(ConsoleLevel::Warn),
        "error" => Some(ConsoleLevel::Error),
        "table" => Some(ConsoleLevel::Table),
        "time" => Some(ConsoleLevel::Time),
        "time_end" => Some(ConsoleLevel::TimeEnd),
        "group" => Some(ConsoleLevel::Group),
        "group_end" => Some(ConsoleLevel::GroupEnd),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum BrowserReadCallKind {
    Location(LocationField),
    UrlSearchParam,
    UrlSearchParamOr,
}

pub(super) fn browser_read_call_kind(path: &syn::Path) -> Option<BrowserReadCallKind> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let [module, method] = segments.as_slice() else {
        let [crate_name, module, method] = segments.as_slice() else {
            return None;
        };
        if crate_name.as_str() != "rsjs" {
            return None;
        }
        return browser_read_method(module.as_str(), method.as_str());
    };
    browser_read_method(module.as_str(), method.as_str())
}

pub(super) fn browser_read_method(module: &str, method: &str) -> Option<BrowserReadCallKind> {
    match (module, method) {
        ("location", "href") => Some(BrowserReadCallKind::Location(LocationField::Href)),
        ("location", "origin") => Some(BrowserReadCallKind::Location(LocationField::Origin)),
        ("location", "protocol") => Some(BrowserReadCallKind::Location(LocationField::Protocol)),
        ("location", "host") => Some(BrowserReadCallKind::Location(LocationField::Host)),
        ("location", "hostname") => Some(BrowserReadCallKind::Location(LocationField::Hostname)),
        ("location", "port") => Some(BrowserReadCallKind::Location(LocationField::Port)),
        ("location", "pathname") => Some(BrowserReadCallKind::Location(LocationField::Pathname)),
        ("location", "search") => Some(BrowserReadCallKind::Location(LocationField::Search)),
        ("location", "hash") => Some(BrowserReadCallKind::Location(LocationField::Hash)),
        ("url", "search_param") => Some(BrowserReadCallKind::UrlSearchParam),
        ("url", "search_param_or") => Some(BrowserReadCallKind::UrlSearchParamOr),
        _ => None,
    }
}

pub(super) fn encoding_call_op(path: &syn::Path) -> Option<EncodingOp> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let [module, method] = segments.as_slice() else {
        let [crate_name, module, method] = segments.as_slice() else {
            return None;
        };
        if crate_name.as_str() != "rsjs" {
            return None;
        }
        return encoding_method(module.as_str(), method.as_str());
    };
    encoding_method(module.as_str(), method.as_str())
}

pub(super) fn encoding_method(module: &str, method: &str) -> Option<EncodingOp> {
    match (module, method) {
        ("encoding", "utf8_bytes") => Some(EncodingOp::Utf8Bytes),
        ("encoding", "utf8_string") => Some(EncodingOp::Utf8String),
        ("encoding", "base64_encode") => Some(EncodingOp::Base64Encode),
        ("encoding", "base64_decode") => Some(EncodingOp::Base64Decode),
        ("encoding", "base64url_encode") => Some(EncodingOp::Base64UrlEncode),
        ("encoding", "base64url_decode") => Some(EncodingOp::Base64UrlDecode),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum BrowserEffectCallKind {
    LocationSetHash,
    LocationAssign,
    LocationReplace,
    LocationReload,
    HistoryPushState,
    HistoryReplaceState,
    HistoryBack,
    HistoryForward,
    HistoryGo,
    DocumentDispatchEvent,
    DocumentDispatchEventWithDetail,
    DocumentSetTitle,
    WebAuthnCreateJsonInto,
    WebAuthnCreateJsonIntoOr,
    WebAuthnGetJsonInto,
    WebAuthnGetJsonIntoOr,
    UrlSetSearchParam(UrlHistoryMode),
    UrlDeleteSearchParam(UrlHistoryMode),
}

pub(super) fn browser_effect_call_kind(path: &syn::Path) -> Option<BrowserEffectCallKind> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let [module, method] = segments.as_slice() else {
        let [crate_name, module, method] = segments.as_slice() else {
            return None;
        };
        if crate_name.as_str() != "rsjs" {
            return None;
        }
        return browser_effect_method(module.as_str(), method.as_str());
    };
    browser_effect_method(module.as_str(), method.as_str())
}

pub(super) fn browser_effect_method(module: &str, method: &str) -> Option<BrowserEffectCallKind> {
    match (module, method) {
        ("location", "set_hash") => Some(BrowserEffectCallKind::LocationSetHash),
        ("location", "assign") => Some(BrowserEffectCallKind::LocationAssign),
        ("location", "replace") => Some(BrowserEffectCallKind::LocationReplace),
        ("location", "reload") => Some(BrowserEffectCallKind::LocationReload),
        ("history", "push_state") => Some(BrowserEffectCallKind::HistoryPushState),
        ("history", "replace_state") => Some(BrowserEffectCallKind::HistoryReplaceState),
        ("history", "back") => Some(BrowserEffectCallKind::HistoryBack),
        ("history", "forward") => Some(BrowserEffectCallKind::HistoryForward),
        ("history", "go") => Some(BrowserEffectCallKind::HistoryGo),
        ("document", "dispatch_event") => Some(BrowserEffectCallKind::DocumentDispatchEvent),
        ("document", "dispatch_event_with_detail") => {
            Some(BrowserEffectCallKind::DocumentDispatchEventWithDetail)
        }
        ("document", "set_title") => Some(BrowserEffectCallKind::DocumentSetTitle),
        ("webauthn", "create_json_into") => Some(BrowserEffectCallKind::WebAuthnCreateJsonInto),
        ("webauthn", "create_json_into_or") => {
            Some(BrowserEffectCallKind::WebAuthnCreateJsonIntoOr)
        }
        ("webauthn", "get_json_into") => Some(BrowserEffectCallKind::WebAuthnGetJsonInto),
        ("webauthn", "get_json_into_or") => Some(BrowserEffectCallKind::WebAuthnGetJsonIntoOr),
        ("url", "set_search_param") => Some(BrowserEffectCallKind::UrlSetSearchParam(
            UrlHistoryMode::Replace,
        )),
        ("url", "push_search_param") => Some(BrowserEffectCallKind::UrlSetSearchParam(
            UrlHistoryMode::Push,
        )),
        ("url", "delete_search_param") => Some(BrowserEffectCallKind::UrlDeleteSearchParam(
            UrlHistoryMode::Replace,
        )),
        ("url", "push_delete_search_param") => Some(BrowserEffectCallKind::UrlDeleteSearchParam(
            UrlHistoryMode::Push,
        )),
        _ => None,
    }
}

pub(super) fn is_iterator_like_method(method: &str) -> bool {
    matches!(
        method,
        "iter"
            | "iter_mut"
            | "into_iter"
            | "map"
            | "filter"
            | "filter_map"
            | "flat_map"
            | "fold"
            | "reduce"
            | "for_each"
            | "find"
            | "any"
            | "all"
            | "collect"
            | "sum"
            | "count"
    )
}

pub(super) fn expr_is_inline_closure(expr: &Expr) -> bool {
    match expr {
        Expr::Closure(_) => true,
        Expr::Paren(paren) => expr_is_inline_closure(&paren.expr),
        Expr::Group(group) => expr_is_inline_closure(&group.expr),
        _ => false,
    }
}
