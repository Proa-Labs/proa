// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Declarative registry of `rsjs::<module>::<helper>` browser helpers.
//!
//! Each [`BrowserHelperSpec`] entry declares one authoring-surface
//! helper in a single place: its `rsjs` path, argument shapes, usage
//! string for diagnostics, the line it contributes to the
//! "unknown helper" allowlist, its client value type, gesture/async
//! constraints, how it lowers into Client IR, and the
//! `rsjs_conformance/browser_api_matrix.json` entry it backs.
//!
//! The registry drives:
//!
//! - recognition and argument checking in the `rsjs_macros` analyzer
//!   (`rsjs_macros::analyze::browser` routes registry paths here
//!   before its legacy hand-written tables),
//! - JS emission for [`HelperLowering::DeclaredRead`] helpers via
//!   their inline JS template (`crate::client::emit_js`),
//! - typing of declared reads (`crate::analysis::typing`),
//! - the SSR stub cross-check test in the `rsjs` crate, which asserts
//!   every entry has a matching hand-written stub and every stub in a
//!   migrated module has an entry.
//!
//! # Adding a new helper
//!
//! New browser helpers MUST be added here instead of extending the
//! legacy per-helper tables. For a simple read, one
//! [`HelperLowering::DeclaredRead`] entry plus a hand-written rustdoc
//! stub in `rsjs/src/lib.rs` is the entire compiler-side cost: the
//! analyzer, Client IR, descriptor JSON, JS emitter, typing pass, and
//! SSR fallback all handle it generically. Helpers that need bespoke
//! IR (multi-statement async protocols, signal writes) keep dedicated
//! Client IR variants; declare them here with a dedicated
//! [`HelperLowering`] tag so recognition, arity, diagnostics, and the
//! stub cross-check still come from the registry.
//!
//! # Migration status
//!
//! Migrated families: `window` (reads + scroll/print effects),
//! `local_storage` / `session_storage`, `clipboard`, `navigator`.
//! The remaining families (`location`, `history`, `document`,
//! `webauthn`, `url`, `encoding`) still live on the legacy
//! hand-written path in `rsjs_macros::analyze` and migrate
//! opportunistically.

use crate::client::{BrowserStorageArea, ClientType, StorageValueType, WindowField};
use crate::reactive::StorageArea;

/// Where a helper call is allowed to appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperContext {
    /// A value-producing read, usable in reactive expressions (text
    /// bindings, signal initializers) and handler payload
    /// expressions.
    ReactiveRead,
    /// A side effect, usable only as a handler statement.
    HandlerEffect,
}

/// Gesture / async constraints a helper carries at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperConstraint {
    /// No special activation requirement.
    None,
    /// The browser requires transient user activation (a user
    /// gesture). Enforced structurally: such helpers are
    /// [`HelperContext::HandlerEffect`], so they can only run inside
    /// DOM event handlers, and their emitted JS falls back when the
    /// browser rejects the call.
    UserGesture,
}

/// Shape of one argument slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// An arbitrary reactive expression lowered to [`crate::client::ClientExpr`].
    Reactive,
    /// A `&signal` reference resolved to a [`crate::types::SignalId`]
    /// (the `*_into` write-target convention).
    SignalRef,
}

/// One declared argument of a helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    /// Name used in documentation and diagnostics.
    pub name: &'static str,
    /// How the analyzer lowers the argument.
    pub kind: ArgKind,
}

const fn reactive(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Reactive,
    }
}

const fn signal_ref(name: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::SignalRef,
    }
}

/// How a registry helper is realised in Client IR.
///
/// Tags other than [`Self::DeclaredRead`] map onto dedicated Client
/// IR variants that predate the registry; they keep descriptor JSON
/// and emitted JS byte-identical for already-shipped helpers. New
/// simple reads use [`Self::DeclaredRead`] and need no IR, JSON, or
/// emitter additions at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperLowering {
    /// [`crate::client::ClientBrowserRead::Window`].
    WindowField(WindowField),
    /// [`crate::client::ClientBrowserRead::Storage`]; the optional
    /// fallback comes from the trailing `Reactive` argument when the
    /// entry declares one.
    StorageGet {
        area: BrowserStorageArea,
        value_type: StorageValueType,
    },
    /// [`crate::client::ClientBrowserEffect::WindowScrollTo`].
    WindowScrollTo,
    /// [`crate::client::ClientBrowserEffect::WindowScrollBy`].
    WindowScrollBy,
    /// [`crate::client::ClientBrowserEffect::WindowPrint`].
    WindowPrint,
    /// [`crate::client::ClientBrowserEffect::ClipboardWriteText`].
    ClipboardWriteText,
    /// [`crate::client::ClientBrowserEffect::ClipboardReadTextInto`];
    /// the optional fallback comes from the trailing `Reactive`
    /// argument when the entry declares one.
    ClipboardReadTextInto,
    /// [`crate::client::ClientBrowserEffect::StorageSetItem`].
    StorageSetItem(StorageArea),
    /// [`crate::client::ClientBrowserEffect::StorageRemoveItem`].
    StorageRemoveItem(StorageArea),
    /// A fully declarative read lowered to
    /// [`crate::client::ClientBrowserRead::Declared`]. `js` is the
    /// complete emitted expression; `$0`..`$9` placeholders splice
    /// the lowered arguments in order. The template must be a
    /// self-contained JS expression that survives a missing browser
    /// global (SSR-safe guards, `?? fallback`).
    DeclaredRead { js: &'static str },
    /// A fully declarative side effect lowered to
    /// [`crate::client::ClientBrowserEffect::Declared`]. The read-side
    /// mirror of [`Self::DeclaredRead`]: `js` is the complete emitted
    /// statement expression, with `$0`..`$9` placeholders splicing the
    /// lowered arguments in order. The template must be a
    /// self-contained JS expression that survives a missing browser
    /// global, because island modules can be evaluated in environments
    /// where the DOM is absent.
    DeclaredEffect { js: &'static str },
}

/// One declarative browser-helper entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserHelperSpec {
    /// Stable identifier, `<module>.<method>`. Used as the
    /// descriptor-JSON tag for declared reads, so it must never
    /// change once shipped.
    pub id: &'static str,
    /// First authoring path segment after the optional `rsjs::`
    /// prefix, e.g. `"clipboard"`.
    pub module: &'static str,
    /// Helper function name, e.g. `"write_text"`.
    pub method: &'static str,
    /// Where the call may appear.
    pub context: HelperContext,
    /// Argument slots in source order; arity is `args.len()`.
    pub args: &'static [ArgSpec],
    /// Exact call shape shown by arity / argument diagnostics,
    /// e.g. `"rsjs::window::scroll_to(x, y)"`.
    pub usage: &'static str,
    /// The line this helper contributes to the "unknown helper"
    /// allowlist diagnostic. Entries in one family may share a
    /// grouped line; the message builder deduplicates.
    pub suggestion: &'static str,
    /// Client value type a read produces ([`ClientType::Unit`] for
    /// effects).
    pub value_type: ClientType,
    /// Gesture / async constraint.
    pub constraint: HelperConstraint,
    /// How the analyzer and Client IR realise the call.
    pub lowering: HelperLowering,
    /// Entry id in `rsjs_conformance/browser_api_matrix.json`, when
    /// the helper backs a matrix entry. `None` for helpers the matrix
    /// does not (yet) track, such as window scrolling and metrics.
    pub matrix_id: Option<&'static str>,
}

const SUGGEST_WINDOW_FIELDS: &str = "rsjs::window::{inner_width,inner_height,scroll_x,scroll_y}()";
const SUGGEST_STORAGE_GET_LOCAL: &str =
    "rsjs::local_storage::{get_item,get_item_or,get_bool_or,get_i64_or,get_f64_or}(key, fallback)";
const SUGGEST_STORAGE_GET_SESSION: &str =
    "rsjs::session_storage::{get_item,get_item_or,get_bool_or,get_i64_or,get_f64_or}(key, fallback)";

const fn window_read(
    method: &'static str,
    id: &'static str,
    field: WindowField,
    value_type: ClientType,
) -> BrowserHelperSpec {
    BrowserHelperSpec {
        id,
        module: "window",
        method,
        context: HelperContext::ReactiveRead,
        args: &[],
        usage: "rsjs::window::<field>()",
        suggestion: SUGGEST_WINDOW_FIELDS,
        value_type,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::WindowField(field),
        matrix_id: None,
    }
}

#[allow(clippy::too_many_arguments)] // const table constructor: one positional slot per spec field
const fn storage_get(
    module: &'static str,
    method: &'static str,
    id: &'static str,
    suggestion: &'static str,
    area: BrowserStorageArea,
    value_type: StorageValueType,
    usage: &'static str,
    args: &'static [ArgSpec],
    client_type: ClientType,
) -> BrowserHelperSpec {
    BrowserHelperSpec {
        id,
        module,
        method,
        context: HelperContext::ReactiveRead,
        args,
        usage,
        suggestion,
        value_type: client_type,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::StorageGet { area, value_type },
        matrix_id: Some("storage.web_storage"),
    }
}

const STORAGE_GET_ARGS: &[ArgSpec] = &[reactive("key")];
const STORAGE_GET_OR_ARGS: &[ArgSpec] = &[reactive("key"), reactive("fallback")];

/// The declarative browser-helper registry.
///
/// Order within a family controls the order of grouped lines in the
/// "unknown helper" allowlist diagnostic.
pub static REGISTRY: &[BrowserHelperSpec] = &[
    // window: effects
    BrowserHelperSpec {
        id: "window.scroll_to",
        module: "window",
        method: "scroll_to",
        context: HelperContext::HandlerEffect,
        args: &[reactive("x"), reactive("y")],
        usage: "rsjs::window::scroll_to(x, y)",
        suggestion: "rsjs::window::scroll_to(x, y)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::WindowScrollTo,
        matrix_id: None,
    },
    BrowserHelperSpec {
        id: "window.scroll_by",
        module: "window",
        method: "scroll_by",
        context: HelperContext::HandlerEffect,
        args: &[reactive("x"), reactive("y")],
        usage: "rsjs::window::scroll_by(x, y)",
        suggestion: "rsjs::window::scroll_by(x, y)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::WindowScrollBy,
        matrix_id: None,
    },
    BrowserHelperSpec {
        id: "window.print",
        module: "window",
        method: "print",
        context: HelperContext::HandlerEffect,
        args: &[],
        usage: "rsjs::window::print()",
        suggestion: "rsjs::window::print()",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::UserGesture,
        lowering: HelperLowering::WindowPrint,
        matrix_id: None,
    },
    // window: reads
    window_read(
        "inner_width",
        "window.inner_width",
        WindowField::InnerWidth,
        ClientType::I32,
    ),
    window_read(
        "inner_height",
        "window.inner_height",
        WindowField::InnerHeight,
        ClientType::I32,
    ),
    window_read(
        "scroll_x",
        "window.scroll_x",
        WindowField::ScrollX,
        ClientType::F64,
    ),
    window_read(
        "scroll_y",
        "window.scroll_y",
        WindowField::ScrollY,
        ClientType::F64,
    ),
    // clipboard
    BrowserHelperSpec {
        id: "clipboard.write_text",
        module: "clipboard",
        method: "write_text",
        context: HelperContext::HandlerEffect,
        args: &[reactive("text")],
        usage: "rsjs::clipboard::write_text(text)",
        suggestion: "rsjs::clipboard::write_text(text)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::UserGesture,
        lowering: HelperLowering::ClipboardWriteText,
        matrix_id: Some("clipboard"),
    },
    BrowserHelperSpec {
        id: "clipboard.read_text_into",
        module: "clipboard",
        method: "read_text_into",
        context: HelperContext::HandlerEffect,
        args: &[signal_ref("target")],
        usage: "rsjs::clipboard::read_text_into(&signal)",
        suggestion: "rsjs::clipboard::read_text_into(&signal)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::UserGesture,
        lowering: HelperLowering::ClipboardReadTextInto,
        matrix_id: Some("clipboard"),
    },
    BrowserHelperSpec {
        id: "clipboard.read_text_into_or",
        module: "clipboard",
        method: "read_text_into_or",
        context: HelperContext::HandlerEffect,
        args: &[signal_ref("target"), reactive("fallback")],
        usage: "rsjs::clipboard::read_text_into_or(&signal, fallback)",
        suggestion: "rsjs::clipboard::read_text_into_or(&signal, fallback)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::UserGesture,
        lowering: HelperLowering::ClipboardReadTextInto,
        matrix_id: Some("clipboard"),
    },
    // local_storage
    storage_get(
        "local_storage",
        "get_item",
        "local_storage.get_item",
        SUGGEST_STORAGE_GET_LOCAL,
        BrowserStorageArea::Local,
        StorageValueType::String,
        "rsjs::<storage>::get_item(key)",
        STORAGE_GET_ARGS,
        ClientType::String,
    ),
    storage_get(
        "local_storage",
        "get_item_or",
        "local_storage.get_item_or",
        SUGGEST_STORAGE_GET_LOCAL,
        BrowserStorageArea::Local,
        StorageValueType::String,
        "rsjs::<storage>::get_item_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        ClientType::String,
    ),
    storage_get(
        "local_storage",
        "get_bool_or",
        "local_storage.get_bool_or",
        SUGGEST_STORAGE_GET_LOCAL,
        BrowserStorageArea::Local,
        StorageValueType::Bool,
        "rsjs::<storage>::get_<type>_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        ClientType::Bool,
    ),
    storage_get(
        "local_storage",
        "get_i64_or",
        "local_storage.get_i64_or",
        SUGGEST_STORAGE_GET_LOCAL,
        BrowserStorageArea::Local,
        StorageValueType::I64,
        "rsjs::<storage>::get_<type>_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        // `get_i64_or` preserves the full Rust `i64` domain as a JS BigInt.
        ClientType::I64,
    ),
    storage_get(
        "local_storage",
        "get_f64_or",
        "local_storage.get_f64_or",
        SUGGEST_STORAGE_GET_LOCAL,
        BrowserStorageArea::Local,
        StorageValueType::F64,
        "rsjs::<storage>::get_<type>_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        ClientType::F64,
    ),
    BrowserHelperSpec {
        id: "local_storage.set_item",
        module: "local_storage",
        method: "set_item",
        context: HelperContext::HandlerEffect,
        args: &[reactive("key"), reactive("value")],
        usage: "rsjs::*_storage::set_item(key, value)",
        suggestion: "rsjs::local_storage::set_item(key, value)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::StorageSetItem(StorageArea::Local),
        matrix_id: Some("storage.web_storage"),
    },
    BrowserHelperSpec {
        id: "local_storage.remove_item",
        module: "local_storage",
        method: "remove_item",
        context: HelperContext::HandlerEffect,
        args: &[reactive("key")],
        usage: "rsjs::*_storage::remove_item(key)",
        suggestion: "rsjs::local_storage::remove_item(key)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::StorageRemoveItem(StorageArea::Local),
        matrix_id: Some("storage.web_storage"),
    },
    // session_storage
    storage_get(
        "session_storage",
        "get_item",
        "session_storage.get_item",
        SUGGEST_STORAGE_GET_SESSION,
        BrowserStorageArea::Session,
        StorageValueType::String,
        "rsjs::<storage>::get_item(key)",
        STORAGE_GET_ARGS,
        ClientType::String,
    ),
    storage_get(
        "session_storage",
        "get_item_or",
        "session_storage.get_item_or",
        SUGGEST_STORAGE_GET_SESSION,
        BrowserStorageArea::Session,
        StorageValueType::String,
        "rsjs::<storage>::get_item_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        ClientType::String,
    ),
    storage_get(
        "session_storage",
        "get_bool_or",
        "session_storage.get_bool_or",
        SUGGEST_STORAGE_GET_SESSION,
        BrowserStorageArea::Session,
        StorageValueType::Bool,
        "rsjs::<storage>::get_<type>_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        ClientType::Bool,
    ),
    storage_get(
        "session_storage",
        "get_i64_or",
        "session_storage.get_i64_or",
        SUGGEST_STORAGE_GET_SESSION,
        BrowserStorageArea::Session,
        StorageValueType::I64,
        "rsjs::<storage>::get_<type>_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        // `get_i64_or` preserves the full Rust `i64` domain as a JS BigInt.
        ClientType::I64,
    ),
    storage_get(
        "session_storage",
        "get_f64_or",
        "session_storage.get_f64_or",
        SUGGEST_STORAGE_GET_SESSION,
        BrowserStorageArea::Session,
        StorageValueType::F64,
        "rsjs::<storage>::get_<type>_or(key, fallback)",
        STORAGE_GET_OR_ARGS,
        ClientType::F64,
    ),
    BrowserHelperSpec {
        id: "session_storage.set_item",
        module: "session_storage",
        method: "set_item",
        context: HelperContext::HandlerEffect,
        args: &[reactive("key"), reactive("value")],
        usage: "rsjs::*_storage::set_item(key, value)",
        suggestion: "rsjs::session_storage::set_item(key, value)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::StorageSetItem(StorageArea::Session),
        matrix_id: Some("storage.web_storage"),
    },
    BrowserHelperSpec {
        id: "session_storage.remove_item",
        module: "session_storage",
        method: "remove_item",
        context: HelperContext::HandlerEffect,
        args: &[reactive("key")],
        usage: "rsjs::*_storage::remove_item(key)",
        suggestion: "rsjs::session_storage::remove_item(key)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::StorageRemoveItem(StorageArea::Session),
        matrix_id: Some("storage.web_storage"),
    },
    // navigator
    BrowserHelperSpec {
        id: "navigator.language",
        module: "navigator",
        method: "language",
        context: HelperContext::ReactiveRead,
        args: &[],
        usage: "rsjs::navigator::language()",
        suggestion: "rsjs::navigator::language()",
        value_type: ClientType::String,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::DeclaredRead {
            js: "((typeof navigator !== \"undefined\") ? (navigator.language ?? \"\") : \"\")",
        },
        matrix_id: Some("platform.navigator_info"),
    },
    // root (document.documentElement)
    //
    // The root element carries page-wide presentation state that no
    // single island owns: the theme class and the `color-scheme` it
    // implies. A `node_ref` cannot reach it, because `ref=` only binds
    // elements the island itself renders and `<html>` belongs to the
    // document shell.
    BrowserHelperSpec {
        id: "root.has_class",
        module: "root",
        method: "has_class",
        context: HelperContext::ReactiveRead,
        args: &[reactive("class")],
        usage: "rsjs::root::has_class(class)",
        suggestion: "rsjs::root::has_class(class)",
        value_type: ClientType::Bool,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::DeclaredRead {
            js: "((typeof document !== \"undefined\" && document.documentElement) \
                 ? document.documentElement.classList.contains($0) : false)",
        },
        matrix_id: None,
    },
    BrowserHelperSpec {
        id: "root.set_class",
        module: "root",
        method: "set_class",
        context: HelperContext::HandlerEffect,
        args: &[reactive("class"), reactive("on")],
        usage: "rsjs::root::set_class(class, on)",
        suggestion: "rsjs::root::set_class(class, on)",
        value_type: ClientType::Unit,
        constraint: HelperConstraint::None,
        lowering: HelperLowering::DeclaredEffect {
            js: "((typeof document !== \"undefined\" && document.documentElement) \
                 ? document.documentElement.classList.toggle($0, !!($1)) : undefined)",
        },
        matrix_id: None,
    },
];

/// Look up a helper by authoring path.
pub fn find(module: &str, method: &str) -> Option<&'static BrowserHelperSpec> {
    REGISTRY
        .iter()
        .find(|spec| spec.module == module && spec.method == method)
}

/// Look up a value-producing read by authoring path.
pub fn find_read(module: &str, method: &str) -> Option<&'static BrowserHelperSpec> {
    find(module, method).filter(|spec| spec.context == HelperContext::ReactiveRead)
}

/// Look up a handler-statement effect by authoring path.
pub fn find_effect(module: &str, method: &str) -> Option<&'static BrowserHelperSpec> {
    find(module, method).filter(|spec| spec.context == HelperContext::HandlerEffect)
}

/// Look up a helper by stable id (`<module>.<method>`). Used by the
/// JS emitter and the typing pass to resolve
/// [`crate::client::ClientBrowserRead::Declared`] nodes round-tripped
/// through descriptor JSON.
pub fn find_by_id(id: &str) -> Option<&'static BrowserHelperSpec> {
    REGISTRY.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_paths_and_ids_are_unique() {
        for (i, a) in REGISTRY.iter().enumerate() {
            for b in REGISTRY.iter().skip(i + 1) {
                assert!(
                    a.module != b.module || a.method != b.method,
                    "duplicate path {}::{}",
                    a.module,
                    a.method
                );
                assert_ne!(a.id, b.id, "duplicate id {}", a.id);
            }
        }
    }

    #[test]
    fn ids_follow_module_dot_method() {
        for spec in REGISTRY {
            assert_eq!(
                spec.id,
                format!("{}.{}", spec.module, spec.method),
                "id must be <module>.<method> for {}",
                spec.id
            );
        }
    }

    #[test]
    fn reads_produce_values_and_effects_do_not() {
        for spec in REGISTRY {
            match spec.context {
                HelperContext::ReactiveRead => {
                    assert_ne!(
                        spec.value_type,
                        ClientType::Unit,
                        "read {} must declare a value type",
                        spec.id
                    );
                }
                HelperContext::HandlerEffect => {
                    assert_eq!(
                        spec.value_type,
                        ClientType::Unit,
                        "effect {} must not declare a value type",
                        spec.id
                    );
                }
            }
        }
    }

    #[test]
    fn signal_ref_args_only_in_effects() {
        for spec in REGISTRY {
            if spec.args.iter().any(|arg| arg.kind == ArgKind::SignalRef) {
                assert_eq!(
                    spec.context,
                    HelperContext::HandlerEffect,
                    "signal-ref args require handler context: {}",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn declared_helpers_have_reactive_args_and_in_range_placeholders() {
        for spec in REGISTRY {
            let js = match spec.lowering {
                HelperLowering::DeclaredRead { js } | HelperLowering::DeclaredEffect { js } => js,
                _ => continue,
            };
            assert!(
                spec.args.iter().all(|arg| arg.kind == ArgKind::Reactive),
                "declared helper {} must take only reactive args",
                spec.id
            );
            let mut chars = js.chars().peekable();
            while let Some(c) = chars.next() {
                if c != '$' {
                    continue;
                }
                let Some(digit) = chars.peek().and_then(|d| d.to_digit(10)) else {
                    panic!(
                        "declared helper {} has a bare `$` in its JS template",
                        spec.id
                    );
                };
                assert!(
                    (digit as usize) < spec.args.len(),
                    "declared helper {} references missing arg ${digit}",
                    spec.id
                );
                chars.next();
            }
        }
    }

    #[test]
    fn gesture_constrained_helpers_are_handler_effects() {
        for spec in REGISTRY {
            if spec.constraint == HelperConstraint::UserGesture {
                assert_eq!(
                    spec.context,
                    HelperContext::HandlerEffect,
                    "user-gesture helper {} must be handler-only",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn lookups_resolve() {
        assert!(find_read("window", "inner_width").is_some());
        assert!(find_read("navigator", "language").is_some());
        assert!(find_effect("clipboard", "write_text").is_some());
        assert!(find_effect("window", "inner_width").is_none());
        assert!(find_read("clipboard", "write_text").is_none());
        assert!(find_by_id("navigator.language").is_some());
        assert!(find_by_id("navigator.does_not_exist").is_none());
    }

    #[test]
    fn window_read_types_match_web_idl_and_public_stubs() {
        assert_eq!(
            find_read("window", "inner_width").map(|spec| &spec.value_type),
            Some(&ClientType::I32)
        );
        assert_eq!(
            find_read("window", "inner_height").map(|spec| &spec.value_type),
            Some(&ClientType::I32)
        );
        assert_eq!(
            find_read("window", "scroll_x").map(|spec| &spec.value_type),
            Some(&ClientType::F64)
        );
        assert_eq!(
            find_read("window", "scroll_y").map(|spec| &spec.value_type),
            Some(&ClientType::F64)
        );
    }
}
