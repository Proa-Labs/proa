// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! # rsjs
//!
//! Compile interactive Proa components written in Rust into server-rendered
//! HTML and route-planned JavaScript.
//!
//! `rsjs` analyzes an explicit `#[rsjs(component)]`-annotated
//! `impl WebRenderSync<L>` or `impl WebRender<L>`. The implementation remains a
//! normal Proa server component. When reachable browser work exists, linked
//! compilation also emits the client graph that makes the rendered HTML
//! interactive. An analyzed implementation with no reachable client roots
//! remains server-only and emits no island JavaScript.
//!
//! Use `#[rsjs(component)]` for linked component participation. Add `client`
//! when the component originates signals, handlers, queries, mutations,
//! shared-state acquisition, node refs, or browser capabilities. The body is a
//! normal `html_sync!` or `html!` template; there is no `rsjs!` body macro.
//!
//! The ordinary trait forms omit the writer and use the reusable
//! `RenderBuffer` owned by `WebContext<L>`. Low-level integrations may specify
//! the advanced `WebRenderSync<L, B>` / `WebRender<L, B>` and
//! `WebContext<L, B>` forms when they genuinely need a custom writer.
//!
//! # Build model
//!
//! Deployable applications use the linked `cargo proa` lifecycle:
//!
//! 1. Descriptor compilation preserves typed SSR and emits component/helper
//!    edges plus latent client crossings.
//! 2. Semantic linking resolves Rust identities, reachability, final owners,
//!    and activation strategies.
//! 3. Final compilation emits wrappers, fragments, serializers, client
//!    modules, and the exact manifest for the linked plan.
//! 4. The framework response planner includes only artifacts recorded by the
//!    rendered response and orders runtime assets and seed tables.
//!
//! Plain `cargo check` and rust-analyzer remain useful for ordinary Rust typing
//! and provisional descriptors. Use `cargo proa check` for full linked
//! diagnostics and `cargo proa build` for a deployable artifact. Manifest
//! entries expose raw JavaScript sources; production minification and
//! compression are framework-adapter responsibilities.
//!
//! `rsjs` depends on `proa_core` for `WebRenderSync`, `WebRender`,
//! `WebContext`, and `WriteBuf`; it is intentionally part of Proa rather than a
//! framework-agnostic client compiler.
//!
//! # Authoring an island
//!
//! ```rust,ignore
//! use rsjs::*;
//! use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
//! use proa_macros::html_sync;
//!
//! pub struct Counter {
//!     pub initial: i32,
//! }
//!
//! #[rsjs(component, client)]
//! impl<L: DataLoader> WebRenderSync<L> for Counter {
//!     fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
//!         let count = signal::<i32>(self.initial);
//!         let decrement = event_handler(
//!             |_: MouseEvent<elem::Button>| count.update(|value| value - 1),
//!         );
//!         let increment = event_handler(
//!             |_: MouseEvent<elem::Button>| count.update(|value| value + 1),
//!         );
//!
//!         html_sync! {
//!             <section data-counter>
//!                 <button type="button" on_click={decrement()}>"-"</button>
//!                 <span>{count.get()}</span>
//!                 <button type="button" on_click={increment()}>"+"</button>
//!             </section>
//!         }
//!         .render(cx)
//!     }
//! }
//! ```
//!
//! Notable points:
//!
//! - The user writes the struct and the trait impl explicitly.
//!   `#[rsjs(component)]` attribute attaches to the impl block; the
//!   macro rewrites the render method body but doesn't generate
//!   the trait signature for you. Add `client` only when the component
//!   originates writable state, handlers, queries, mutations, shared-state
//!   acquisition, node references, timers, effects, or browser capabilities.
//! - Sync vs async is the user's choice: write
//!   `impl WebRenderSync<L>` for sync islands, or
//!   the real `impl WebRender<L>` method returning
//!   `RenderOutcome<impl Future<...>>` for components that need to await.
//!   `async fn render` sugar and `#[rsjs(async)]` are not supported—the
//!   authored trait signature remains ordinary Rust.
//!   These forms use the `RenderBuffer` owned by `WebContext<L>`; the second
//!   `B` parameter is available only when an integration needs a custom writer.
//! - The body macro is `html_sync!` (or `html!` for async). There
//!   is no separate `rsjs!` body macro.
//! - Component composition uses proa's struct-construction
//!   pattern (`Card { children: Some(...), ..Default::default() }`),
//!   not JSX-style `<Card>...</Card>`. Plain HTML elements
//!   (`<button>`, `<span>`) keep their tag syntax inside
//!   `html_sync!`.
//! - `signal()`, `on_click={...}`, and other reactive primitives
//!   are recognised by the rsjs analyser by name. Event handlers
//!   and input bindings use snake_case identifiers (`on_click`,
//!   `on_input`, `on_change`, `on_paste`, `on_document_keydown`,
//!   `on_submit`, `bind_value`, `bind_checked`, `bind_selected`);
//!   they parse as ordinary attributes and the analyser recognises
//!   the closed `on_*` / `bind_*` prefix list. Outside the render
//!   method of an origin-capable `#[rsjs(component, client)]` impl block,
//!   these are compile
//!   errors (best-effort).
//! - `cx` is in scope inside the render method as it is in any Proa component;
//!   async code can load values with a real `LoadKey`, such as
//!   `cx.load::<User>(LoadKey::typed("user", id)).await`.
//!
//! # Status
//!
//! Pre-alpha. The compiler pipeline is being built up incrementally. See
//! `rsjs/README.md` for supported authoring and `rsjs/STATUS.md` for current
//! implementation coverage.

// Re-export the proc-macro entry points so users only depend on `rsjs`.
pub use rsjs_macros::rsjs;

/// Generated, zero-cost browser type facade used by RSJS authoring handles.
///
/// These are Rust compiler witnesses for JavaScript lowering, not WASM DOM
/// bindings. Application crates can use `rsjs::web` without adding a second
/// direct dependency.
pub mod web {
    pub use rsjs_web::*;
}

/// Embedded browser runtime sources for framework and asset-pipeline adapters.
pub mod runtime_assets;

/// `#[rsjs::client]` — mark a free function as a reusable client-side
/// helper, or define a closed client value-object module.
///
/// The annotated function's body must conform to the v0.1 constrained
/// `Expr` IR (literals, parameter reads, binary operators,
/// `.is_non_empty()`, calls to other `#[rsjs::client]` helpers). The
/// macro replaces the original function with a metadata struct
/// `__rsjs_client_<name>_meta` (carrying `NAME`, `PARAMS`,
/// `RETURN_TY`, `BODY_HIR_JSON`, and v3 `BODY_IR_JSON`) plus a re-emitted
/// Rust function that delegates to [`__runtime::run_client_helper`].
/// Client-module metadata and bundles are emitted from the v3 body; server
/// render uses the generated metadata evaluator for the supported primitive
/// helper body subset.
///
/// See `docs/agent/rsjs-islands.md` for supported helper authoring and
/// `rsjs/STATUS.md` for current helper coverage.
///
/// # Positive example
///
/// ```
/// #[rsjs::client]
/// pub fn subtotal(price: i64, qty: i64) -> i64 {
///     price * qty
/// }
/// ```
///
/// A value-object module keeps instances local to its public scalar helper
/// roots. Constructors and borrowed methods with typed value arguments lower
/// to plain record operations rather than a JavaScript prototype:
///
/// ```
/// #[rsjs::client]
/// pub mod menu_logic {
///     pub struct Menu { open: bool }
///
///     impl Menu {
///         pub fn new(open: bool) -> Self { Self { open } }
///         pub fn set_open(&mut self, enabled: bool) {
///             if enabled { self.open = true; } else { self.open = false; }
///         }
///         pub fn is_open(&self) -> bool { self.open }
///     }
///
///     pub fn toggled() -> bool {
///         let mut menu = Menu::new(false);
///         menu.set_open(true);
///         menu.is_open()
///     }
/// }
///
/// assert!(menu_logic::toggled());
/// ```
///
/// # Negative cases (each rejected with a span-pointed compile error)
///
/// Closure body:
///
/// ```compile_fail
/// #[rsjs::client]
/// pub fn boom(x: i64) -> i64 {
///     |y: i64| y + x
/// }
/// ```
///
/// `async` body:
///
/// ```compile_fail
/// #[rsjs::client]
/// pub async fn boom(x: i64) -> i64 { x }
/// ```
///
/// `cx` reference:
///
/// ```compile_fail
/// #[rsjs::client]
/// pub fn boom(x: i64) -> i64 { cx + x }
/// ```
///
/// Unrecognised method call:
///
/// ```compile_fail
/// #[rsjs::client]
/// pub fn boom(x: i64) -> i64 { x.frobnicate() }
/// ```
pub use rsjs_macros::client;

/// `#[rsjs::client_module]` — group `#[rsjs::client]` helpers into a
/// single client-loadable JS bundle.
///
/// Applied to `pub mod <name> { ... }`. The macro collects every
/// `pub fn` inside the module (each must be `#[rsjs::client]`-marked)
/// and emits a sibling
/// `__rsjs_client_module_<name>_meta: &'static rsjs::ClientModuleMeta`
/// for optional bundle metadata. A consuming component simply calls an
/// annotated helper through its ordinary Rust path; reachable calls are linked
/// automatically.
///
/// # Positive example
///
/// ```
/// #[rsjs::client_module]
/// pub mod pricing {
///     #[rsjs::client]
///     pub fn subtotal(price: i64, qty: i64) -> i64 {
///         price * qty
///     }
/// }
/// ```
///
/// # Negative cases (each rejected with a span-pointed compile error)
///
/// Extern (non-inline) module:
///
/// ```compile_fail
/// #[rsjs::client_module]
/// pub mod boom;
/// ```
///
/// Empty module:
///
/// ```compile_fail
/// #[rsjs::client_module]
/// pub mod boom { }
/// ```
///
/// Private (non-`pub`) module:
///
/// ```compile_fail
/// #[rsjs::client_module]
/// mod boom {
///     #[rsjs::client]
///     pub fn x(a: i64) -> i64 { a }
/// }
/// ```
pub use rsjs_macros::client_module;

/// `#[derive(rsjs::ClientProps)]` — generate a `ClientEncode` impl,
/// a `ClientDecode` impl when the fields are owned/decodeable, and a sibling
/// `__rsjs_codec_meta_<TypeIdent>` const for a user struct or fieldless enum.
///
/// See [`ClientEncode`] and [`ClientDecode`] for the closed-vocabulary type
/// table and JSON wire format.
///
/// # Positive example
///
/// ```
/// #[derive(rsjs::ClientProps)]
/// pub struct UserDraft {
///     pub username: String,
///     pub age: u32,
///     pub admin: bool,
/// }
/// ```
///
/// Borrowed/static UI payloads that only travel from Rust to JavaScript use
/// the same derive. The macro still emits `ClientEncode` and metadata, but
/// intentionally skips `ClientDecode` because browser JSON cannot soundly
/// recreate Rust borrows such as `&'static str`.
///
/// ```
/// #[derive(rsjs::ClientProps)]
/// pub struct StaticOption {
///     pub value: &'static str,
///     pub label: &'static str,
/// }
/// ```
///
/// Fieldless enums use their exact Rust variant identifiers as stable JSON
/// string tags. Explicit numeric discriminants do not cross the boundary.
///
/// ```
/// use rsjs::{ClientDecode, ClientEncode};
///
/// #[derive(Debug, PartialEq, rsjs::ClientProps)]
/// #[repr(u8)]
/// pub enum Fulfillment {
///     Pickup = 1,
///     Delivery = 2,
/// }
///
/// assert_eq!(Fulfillment::Pickup.encode(), "\"Pickup\"");
/// assert_eq!(Fulfillment::decode("\"Delivery\"").unwrap(), Fulfillment::Delivery);
/// ```
///
/// # Negative cases (each rejected with a span-pointed compile error)
///
/// Tuple struct:
///
/// ```compile_fail
/// #[derive(rsjs::ClientProps)]
/// pub struct Wrap(pub u32);
/// ```
///
/// Data-carrying enum:
///
/// ```compile_fail
/// #[derive(rsjs::ClientProps)]
/// pub enum Pick {
///     A,
///     Custom(String),
/// }
/// ```
///
/// Union:
///
/// ```compile_fail
/// #[derive(rsjs::ClientProps)]
/// pub union Bits { a: u32, b: f32 }
/// ```
///
/// Unsupported field type (a generic path other than
/// `Option<T>` / `Vec<T>` / `String`):
///
/// ```compile_fail
/// use std::collections::HashMap;
/// #[derive(rsjs::ClientProps)]
/// pub struct Bag {
///     pub map: HashMap<String, String>,
/// }
/// ```
///
/// Nested `Option<Option<T>>`:
///
/// ```compile_fail
/// #[derive(rsjs::ClientProps)]
/// pub struct DoubleOpt {
///     pub maybe: Option<Option<i32>>,
/// }
/// ```
pub use rsjs_macros::ClientProps;

/// Derive a typed page-scoped shared-state schema and its generated handle.
///
/// Every schema requires an explicit, namespaced protocol identity:
///
/// ```
/// #[derive(rsjs::SharedState)]
/// #[rsjs(type_key = "acme.workbench")]
/// pub struct WorkbenchState {
///     pub select_mode: bool,
///     pub button_bg: String,
/// }
/// ```
pub use rsjs_macros::SharedState;

mod component_abi;
mod helper_abi;
mod linked_render;
mod shared_state_abi;

#[doc(hidden)]
pub mod __private {
    pub use crate::__rsjs_emit_client_helper_edge;
    pub use crate::__rsjs_emit_component_edge;
    pub use crate::__rsjs_emit_component_field;
    pub use crate::component_abi::{
        ComponentAbi, ComponentAbiWitness, ComponentOriginPermission, ComponentRenderKind,
        ComponentSiteRender, ComponentSiteRenderSync, LinkedCallbackDispatch, LinkedChildSite,
        LinkedComponent, LinkedLiveCallback, LinkedLiveProp, LinkedOwnerActivationStrategy,
        LinkedRenderSitePlan, LinkedSharedStateRequirement, LinkedTemporalPropMode,
        COMPONENT_ABI_FORMAT_MAGIC, COMPONENT_ABI_VERSION, COMPONENT_DESCRIPTOR_SCHEMA_VERSION,
        COMPONENT_SOURCE_KEY_VERSION,
    };
    pub use crate::linked_render::{
        linked_owner_value, LinkedMarkerAttr, LinkedRenderBoundary,
        LinkedSharedStateProviderChildren, LinkedTextBinding,
    };
    pub use crate::shared_state_abi::{
        SharedStateAbi, SharedStateAbiSchemaStatus, SharedStateAbiWitness,
        SHARED_STATE_ABI_WITNESS_MAGIC, SHARED_STATE_ABI_WITNESS_VERSION,
    };
    pub use proa_core::RsjsLinkedProviderOccurrenceKey;
    pub use rsjs_transport::{
        encode_semantic_identity, ComponentDefinitionRecord, ComponentEdgeRecord,
        ComponentFieldRecord, HelperDefinitionRecord, HelperEdgeRecord, PayloadRecord,
        SemanticIdentityKind, SemanticIdentityRecord, SharedStateAcquisitionRecord,
        SharedStateDefinitionRecord, UNASSIGNED_CARGO_UNIT_ID,
    };
    pub use serde_json;

    /// Rust-type-check one compiler-discovered inbound value field.
    ///
    /// The generated validation method projects the authored field before
    /// calling this function. The unconstrained generic is intentional: live
    /// values fused into one owner do not cross a serialization boundary, so
    /// they must not acquire an artificial `ClientEncode` requirement.
    #[doc(hidden)]
    pub fn assert_component_value_field<T: ?Sized>(_field: &T) {}

    /// Rust-type-check one required inbound node-ref handle field.
    ///
    /// Accepting `&NodeRef<T>` (rather than an unconstrained generic) rejects
    /// optional/container-wrapped handles and ordinary values before their
    /// descriptor can participate in linked composition.
    #[doc(hidden)]
    pub fn assert_component_node_ref_field<T>(_field: &crate::NodeRef<T>) {}

    /// Generated assignability relation shared by node refs and typed event
    /// current targets.
    #[doc(hidden)]
    pub use rsjs_web::ElementTargetFor as ComponentNodeRefTarget;

    #[doc(hidden)]
    pub fn assert_component_node_ref_target<Field, Target>(_field: &crate::NodeRef<Field>)
    where
        Target: crate::elem::ElementMarker,
        Field: ComponentNodeRefTarget<Target>,
    {
    }

    /// Generated event-interface vocabulary and compatibility traits used by
    /// compiler-emitted Rust witnesses.
    #[doc(hidden)]
    pub use rsjs_web::{event_interface, EventCurrentTargetFor, EventFor};

    /// Implemented when an authored event handle is safe for a DOM interface.
    ///
    /// `rsjs::Event<E>` is the base-interface escape hatch and can receive any
    /// supported event. More specific handles only implement their matching
    /// browser interface.
    impl<E, Interface> EventFor<Interface> for crate::Event<E>
    where
        E: crate::web::dom::DomTargetMarker,
        Interface: event_interface::Interface,
    {
    }

    macro_rules! impl_event_for {
        ($event:ty => $interface:ty) => {
            impl<E> EventFor<$interface> for $event where E: crate::web::dom::DomTargetMarker {}
        };
    }

    impl_event_for!(crate::MouseEvent<E> => event_interface::MouseEvent);
    impl_event_for!(crate::InputEvent<E> => event_interface::InputEvent);
    impl_event_for!(crate::ChangeEvent<E> => event_interface::ChangeEvent);
    impl_event_for!(crate::ClipboardEvent<E> => event_interface::ClipboardEvent);
    impl_event_for!(crate::KeyboardEvent<E> => event_interface::KeyboardEvent);
    impl_event_for!(crate::WheelEvent<E> => event_interface::WheelEvent);
    impl_event_for!(crate::CompositionEvent<E> => event_interface::CompositionEvent);
    impl_event_for!(crate::DragEvent<E> => event_interface::DragEvent);
    impl_event_for!(crate::ToggleEvent<E> => event_interface::ToggleEvent);
    impl_event_for!(crate::AnimationEvent<E> => event_interface::AnimationEvent);
    impl_event_for!(crate::TransitionEvent<E> => event_interface::TransitionEvent);
    impl_event_for!(crate::PointerEvent<E> => event_interface::PointerEvent);
    impl_event_for!(crate::TouchEvent<E> => event_interface::TouchEvent);
    impl_event_for!(crate::PopStateEvent<E> => event_interface::PopStateEvent);
    impl_event_for!(crate::HashChangeEvent<E> => event_interface::HashChangeEvent);
    impl_event_for!(crate::StorageEvent<E> => event_interface::StorageEvent);
    impl_event_for!(crate::SubmitEvent<E> => event_interface::SubmitEvent);

    /// Rustc witness for a local DOM event binding.
    #[doc(hidden)]
    pub fn assert_event_binding<Target, Interface, E>(_binding: &crate::EventBinding<E>)
    where
        Target: crate::web::dom::DomTargetMarker,
        Interface: event_interface::Interface,
        E: EventFor<Interface>,
        <E as crate::EventParamStub>::CurrentTarget: EventCurrentTargetFor<Target>,
    {
    }

    /// Rustc witness for an inline local DOM event closure.
    #[doc(hidden)]
    pub fn assert_event_type<Target, Interface, E>()
    where
        Target: crate::web::dom::DomTargetMarker,
        Interface: event_interface::Interface,
        E: EventFor<Interface>,
        <E as crate::EventParamStub>::CurrentTarget: EventCurrentTargetFor<Target>,
    {
    }

    /// Recreate only the event type of a named handler in an IDE/source-
    /// checking closure, without replaying its browser-only body or forcing
    /// its separately lowered template payload to have an owned Rust type.
    #[doc(hidden)]
    pub fn event_handler_type<E>() -> crate::EventBinding<E>
    where
        E: crate::EventParamStub,
    {
        crate::EventBinding(core::marker::PhantomData)
    }

    /// Rust-type-check a required compiler-owned callback field.
    #[doc(hidden)]
    pub fn assert_required_component_callback<T>(_field: &crate::Handler<T>) {}

    /// Rust-type-check an optional compiler-owned callback field.
    #[doc(hidden)]
    pub fn assert_optional_component_callback<T>(_field: &Option<crate::Handler<T>>) {}

    /// Serialize a server-side query seed into JSON that is safe to place in a
    /// `<script type="application/json">` island-state blob.
    pub fn to_script_json<T>(value: &T) -> Result<String, serde_json::Error>
    where
        T: serde::Serialize + ?Sized,
    {
        let json = serde_json::to_string(value)?;
        Ok(escape_script_json(json))
    }

    /// Enforce the HTML script-data boundary after arbitrary `ClientEncode`
    /// implementations have appended otherwise-valid JSON.
    #[doc(hidden)]
    pub fn make_script_json_safe(json: &mut String) {
        if !json
            .chars()
            .any(|ch| matches!(ch, '<' | '>' | '&' | '\u{2028}' | '\u{2029}'))
        {
            return;
        }
        *json = escape_script_json(core::mem::take(json));
    }

    pub fn json_project(value: &serde_json::Value, pointer: &str) -> serde_json::Value {
        value
            .pointer(pointer)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    pub fn json_as_str(value: &serde_json::Value) -> String {
        value.as_str().unwrap_or("").to_owned()
    }

    pub fn json_as_f64(value: &serde_json::Value) -> f64 {
        value.as_f64().unwrap_or(0.0)
    }

    pub fn json_as_i64(value: &serde_json::Value) -> i64 {
        const JS_MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;
        const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

        if let Some(value) = value.as_i64() {
            return if (JS_MIN_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&value) {
                value
            } else {
                0
            };
        }
        if let Some(value) = value.as_f64() {
            return if value.is_finite()
                && value.fract() == 0.0
                && value >= JS_MIN_SAFE_INTEGER as f64
                && value <= JS_MAX_SAFE_INTEGER as f64
            {
                value as i64
            } else {
                0
            };
        }
        value
            .as_str()
            .and_then(parse_canonical_i64_decimal)
            .unwrap_or(0)
    }

    fn parse_canonical_i64_decimal(value: &str) -> Option<i64> {
        if value == "0" {
            return Some(0);
        }
        let digits = value.strip_prefix('-').unwrap_or(value);
        let first = *digits.as_bytes().first()?;
        if !(b'1'..=b'9').contains(&first) || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        value.parse().ok()
    }

    pub fn json_as_bool(value: &serde_json::Value) -> bool {
        value.as_bool().unwrap_or(false)
    }

    pub fn json_as_array(value: &serde_json::Value) -> String {
        if value.is_array() {
            value.to_string()
        } else {
            "[]".to_owned()
        }
    }

    pub fn json_as_object(value: &serde_json::Value) -> String {
        if value.is_object() {
            value.to_string()
        } else {
            "{}".to_owned()
        }
    }

    pub fn json_is_present(value: &serde_json::Value) -> bool {
        !value.is_null()
    }

    pub fn json_eq_value(lhs: &serde_json::Value, rhs: &serde_json::Value) -> bool {
        lhs == rhs
    }

    pub fn json_coalesce(
        value: &serde_json::Value,
        fallback: serde_json::Value,
    ) -> serde_json::Value {
        if value.is_null() {
            fallback
        } else {
            value.clone()
        }
    }

    pub fn json_text(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => String::new(),
            serde_json::Value::String(value) => value.clone(),
            serde_json::Value::Bool(_) | serde_json::Value::Number(_) => value.to_string(),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
        }
    }

    fn escape_script_json(json: String) -> String {
        let mut out = String::with_capacity(json.len());
        for ch in json.chars() {
            match ch {
                '<' => out.push_str("\\u003C"),
                '>' => out.push_str("\\u003E"),
                '&' => out.push_str("\\u0026"),
                '\u{2028}' => out.push_str("\\u2028"),
                '\u{2029}' => out.push_str("\\u2029"),
                _ => out.push(ch),
            }
        }
        out
    }
}

#[cfg(test)]
mod private_tests {
    #[test]
    fn query_seed_json_is_script_safe() {
        let value = "</script><div>&\u{2028}";
        let json = super::__private::to_script_json(&value).unwrap();
        assert!(!json.contains('<'));
        assert!(!json.contains('>'));
        assert!(!json.contains('&'));
        assert!(!json.contains('\u{2028}'));
        assert_eq!(serde_json::from_str::<String>(&json).unwrap(), value);
    }

    #[test]
    fn arbitrary_client_json_is_made_script_safe_at_the_boundary() {
        let mut json = "{\"bio\":\"</script><div>&\u{2028}\"}".to_owned();
        super::__private::make_script_json_safe(&mut json);
        assert!(!json.contains('<'));
        assert!(!json.contains('>'));
        assert!(!json.contains('&'));
        assert!(!json.contains('\u{2028}'));
        let decoded: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded["bio"], "</script><div>&\u{2028}");
    }

    #[test]
    fn query_projection_helpers_match_browser_fallbacks() {
        let value = serde_json::json!({
            "price": { "value": 172.5, "currency": "USD" },
            "active": true
        });
        let price = super::__private::json_project(&value, "/price/value");
        let missing = super::__private::json_project(&value, "/price/missing");

        assert_eq!(super::__private::json_as_f64(&price), 172.5);
        assert_eq!(
            super::__private::json_as_str(&super::__private::json_project(
                &value,
                "/price/currency"
            )),
            "USD"
        );
        assert!(super::__private::json_as_bool(
            &super::__private::json_project(&value, "/active")
        ));
        assert!(!super::__private::json_is_present(&missing));
        assert_eq!(
            super::__private::json_text(&super::__private::json_coalesce(
                &missing,
                serde_json::Value::String("fallback".to_owned())
            )),
            "fallback"
        );
    }
}

/// Mark a child expression inside an analyzed RSJS component as server-only.
///
/// This is intentionally an identity function at runtime. Its purpose
/// is compile-time signalling to the `#[rsjs(component...)]` analyser:
/// `rsjs::server_only(child)` is preserved in the generated
/// `html_sync!` body as ordinary Proa server-rendered content, but no
/// client-side signal/query dependency or hydration binding is created
/// for it.
///
/// Use this for trusted static SVG sprites, `UnsafeRaw(...)`, or
/// regular Proa components that should render once on the server and
/// remain opaque to rsjs.
#[inline(always)]
pub fn server_only<T>(child: T) -> T {
    child
}

/// Request an explicit island boundary for a child component.
///
/// Inside a parent `#[rsjs(component...)]`, descriptor-backed components compose by
/// default: their client IR is inlined into the parent so callback props can
/// update parent-owned state. Wrap a child in `rsjs::island(...)` when it should
/// stay independent and render as its own nested island boundary.
///
/// This is an identity function on the server. The `#[rsjs(component...)]` macro recognizes
/// the call and keeps the child render outside the parent client IR. Parent
/// `rsjs::handler(...)` values cannot cross this boundary.
#[inline(always)]
pub fn island<T>(child: T) -> T {
    child
}

/// Private runtime hooks consumed by code generated by `rsjs_macros`.
///
/// **Not part of the public API.** Items in this module are emitted
/// into user crates by the proc macros (`#[rsjs::client]`,
/// `#[rsjs(component...)]`, etc.) and need to resolve through the user's normal
/// `rsjs` dependency. Their signatures, names, and behaviour are
/// internal contracts between the macros and the runtime; expect
/// breakage on any rsjs version bump if you reach into this module
/// directly.
///
/// The generated helper contract is implemented by
/// `rsjs_macros::client_attr`; supported authoring is documented in
/// `docs/agent/rsjs-islands.md`.
#[doc(hidden)]
pub mod __runtime {
    /// Runtime value shape used by generated `#[rsjs::client]`
    /// server-side evaluators.
    #[derive(Debug, Clone, PartialEq)]
    pub enum ClientValue {
        Unit,
        Null,
        Bool(bool),
        I64(i64),
        U64(u64),
        F64(f64),
        String(String),
        Array(Vec<ClientValue>),
        /// An ephemeral object used while evaluating a client helper on the
        /// server. The raw value deliberately has no `ClientArgEncode` or
        /// `ClientReturnValue` implementation. Generated linked receiver
        /// helpers use the opaque carrier below, while public helper boundaries
        /// still cannot transport objects.
        Object(::std::rc::Rc<::std::cell::RefCell<Vec<(String, ClientValue)>>>),
    }

    /// Opaque Rust-side carrier for a helper-internal client value object.
    ///
    /// Generated receiver helpers use this type in their private Rust shims.
    /// It deliberately has no public constructor and is never accepted by a
    /// user-authored standalone-helper signature. Cloning the carrier shares
    /// the same object, matching JavaScript reference semantics across linked
    /// method calls.
    #[derive(Debug, Clone, PartialEq)]
    pub struct ClientObjectValue(ClientValue);

    /// Convert a Rust argument into the closed primitive value set
    /// supported by `#[rsjs::client]` helper bodies.
    pub trait ClientArgEncode {
        fn encode_client_arg(&self) -> ClientValue;
    }

    /// Argument passed to a generated `#[rsjs::client]` server-side
    /// shim.
    ///
    /// The associated [`Self::from_value`] constructor is invoked from
    /// macro-generated code.
    #[derive(Debug, Clone, PartialEq)]
    pub struct ClientArg {
        pub value: ClientValue,
    }

    impl ClientArg {
        /// Construct a `ClientArg` from any supported primitive
        /// reference.
        pub fn from_value<T: ClientArgEncode + ?Sized>(value: &T) -> Self {
            Self {
                value: value.encode_client_arg(),
            }
        }
    }

    macro_rules! impl_signed_arg {
        ($($ty:ty),* $(,)?) => {
            $(
                impl ClientArgEncode for $ty {
                    fn encode_client_arg(&self) -> ClientValue {
                        ClientValue::I64(*self as i64)
                    }
                }
            )*
        };
    }

    macro_rules! impl_unsigned_arg {
        ($($ty:ty),* $(,)?) => {
            $(
                impl ClientArgEncode for $ty {
                    fn encode_client_arg(&self) -> ClientValue {
                        ClientValue::U64(*self as u64)
                    }
                }
            )*
        };
    }

    impl_signed_arg!(i8, i16, i32, i64);
    impl_unsigned_arg!(u8, u16, u32, u64);

    impl ClientArgEncode for bool {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::Bool(*self)
        }
    }

    impl ClientArgEncode for f32 {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::F64(*self as f64)
        }
    }

    impl ClientArgEncode for f64 {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::F64(*self)
        }
    }

    impl ClientArgEncode for String {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::String(self.clone())
        }
    }

    impl ClientArgEncode for str {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::String(self.to_string())
        }
    }

    impl ClientArgEncode for &str {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::String((*self).to_string())
        }
    }

    impl ClientArgEncode for () {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::Unit
        }
    }

    impl<T: ClientArgEncode> ClientArgEncode for [T] {
        fn encode_client_arg(&self) -> ClientValue {
            ClientValue::Array(
                self.iter()
                    .map(ClientArgEncode::encode_client_arg)
                    .collect(),
            )
        }
    }

    impl<T: ClientArgEncode> ClientArgEncode for Vec<T> {
        fn encode_client_arg(&self) -> ClientValue {
            self.as_slice().encode_client_arg()
        }
    }

    impl<T: ClientArgEncode> ClientArgEncode for &[T] {
        fn encode_client_arg(&self) -> ClientValue {
            (*self).encode_client_arg()
        }
    }

    impl<T: ClientArgEncode> ClientArgEncode for Option<T> {
        fn encode_client_arg(&self) -> ClientValue {
            match self {
                Some(value) => value.encode_client_arg(),
                None => ClientValue::Null,
            }
        }
    }

    impl ClientArgEncode for ClientObjectValue {
        fn encode_client_arg(&self) -> ClientValue {
            self.0.clone()
        }
    }

    /// Convert a `ClientValue` returned by a generated evaluator into
    /// the helper's declared Rust return type.
    pub trait ClientReturnValue: Sized {
        fn from_client_value(value: ClientValue) -> Self;
    }

    impl ClientReturnValue for () {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::Unit => (),
                other => panic_client_type("unit", &other),
            }
        }
    }

    impl ClientReturnValue for bool {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::Bool(value) => value,
                other => panic_client_type("bool", &other),
            }
        }
    }

    macro_rules! impl_signed_return {
        ($($ty:ty),* $(,)?) => {
            $(
                impl ClientReturnValue for $ty {
                    fn from_client_value(value: ClientValue) -> Self {
                        match value {
                            ClientValue::I64(value) => <$ty>::try_from(value).unwrap_or_else(|_| {
                                panic!("rsjs client helper returned i64 value outside `{}` range", stringify!($ty))
                            }),
                            ClientValue::U64(value) => <$ty>::try_from(value).unwrap_or_else(|_| {
                                panic!("rsjs client helper returned u64 value outside `{}` range", stringify!($ty))
                            }),
                            other => panic_client_type(stringify!($ty), &other),
                        }
                    }
                }
            )*
        };
    }

    macro_rules! impl_unsigned_return {
        ($($ty:ty),* $(,)?) => {
            $(
                impl ClientReturnValue for $ty {
                    fn from_client_value(value: ClientValue) -> Self {
                        match value {
                            ClientValue::U64(value) => <$ty>::try_from(value).unwrap_or_else(|_| {
                                panic!("rsjs client helper returned u64 value outside `{}` range", stringify!($ty))
                            }),
                            ClientValue::I64(value) => <$ty>::try_from(value).unwrap_or_else(|_| {
                                panic!("rsjs client helper returned i64 value outside `{}` range", stringify!($ty))
                            }),
                            other => panic_client_type(stringify!($ty), &other),
                        }
                    }
                }
            )*
        };
    }

    impl_signed_return!(i8, i16, i32, i64);
    impl_unsigned_return!(u8, u16, u32, u64);

    impl ClientReturnValue for f32 {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::F64(value) => value as f32,
                ClientValue::I64(value) => value as f32,
                ClientValue::U64(value) => value as f32,
                other => panic_client_type("f32", &other),
            }
        }
    }

    impl ClientReturnValue for f64 {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::F64(value) => value,
                ClientValue::I64(value) => value as f64,
                ClientValue::U64(value) => value as f64,
                other => panic_client_type("f64", &other),
            }
        }
    }

    impl ClientReturnValue for String {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::String(value) => value,
                other => panic_client_type("String", &other),
            }
        }
    }

    impl<T: ClientReturnValue> ClientReturnValue for Option<T> {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::Null => None,
                other => Some(T::from_client_value(other)),
            }
        }
    }

    impl<T: ClientReturnValue> ClientReturnValue for Vec<T> {
        fn from_client_value(value: ClientValue) -> Self {
            match value {
                ClientValue::Array(values) => {
                    values.into_iter().map(T::from_client_value).collect()
                }
                other => panic_client_type("array", &other),
            }
        }
    }

    impl ClientReturnValue for ClientObjectValue {
        fn from_client_value(value: ClientValue) -> Self {
            if !matches!(value, ClientValue::Object(_)) {
                panic_client_type("helper-internal object", &value);
            }
            Self(value)
        }
    }

    pub trait ClientHelperRuntimeMeta {
        const META: &'static crate::ClientHelperMeta;
    }

    /// Server-side runner for a `#[rsjs::client]` helper.
    pub fn run_client_helper<M, R>(name: &'static str, args: &[(&'static str, ClientArg)]) -> R
    where
        M: ClientHelperRuntimeMeta,
        R: ClientReturnValue,
    {
        let meta = M::META;
        if meta.name != name {
            panic!(
                "rsjs client helper metadata mismatch: generated call expected `{name}`, \
                 but metadata is for `{}`",
                meta.name
            );
        }
        if meta.params.len() != args.len() {
            panic!(
                "rsjs client helper `{name}` expected {} args, got {}",
                meta.params.len(),
                args.len()
            );
        }
        R::from_client_value((meta.server_eval)(args))
    }

    pub fn client_arg_value(
        args: &[(&'static str, ClientArg)],
        index: usize,
        expected_name: &'static str,
    ) -> ClientValue {
        let Some((actual_name, arg)) = args.get(index) else {
            panic!("rsjs client helper missing argument `{expected_name}` at index {index}");
        };
        if *actual_name != expected_name {
            panic!(
                "rsjs client helper argument mismatch at index {index}: expected \
                 `{expected_name}`, got `{actual_name}`"
            );
        }
        arg.value.clone()
    }

    pub fn client_is_non_empty(value: ClientValue) -> ClientValue {
        match value {
            ClientValue::String(value) => ClientValue::Bool(!value.is_empty()),
            ClientValue::Array(values) => ClientValue::Bool(!values.is_empty()),
            ClientValue::Unit | ClientValue::Null => ClientValue::Bool(false),
            other => panic_client_type("String or array for .is_non_empty()", &other),
        }
    }

    pub fn client_len(value: ClientValue) -> ClientValue {
        match value {
            // Rust's `str::len` / `String::len` count UTF-8 bytes, not
            // Unicode scalar values or UTF-16 code units. The browser-side
            // helper mirrors this with `TextEncoder`.
            ClientValue::String(value) => ClientValue::I64(value.len() as i64),
            ClientValue::Array(values) => ClientValue::I64(values.len() as i64),
            ClientValue::Unit | ClientValue::Null => ClientValue::I64(0),
            other => panic_client_type("String or array for .len()", &other),
        }
    }

    pub fn client_string_to_lowercase(value: ClientValue) -> ClientValue {
        match value {
            ClientValue::String(value) => ClientValue::String(value.to_lowercase()),
            ClientValue::Unit | ClientValue::Null => ClientValue::String(String::new()),
            other => panic_client_type("String for .to_lowercase()", &other),
        }
    }

    pub fn client_string_contains(haystack: ClientValue, needle: ClientValue) -> ClientValue {
        let haystack = client_value_to_string(haystack);
        let needle = client_value_to_string(needle);
        ClientValue::Bool(haystack.contains(&needle))
    }

    pub fn client_string_transform(op: &'static str, value: ClientValue) -> ClientValue {
        let value = match value {
            ClientValue::String(value) => value,
            ClientValue::Unit | ClientValue::Null => String::new(),
            other => panic_client_type("String for a string transform", &other),
        };
        match op {
            "to_uppercase" => ClientValue::String(value.to_uppercase()),
            "trim" => ClientValue::String(value.trim().to_string()),
            other => panic!("rsjs client helper uses unsupported string transform op `{other}`"),
        }
    }

    pub fn client_string_predicate(
        op: &'static str,
        haystack: ClientValue,
        needle: ClientValue,
    ) -> ClientValue {
        let haystack = client_value_to_string(haystack);
        let needle = client_value_to_string(needle);
        match op {
            "starts_with" => ClientValue::Bool(haystack.starts_with(&needle)),
            "ends_with" => ClientValue::Bool(haystack.ends_with(&needle)),
            other => panic!("rsjs client helper uses unsupported string predicate op `{other}`"),
        }
    }

    pub fn client_bool(value: ClientValue) -> bool {
        expect_bool(value)
    }

    /// Construct an ephemeral helper-local object.
    pub fn client_object(fields: Vec<(&'static str, ClientValue)>) -> ClientValue {
        ClientValue::Object(::std::rc::Rc::new(::std::cell::RefCell::new(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        )))
    }

    /// Read an own field from an ephemeral helper-local object.
    pub fn client_field(value: ClientValue, name: &'static str) -> ClientValue {
        let ClientValue::Object(fields) = value else {
            panic_client_type("object for a field read", &value);
        };
        let field = fields
            .borrow()
            .iter()
            .find_map(|(field, value)| (field == name).then(|| value.clone()))
            .unwrap_or_else(|| panic!("rsjs client helper object has no field `{name}`"));
        field
    }

    /// Assign an own field on an ephemeral helper-local object.
    pub fn client_set_field(value: &mut ClientValue, name: &'static str, next: ClientValue) {
        let ClientValue::Object(fields) = value else {
            panic_client_type("object for a field assignment", value);
        };
        let mut fields = fields.borrow_mut();
        let Some((_, field_value)) = fields.iter_mut().find(|(field, _)| field == name) else {
            panic!("rsjs client helper object has no field `{name}`");
        };
        *field_value = next;
    }

    pub fn client_binary_op(op: &'static str, lhs: ClientValue, rhs: ClientValue) -> ClientValue {
        match op {
            "checked_i32_add" => client_checked_integer("i32", "add", lhs, rhs),
            "checked_i32_sub" => client_checked_integer("i32", "sub", lhs, rhs),
            "checked_i32_mul" => client_checked_integer("i32", "mul", lhs, rhs),
            "checked_i32_div" => client_checked_integer("i32", "div", lhs, rhs),
            "checked_i32_rem" => client_checked_integer("i32", "rem", lhs, rhs),
            "checked_u32_add" => client_checked_integer("u32", "add", lhs, rhs),
            "checked_u32_sub" => client_checked_integer("u32", "sub", lhs, rhs),
            "checked_u32_mul" => client_checked_integer("u32", "mul", lhs, rhs),
            "checked_u32_div" => client_checked_integer("u32", "div", lhs, rhs),
            "checked_u32_rem" => client_checked_integer("u32", "rem", lhs, rhs),
            "checked_i64_add" => client_checked_integer("i64", "add", lhs, rhs),
            "checked_i64_sub" => client_checked_integer("i64", "sub", lhs, rhs),
            "checked_i64_mul" => client_checked_integer("i64", "mul", lhs, rhs),
            "checked_i64_div" => client_checked_integer("i64", "div", lhs, rhs),
            "checked_i64_rem" => client_checked_integer("i64", "rem", lhs, rhs),
            "checked_u64_add" => client_checked_integer("u64", "add", lhs, rhs),
            "checked_u64_sub" => client_checked_integer("u64", "sub", lhs, rhs),
            "checked_u64_mul" => client_checked_integer("u64", "mul", lhs, rhs),
            "checked_u64_div" => client_checked_integer("u64", "div", lhs, rhs),
            "checked_u64_rem" => client_checked_integer("u64", "rem", lhs, rhs),
            "add" => client_add(lhs, rhs),
            "sub" => client_sub(lhs, rhs),
            "mul" => client_mul(lhs, rhs),
            "div" => client_div(lhs, rhs),
            "rem" => client_rem(lhs, rhs),
            "eq" => ClientValue::Bool(client_value_eq(&lhs, &rhs)),
            "neq" => ClientValue::Bool(!client_value_eq(&lhs, &rhs)),
            "lt" => ClientValue::Bool(client_cmp_lt(lhs, rhs)),
            "gt" => ClientValue::Bool(client_cmp_gt(lhs, rhs)),
            "le" => ClientValue::Bool(!client_cmp_gt(lhs, rhs)),
            "ge" => ClientValue::Bool(!client_cmp_lt(lhs, rhs)),
            "and" => ClientValue::Bool(expect_bool(lhs) && expect_bool(rhs)),
            "or" => ClientValue::Bool(expect_bool(lhs) || expect_bool(rhs)),
            other => panic!("rsjs client helper uses unsupported binary op `{other}`"),
        }
    }

    pub fn client_unary_op(op: &'static str, value: ClientValue) -> ClientValue {
        match op {
            "not" => ClientValue::Bool(!expect_bool(value)),
            "checked_i32_neg" => client_checked_integer_neg("i32", value),
            "checked_i64_neg" => client_checked_integer_neg("i64", value),
            "neg" => match value {
                ClientValue::F64(value) => ClientValue::F64(-value),
                ClientValue::I64(value) => {
                    ClientValue::I64(value.checked_neg().expect("rsjs i64 neg overflow"))
                }
                ClientValue::U64(value) => ClientValue::I64(
                    i64::try_from(value)
                        .expect("rsjs client helper u64 value does not fit in i64")
                        .checked_neg()
                        .expect("rsjs i64 neg overflow"),
                ),
                other => panic_client_type("number", &other),
            },
            other => panic!("rsjs client helper uses unsupported unary op `{other}`"),
        }
    }

    fn client_checked_integer(
        domain: &'static str,
        op: &'static str,
        lhs: ClientValue,
        rhs: ClientValue,
    ) -> ClientValue {
        let (min, max, unsigned) = checked_integer_bounds(domain);
        let lhs = checked_integer_operand(lhs, domain, min, max);
        let rhs = checked_integer_operand(rhs, domain, min, max);
        let result = match op {
            "add" => lhs.checked_add(rhs),
            "sub" => lhs.checked_sub(rhs),
            "mul" => lhs.checked_mul(rhs),
            "div" => {
                if rhs == 0 || (!unsigned && lhs == min && rhs == -1) {
                    None
                } else {
                    Some(lhs / rhs)
                }
            }
            "rem" => {
                if rhs == 0 || (!unsigned && lhs == min && rhs == -1) {
                    None
                } else {
                    Some(lhs % rhs)
                }
            }
            _ => unreachable!("generated checked helper op"),
        }
        .filter(|value| (min..=max).contains(value))
        .unwrap_or_else(|| panic!("rsjs checked {domain} {op} failed"));
        checked_integer_result(result, domain, unsigned)
    }

    fn client_checked_integer_neg(domain: &'static str, value: ClientValue) -> ClientValue {
        let (min, max, unsigned) = checked_integer_bounds(domain);
        assert!(!unsigned, "rsjs checked negation requires a signed domain");
        let value = checked_integer_operand(value, domain, min, max);
        let result = value
            .checked_neg()
            .filter(|value| (min..=max).contains(value))
            .unwrap_or_else(|| panic!("rsjs checked {domain} negation failed"));
        checked_integer_result(result, domain, false)
    }

    fn checked_integer_bounds(domain: &str) -> (i128, i128, bool) {
        match domain {
            "i32" => (i32::MIN as i128, i32::MAX as i128, false),
            "u32" => (0, u32::MAX as i128, true),
            "i64" => (i64::MIN as i128, i64::MAX as i128, false),
            "u64" => (0, u64::MAX as i128, true),
            _ => panic!("rsjs checked helper uses unsupported domain `{domain}`"),
        }
    }

    fn checked_integer_operand(value: ClientValue, domain: &str, min: i128, max: i128) -> i128 {
        let value = match value {
            ClientValue::I64(value) => i128::from(value),
            ClientValue::U64(value) => i128::from(value),
            other => panic_client_type(domain, &other),
        };
        if !(min..=max).contains(&value) {
            panic!("rsjs checked {domain} operand {value} is out of range");
        }
        value
    }

    fn checked_integer_result(value: i128, domain: &str, unsigned: bool) -> ClientValue {
        if unsigned {
            ClientValue::U64(
                u64::try_from(value)
                    .unwrap_or_else(|_| panic!("rsjs checked {domain} result is invalid")),
            )
        } else {
            ClientValue::I64(
                i64::try_from(value)
                    .unwrap_or_else(|_| panic!("rsjs checked {domain} result is invalid")),
            )
        }
    }

    pub fn client_value_from_return<T: ClientArgEncode>(value: T) -> ClientValue {
        value.encode_client_arg()
    }

    fn client_add(lhs: ClientValue, rhs: ClientValue) -> ClientValue {
        match (lhs, rhs) {
            (ClientValue::String(mut lhs), rhs) => {
                lhs.push_str(&client_value_to_string(rhs));
                ClientValue::String(lhs)
            }
            (lhs, ClientValue::String(rhs)) => {
                let mut lhs = client_value_to_string(lhs);
                lhs.push_str(&rhs);
                ClientValue::String(lhs)
            }
            (lhs, rhs) if any_f64(&lhs, &rhs) => ClientValue::F64(as_f64(lhs) + as_f64(rhs)),
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => {
                ClientValue::U64(lhs.checked_add(rhs).expect("rsjs u64 add overflow"))
            }
            (lhs, rhs) => ClientValue::I64(
                as_i64(lhs)
                    .checked_add(as_i64(rhs))
                    .expect("rsjs i64 add overflow"),
            ),
        }
    }

    fn client_sub(lhs: ClientValue, rhs: ClientValue) -> ClientValue {
        if any_f64(&lhs, &rhs) {
            return ClientValue::F64(as_f64(lhs) - as_f64(rhs));
        }
        match (lhs, rhs) {
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => {
                ClientValue::U64(lhs.checked_sub(rhs).expect("rsjs u64 sub underflow"))
            }
            (lhs, rhs) => ClientValue::I64(
                as_i64(lhs)
                    .checked_sub(as_i64(rhs))
                    .expect("rsjs i64 sub overflow"),
            ),
        }
    }

    fn client_mul(lhs: ClientValue, rhs: ClientValue) -> ClientValue {
        if any_f64(&lhs, &rhs) {
            return ClientValue::F64(as_f64(lhs) * as_f64(rhs));
        }
        match (lhs, rhs) {
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => {
                ClientValue::U64(lhs.checked_mul(rhs).expect("rsjs u64 mul overflow"))
            }
            (lhs, rhs) => ClientValue::I64(
                as_i64(lhs)
                    .checked_mul(as_i64(rhs))
                    .expect("rsjs i64 mul overflow"),
            ),
        }
    }

    fn client_div(lhs: ClientValue, rhs: ClientValue) -> ClientValue {
        if any_f64(&lhs, &rhs) {
            return ClientValue::F64(as_f64(lhs) / as_f64(rhs));
        }
        match (lhs, rhs) {
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => ClientValue::U64(lhs / rhs),
            (lhs, rhs) => ClientValue::I64(as_i64(lhs) / as_i64(rhs)),
        }
    }

    fn client_rem(lhs: ClientValue, rhs: ClientValue) -> ClientValue {
        if any_f64(&lhs, &rhs) {
            return ClientValue::F64(as_f64(lhs) % as_f64(rhs));
        }
        match (lhs, rhs) {
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => ClientValue::U64(lhs % rhs),
            (lhs, rhs) => ClientValue::I64(as_i64(lhs) % as_i64(rhs)),
        }
    }

    fn client_cmp_lt(lhs: ClientValue, rhs: ClientValue) -> bool {
        match (lhs, rhs) {
            (ClientValue::String(lhs), ClientValue::String(rhs)) => lhs < rhs,
            (lhs, rhs) if any_f64(&lhs, &rhs) => as_f64(lhs) < as_f64(rhs),
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => lhs < rhs,
            (lhs, rhs) => as_i64(lhs) < as_i64(rhs),
        }
    }

    fn client_cmp_gt(lhs: ClientValue, rhs: ClientValue) -> bool {
        match (lhs, rhs) {
            (ClientValue::String(lhs), ClientValue::String(rhs)) => lhs > rhs,
            (lhs, rhs) if any_f64(&lhs, &rhs) => as_f64(lhs) > as_f64(rhs),
            (ClientValue::U64(lhs), ClientValue::U64(rhs)) => lhs > rhs,
            (lhs, rhs) => as_i64(lhs) > as_i64(rhs),
        }
    }

    fn client_value_eq(lhs: &ClientValue, rhs: &ClientValue) -> bool {
        match (lhs, rhs) {
            (ClientValue::I64(lhs), ClientValue::U64(rhs)) => *lhs >= 0 && *lhs as u64 == *rhs,
            (ClientValue::U64(lhs), ClientValue::I64(rhs)) => *rhs >= 0 && *lhs == *rhs as u64,
            (lhs, rhs) if any_f64(lhs, rhs) => as_f64_ref(lhs) == as_f64_ref(rhs),
            _ => lhs == rhs,
        }
    }

    fn any_f64(lhs: &ClientValue, rhs: &ClientValue) -> bool {
        matches!(lhs, ClientValue::F64(_)) || matches!(rhs, ClientValue::F64(_))
    }

    fn as_f64(value: ClientValue) -> f64 {
        match value {
            ClientValue::F64(value) => value,
            ClientValue::I64(value) => value as f64,
            ClientValue::U64(value) => value as f64,
            other => panic_client_type("number", &other),
        }
    }

    fn as_f64_ref(value: &ClientValue) -> f64 {
        match value {
            ClientValue::F64(value) => *value,
            ClientValue::I64(value) => *value as f64,
            ClientValue::U64(value) => *value as f64,
            other => panic_client_type("number", other),
        }
    }

    fn as_i64(value: ClientValue) -> i64 {
        match value {
            ClientValue::I64(value) => value,
            ClientValue::U64(value) => {
                i64::try_from(value).expect("rsjs client helper u64 value does not fit in i64")
            }
            other => panic_client_type("integer", &other),
        }
    }

    fn expect_bool(value: ClientValue) -> bool {
        match value {
            ClientValue::Bool(value) => value,
            other => panic_client_type("bool", &other),
        }
    }

    fn client_value_to_string(value: ClientValue) -> String {
        match value {
            ClientValue::Unit | ClientValue::Null => String::new(),
            ClientValue::Bool(value) => value.to_string(),
            ClientValue::I64(value) => value.to_string(),
            ClientValue::U64(value) => value.to_string(),
            ClientValue::F64(value) => value.to_string(),
            ClientValue::String(value) => value,
            ClientValue::Array(values) => values
                .into_iter()
                .map(client_value_to_string)
                .collect::<Vec<_>>()
                .join(","),
            ClientValue::Object(_) => {
                panic!("rsjs client helper objects cannot be converted to strings")
            }
        }
    }

    fn panic_client_type(expected: &str, got: &ClientValue) -> ! {
        panic!(
            "rsjs client helper expected {expected}, got {}",
            client_value_kind(got)
        )
    }

    fn client_value_kind(value: &ClientValue) -> &'static str {
        match value {
            ClientValue::Unit => "unit",
            ClientValue::Null => "null",
            ClientValue::Bool(_) => "bool",
            ClientValue::I64(_) => "i64",
            ClientValue::U64(_) => "u64",
            ClientValue::F64(_) => "f64",
            ClientValue::String(_) => "String",
            ClientValue::Array(_) => "array",
            ClientValue::Object(_) => "object",
        }
    }
}

// Client-boundary codec traits and metadata vocabulary.
mod client_codec;
pub use client_codec::{
    client_helper_export_name, ClientDecode, ClientDecodeError, ClientEncode, ClientHelperMeta,
    ClientHelperServerEval, ClientModuleMeta, ClientPropsField, ClientPropsFieldType,
    ClientPropsMeta, ClientPropsShape,
};

// Runtime island manifest. Populated at link time via linkme distributed
// slices submitted from `#[rsjs(component...)]` and
// `#[rsjs::client_module]` macro
// expansions; consumed by framework adapters and the optional collector
// helper that materialises a `manifest.json` + per-asset `.js` files.
pub mod manifest;
pub use manifest::{
    client_class_filename, client_module_filename, client_module_ir_json_source,
    client_module_js_source, deferred_island_bootstrap_js, deferred_island_bootstrap_tag,
    deferred_island_bootstrap_tag_with_nonce, manifest, manifest_json,
    rewrite_runtime_import_specifiers, should_defer_island_asset, write_manifest_assets,
    write_manifest_json, CanonicalRsjsAssetUrls, ClientClassManifestEntry, ClientModuleAsset,
    ClientModuleManifestEntry, ComponentFragmentAsset, ComponentFragmentManifestEntry,
    DeferredIslandAsset, IslandHydrationStrategy, IslandManifestEntry, IslandModuleAsset, Manifest,
    PageRuntimeEmissionState, PageRuntimePlan, PageRuntimePlanError, RsjsAssetRef,
    RsjsAssetUrlResolver, RuntimeAssetPlanError, RuntimeAssets, RuntimeFeatureSet,
    CLIENT_CLASS_REGISTRY, CLIENT_MODULE_REGISTRY, COMPONENT_FRAGMENT_REGISTRY, ISLAND_REGISTRY,
};

// Re-export `linkme` so generated code (and any consumer of the
// distributed-slice machinery) resolves it through `::rsjs::linkme`
// without forcing user crates to declare a direct `linkme` dependency.
pub use manifest::linkme;

// Re-export hidden helpers used by the `#[derive(ClientProps)]`-emitted
// code. These are not part of the public API; they are exposed only so
// the derive expansion can call into them by absolute path.
#[doc(hidden)]
pub mod __codec_internals {
    pub use crate::client_codec::{
        expect_byte, expect_literal, parse_json_string, scan_json_number, skip_ws,
        write_json_string,
    };
}

// ---------------------------------------------------------------------------
// Public primitives that islands import.
// ---------------------------------------------------------------------------
//
// These are deliberately thin Rust types — the rsjs compiler recognises
// them by name and lowers them to JavaScript at build time. At runtime
// on the server they are no-ops (or carry initial values, for
// hydration); the actual reactive behaviour lives in the generated
// client code + `rsjs-runtime.js`.

/// Browser protocol version for page-scoped shared-state seed metadata.
pub const SHARED_STATE_ABI_VERSION: u32 = rsjs_transport::SHARED_STATE_RUNTIME_ABI_VERSION;

/// Equality policy used by a generated shared-state field cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SharedStateEquality {
    /// JavaScript `Object.is`, matching ordinary rsjs signals.
    ObjectIs,
}

/// Static metadata for one top-level shared-state field.
#[derive(Debug, Clone, Copy)]
pub struct SharedStateFieldMeta {
    pub id: u32,
    pub name: &'static str,
    pub rust_type: &'static str,
    pub equality: SharedStateEquality,
}

/// Static protocol metadata emitted by `#[derive(rsjs::SharedState)]`.
#[derive(Debug, Clone, Copy)]
pub struct SharedStateMeta {
    pub abi_version: u32,
    pub type_key: &'static str,
    #[doc(hidden)]
    pub schema_hash_fn: fn() -> &'static str,
    pub rust_type: &'static str,
    pub fields: &'static [SharedStateFieldMeta],
    #[doc(hidden)]
    pub wide_paths_json_fn: fn() -> &'static str,
}

impl SharedStateMeta {
    /// Transitive wire-schema hash, including nested client-codec shapes.
    pub fn schema_hash(&self) -> &'static str {
        (self.schema_hash_fn)()
    }
}

/// Trait implemented by `#[derive(rsjs::SharedState)]`.
pub trait SharedState:
    ClientEncode + ClientDecode + __private::SharedStateAbi + Sized + 'static
{
    type Handle: Clone;

    const TYPE_KEY: &'static str;
    const META: &'static SharedStateMeta;

    /// Transitive wire-schema hash for provider/consumer compatibility.
    fn schema_hash() -> &'static str;

    #[doc(hidden)]
    fn __rsjs_wide_paths_json() -> &'static str;

    #[doc(hidden)]
    fn __rsjs_handle_from_server(value: ::std::rc::Rc<Self>) -> Self::Handle;
}

/// Rust integer division, written explicitly.
///
/// Inside an RSJS-analyzed component, the compiler must prove one matching
/// `i32`, `u32`, `i64`,
/// or `u64` domain so division by zero and signed `MIN / -1` overflow behave
/// exactly like Rust. Two independently typed operands can infer `T`. A
/// page-shared field is intentionally schema-less at macro expansion, so give
/// that boundary explicitly:
///
/// ```rust,ignore
/// let quotient = rsjs::div_int::<i64>(total.get(), count.get());
/// ```
///
/// Use the primitive type itself (or its canonical
/// `core::primitive::i64`/`std::primitive::i64` path), not a reference,
/// wrapper, alias, or lookalike path. Shared reads require this explicit width
/// at every `div_int` call boundary; an earlier explicit call does not make a
/// later bare call infer by source order.
///
/// The generated client code uses checked fixed-width division, not the legacy
/// representation-only `num_div_int` helper. An explicit width is also
/// re-checked against the real shared-field Rust type in generated server code.
#[inline(always)]
pub fn div_int<T: core::ops::Div<Output = T>>(a: T, b: T) -> T {
    a / b
}

/// Float division, written explicitly. See [`div_int`].
#[inline(always)]
pub fn div_f64(a: impl Into<f64>, b: impl Into<f64>) -> f64 {
    a.into() / b.into()
}

/// The shared-state protocol identities an island component's own module
/// resolved when the island was compiled.
///
/// Implemented by `#[rsjs(component...)]` on the component's `Self` type, one entry per shared
/// store in store-id order. It exists so a *parent* that fully inlines this
/// component can prove the component's Rust type names still denote the same
/// states after being re-resolved in the parent's scope.
///
/// A component records the shared-state type as it wrote it, usually a bare
/// `Cfg`. Inlining replays that name in the parent module, where Rust resolves
/// it against the parent's items. If the parent has its own `Cfg`, the inlined
/// markup would bind to the parent's provider scope: wrong data, no error, no
/// exception. The parent therefore emits
/// `assert!(<Cfg as SharedState>::TYPE_KEY == <Child as IslandSharedStores>::SHARED_TYPE_KEYS[i])`,
/// which is a const comparison of two trait-resolved items. Both sides are
/// resolved by rustc in their own crates, so the check needs no registry, no
/// name matching, and no build ordering.
pub trait IslandSharedStores {
    /// `SharedState::TYPE_KEY` per shared store, indexed by store id.
    const SHARED_TYPE_KEYS: &'static [&'static str];
}

/// Compute the shared-state protocol hash from the complete recursive codec
/// metadata. Generated `SharedState` impls memoize this result once per type.
#[doc(hidden)]
pub fn __shared_state_schema_hash(type_key: &str, meta: &ClientPropsMeta) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    fn update_str(hasher: &mut Sha256, value: &str) {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }

    fn update_field_type(hasher: &mut Sha256, ty: &ClientPropsFieldType) {
        match ty {
            ClientPropsFieldType::Bool => hasher.update(b"bool"),
            ClientPropsFieldType::I32 => hasher.update(b"i32"),
            ClientPropsFieldType::U32 => hasher.update(b"u32"),
            ClientPropsFieldType::I64 => hasher.update(b"i64"),
            ClientPropsFieldType::U64 => hasher.update(b"u64"),
            ClientPropsFieldType::F32 => hasher.update(b"f32"),
            ClientPropsFieldType::F64 => hasher.update(b"f64"),
            ClientPropsFieldType::String => hasher.update(b"string"),
            ClientPropsFieldType::OptionOf(inner) => {
                hasher.update(b"option(");
                update_field_type(hasher, inner);
                hasher.update(b")");
            }
            ClientPropsFieldType::VecOf(inner) => {
                hasher.update(b"vec(");
                update_field_type(hasher, inner);
                hasher.update(b")");
            }
            ClientPropsFieldType::NestedClientProps(nested) => {
                hasher.update(b"client_props(");
                update_meta(hasher, nested);
                hasher.update(b")");
            }
        }
    }

    fn update_meta(hasher: &mut Sha256, meta: &ClientPropsMeta) {
        match meta.shape {
            ClientPropsShape::Object => {
                hasher.update(b"object(");
                hasher.update((meta.fields.len() as u64).to_le_bytes());
                for field in meta.fields {
                    update_str(hasher, field.name);
                    update_field_type(hasher, field.ty);
                    hasher.update(b"object_is");
                }
                hasher.update(b")");
            }
            ClientPropsShape::TaggedString { variants } => {
                hasher.update(b"tagged_string(");
                hasher.update((variants.len() as u64).to_le_bytes());
                for variant in variants {
                    update_str(hasher, variant);
                }
                hasher.update(b")");
            }
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(b"rsjs.shared-state.v2");
    hasher.update(SHARED_STATE_ABI_VERSION.to_le_bytes());
    update_str(&mut hasher, type_key);
    update_meta(&mut hasher, meta);
    let digest = hasher.finalize();
    let mut output = String::with_capacity("sha256:".len() + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

/// Collect every recursively nested wide-integer location in a shared-state
/// codec. Array elements use the `"*"` segment, so a schema such as
/// `Vec<Order { total: i64 }>` produces `[["orders","*","total"]]`.
#[doc(hidden)]
pub fn __shared_state_wide_paths_json(meta: &ClientPropsMeta) -> String {
    fn walk<'a>(
        ty: &'a ClientPropsFieldType,
        path: &mut Vec<&'a str>,
        paths: &mut Vec<Vec<&'a str>>,
    ) {
        match ty {
            ClientPropsFieldType::I64 | ClientPropsFieldType::U64 => {
                paths.push(path.clone());
            }
            ClientPropsFieldType::OptionOf(inner) => walk(inner, path, paths),
            ClientPropsFieldType::VecOf(inner) => {
                path.push("*");
                walk(inner, path, paths);
                path.pop();
            }
            ClientPropsFieldType::NestedClientProps(nested) => {
                for field in nested.fields {
                    path.push(field.name);
                    walk(field.ty, path, paths);
                    path.pop();
                }
            }
            ClientPropsFieldType::Bool
            | ClientPropsFieldType::I32
            | ClientPropsFieldType::U32
            | ClientPropsFieldType::F32
            | ClientPropsFieldType::F64
            | ClientPropsFieldType::String => {}
        }
    }

    let mut paths = Vec::new();
    for field in meta.fields {
        let mut path = vec![field.name];
        walk(field.ty, &mut path, &mut paths);
    }
    serde_json::to_string(&paths).expect("static shared-state wide paths must serialize")
}

/// Typed handle for one top-level field of a shared-state schema.
///
/// The Rust value exists for SSR and editor inference. Inside an RSJS-analyzed
/// component, the compiler lowers the handle to the corresponding page-runtime
/// field cell.
///
/// The handle borrows the field from the provider's `Rc` snapshot instead of
/// owning a copy: constructing it is free. [`SharedField::get`] performs one
/// clone for ordinary authored SSR reads, while generated text emission can
/// borrow through the hidden accessor without cloning.
pub struct SharedField<'a, T> {
    _value: &'a T,
    _field_id: u32,
    _field_name: &'static str,
}

impl<T> Clone for SharedField<'_, T> {
    fn clone(&self) -> Self {
        Self {
            _value: self._value,
            _field_id: self._field_id,
            _field_name: self._field_name,
        }
    }
}

impl<'a, T> SharedField<'a, T> {
    #[doc(hidden)]
    pub fn __from_server(value: &'a T, field_id: u32, field_name: &'static str) -> Self {
        Self {
            _value: value,
            _field_id: field_id,
            _field_name: field_name,
        }
    }

    /// Read the server snapshot value. Clones the field value exactly once.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self._value.clone()
    }

    /// Borrow the provider snapshot for allocation-free SSR emission.
    /// Browser code still lowers source `.get()` calls to the live field cell;
    /// this method is used only by generated server render code.
    #[doc(hidden)]
    pub fn __rsjs_get_ref(&self) -> &'a T {
        self._value
    }

    pub fn set(&self, _next: T) {}

    pub fn update<F>(&self, _update: F)
    where
        F: FnOnce(T) -> T,
    {
    }
}

/// Acquire the nearest `SharedStateProvider` for `T`.
///
/// This call is compiler-recognized and is valid only in an origin-capable
/// `#[rsjs(component, client)]` render prelude. The generated server render
/// acquires the provider snapshot from
/// `WebContext`; generated browser code acquires the same scope from the page
/// runtime registry.
pub fn shared<T: SharedState>() -> T::Handle {
    panic!(
        "rsjs::shared::<{}>() is only valid inside an origin-capable RSJS render; add #[rsjs(component, client)] to the render trait impl",
        core::any::type_name::<T>()
    )
}

/// Group several synchronous signal/shared-field writes into one DOM flush.
///
/// Signal values change immediately; subscribed DOM effects remain
/// microtask-batched. Inside an event handler the compiler flattens this
/// closure into the handler IR, preserving that scheduler contract.
pub fn batch<F, R>(run: F) -> R
where
    F: FnOnce() -> R,
{
    run()
}

/// Establish one lexical shared-state scope for `children`.
///
/// Providers do not render a DOM wrapper. The server records a scope id and
/// latent seed in the request-local page registry, while consuming island
/// wrappers carry the exact resolved scope in their hydration metadata. The
/// seed is published only when a committed reachable consumer acquires this
/// provider; an unused provider exposes no page data.
///
/// Once any consumer makes the provider live, the complete `state` value is
/// serialized into page HTML, including fields that consumer does not read.
/// Every field must therefore be safe to expose to the browser; shared state
/// must never contain secrets or server-only policy data.
pub struct SharedStateProvider<S, C> {
    pub state: S,
    pub children: C,
}

impl<L, B, S, C> proa_core::WebRenderSync<L, B> for SharedStateProvider<S, C>
where
    B: proa_core::WriteBuf,
    L: proa_core::DataLoader,
    S: SharedState,
    C: proa_core::WebRenderSync<L, B>,
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let state_json = self.state.encode();
        let state = ::std::rc::Rc::new(self.state);
        let _scope = cx.rsjs_shared_state_enter(
            S::TYPE_KEY,
            S::schema_hash(),
            S::META.rust_type,
            S::__rsjs_wide_paths_json(),
            state,
            state_json,
        )?;
        self.children.render(cx)
    }
}

impl<L, B, S, C> proa_core::WebRender<L, B> for SharedStateProvider<S, C>
where
    B: proa_core::WriteBuf,
    L: proa_core::DataLoader,
    S: SharedState,
    C: proa_core::WebRender<L, B>,
{
    fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> proa_core::RenderOutcome<
        impl core::future::Future<Output = Result<(), proa_core::WriteError>>,
    > {
        proa_core::RenderOutcome::pending(async move {
            let state_json = self.state.encode();
            let state = ::std::rc::Rc::new(self.state);
            let _scope = cx.rsjs_shared_state_enter(
                S::TYPE_KEY,
                S::schema_hash(),
                S::META.rust_type,
                S::__rsjs_wide_paths_json(),
                state,
                state_json,
            )?;
            self.children.render(cx).resolve().await
        })
    }
}

/// A signal: a reactive cell holding a value of type `T`.
///
/// Reads (`.get()`) track the current effect as a subscriber.
/// Writes (`.set()`, `.update()`) notify subscribers.
///
/// **Note**: this server-side type is a placeholder. The compiler
/// translates `signal(...)`, `count.get()`, `count.set(...)` calls
/// into JavaScript that uses the rsjs runtime's signal scheduler.
/// On the server the value is captured at island construction time
/// and embedded in the rendered HTML; mutations on the server have
/// no effect after the initial render.
pub struct Signal<T> {
    _value: T,
}

/// Construct a signal with an initial value.
///
/// **Note**: see [`Signal`] for the build-time vs runtime semantics.
pub fn signal<T>(initial: T) -> Signal<T> {
    Signal { _value: initial }
}

/// A reactive memo: a read-only derived cell that caches its current value.
/// Inputs come from the reactive reads in its closure; no manual dependency
/// array is required. Input writes schedule a batched recomputation, and
/// downstream subscribers are notified only when the result changes.
///
/// **Note**: this server-side type is a placeholder, like [`Signal`].
/// On the server the closure runs once over the signals' initial
/// values and the result is what SSR renders. On the client the
/// compiler lowers `memo(...)` to the runtime's `memo()`
/// derived cell, which recomputes on input writes and only notifies
/// subscribers when the derived value actually changes.
pub struct Memo<T> {
    _value: T,
}

impl<T> Memo<T> {
    /// Read the current derived value. Inside an RSJS-analyzed render body this
    /// subscribes the surrounding reactive scope; on the server it
    /// returns the value computed from the signals' initial values.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self._value.clone()
    }

    /// Return whether the current derived value is non-empty. Inside an
    /// RSJS-analyzed render body this lowers to a small client-side predicate.
    pub fn is_non_empty(&self) -> bool
    where
        T: AsRef<str>,
    {
        !self._value.as_ref().is_empty()
    }
}

/// Construct a reactive memo from a closure over live inputs.
///
/// Inside an RSJS-analyzed component, the compiler derives the dependency
/// list from the closure's reads and emits it for the browser runtime. The
/// closure must use the supported reactive expression grammar; use a pure
/// `#[rsjs::client]` helper for supported calculations with intermediate locals.
///
/// ```ignore
/// let count = signal(0_i64);
/// let doubled = rsjs::memo(|| count.get() * 2_i64);
/// ```
///
/// **Note**: see [`Memo`] for the build-time vs runtime semantics.
pub fn memo<T, F>(f: F) -> Memo<T>
where
    F: Fn() -> T,
{
    Memo { _value: f() }
}

/// Run `f` every `ms` milliseconds for the lifetime of the island.
///
/// Declared in the island prelude; the compiler lowers it to the
/// runtime's `interval()` primitive, whose disposer is tied to the
/// island's teardown so a removed island never fires a stale tick.
/// On the server this is a no-op: nothing ticks during SSR.
pub fn interval<F>(_ms: u32, _f: F)
where
    F: Fn(),
{
}

/// Wrap an event handler so bursts of calls collapse until `delay_ms`
/// milliseconds pass without a new call (trailing edge only).
///
/// The compiler lowers this to the runtime's `debounce()` wrapper and
/// ties cancellation of any pending invocation to the island's
/// teardown. On the server the closure is returned unchanged so the
/// authoring shape type-checks.
/// In analyzed templates, the handler must be a closure and the delay must
/// be an integer literal, e.g. `debounce(|event| query.set(event.value()), 300)`.
pub fn debounce<F>(f: F, _delay_ms: u32) -> F {
    f
}

/// Declare a reusable event handler inside an
/// `#[rsjs(component, client)]` render body.
///
/// Inside an origin-capable RSJS block the proc macro recognises
/// `let on_change = rsjs::handler(|payload| ...);` and lets event
/// attributes invoke it with direct call syntax:
///
/// ```ignore
/// let on_save = rsjs::handler(|_| saved.set(true));
/// html_sync! {
///     <button on_click={on_save(())}>"Save"</button>
/// }
/// ```
///
/// This function returns the closure unchanged so rust-analyzer and
/// non-macro experiments can type-check the authoring shape. Real
/// client behavior is produced by the macro and `rsjs-runtime.js`.
pub fn handler<F>(f: F) -> F {
    f
}

/// Declare a reusable RSJS DOM event handler inside an
/// `#[rsjs(component, client)]` render body.
///
/// This is the IDE-friendly form for non-trivial handlers. The closure body is
/// ordinary Rust source before the `html_sync!` macro, so editors can complete
/// fields such as `self.on_change` while typing. Event attributes call the
/// handler by name and may pass one payload argument from the template:
///
/// ```ignore
/// let on_pick = rsjs::event_handler(|event: rsjs::Event, day: u8| {
///     console::log(event.closest_attr("data-date"));
///     self.on_select.call(day);
/// });
///
/// html_sync! {
///     <button data_date="sample-day" on_click={on_pick(17)}>"Pick"</button>
/// }
/// ```
///
/// Event types may carry a marker for the handler's `currentTarget`: an element
/// marker such as `rsjs::MouseEvent<elem::Button>` for local bindings, or
/// `rsjs::web::dom::Document` / `Window` for global bindings.
///
/// The Rust return value is an authoring stub. A closure taking only the event
/// becomes callable as `handler()`, while a closure taking event plus payload
/// becomes callable as `handler(payload)`. Real client behavior is produced by
/// the macro and `rsjs-runtime.js`.
pub fn event_handler<F, Args>(f: F) -> <F as EventHandlerStub<Args>>::Callable
where
    F: EventHandlerStub<Args>,
{
    f.into_callable()
}

/// Authoring-only conversion used by [`event_handler`].
#[doc(hidden)]
pub trait EventHandlerStub<Args> {
    type Callable;

    fn into_callable(self) -> Self::Callable;
}

/// Marker for event parameter stubs accepted by [`event_handler`].
#[doc(hidden)]
pub use rsjs_web::EventParam as EventParamStub;

/// Zero-sized authoring witness returned by an [`event_handler`] invocation.
///
/// The RSJS compiler removes event attributes from server output. Keeping the
/// event parameter in this otherwise inert value lets ordinary Rust trait
/// checking validate an event binding before descriptor/linker compilation.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default)]
pub struct EventBinding<E: EventParamStub>(core::marker::PhantomData<fn() -> E>);

impl<E: EventParamStub> proa_core::shared::attr_value::AttrValue for EventBinding<E> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        false
    }

    #[inline(always)]
    fn render_attr_value<B: proa_core::WriteBuf, L: proa_core::DataLoader>(
        &self,
        _cx: &mut proa_core::WebContext<L, B>,
    ) -> Result<(), proa_core::WriteError> {
        Ok(())
    }
}

fn event_handler_noop<E: EventParamStub>() -> EventBinding<E> {
    EventBinding(core::marker::PhantomData)
}

fn event_handler_payload_noop<E: EventParamStub, P>(_payload: P) -> EventBinding<E> {
    EventBinding(core::marker::PhantomData)
}

impl<F, E, R> EventHandlerStub<(E,)> for F
where
    E: EventParamStub,
    F: Fn(E) -> R,
{
    type Callable = fn() -> EventBinding<E>;

    fn into_callable(self) -> Self::Callable {
        event_handler_noop::<E>
    }
}

impl<F, E, P, R> EventHandlerStub<(E, P)> for F
where
    E: EventParamStub,
    F: Fn(E, P) -> R,
{
    type Callable = fn(P) -> EventBinding<E>;

    fn into_callable(self) -> Self::Callable {
        event_handler_payload_noop::<E, P>
    }
}

/// Placeholder type for callback fields on reusable rsjs components.
///
/// The full component-callback lowering is macro-managed. This wrapper exists
/// so component structs can name callback fields in source and IDEs can resolve
/// `self.on_change.call(value)` as a normal field access plus method call.
pub struct Handler<T> {
    f: fn(T),
}

impl<T> Copy for Handler<T> {}

impl<T> Clone for Handler<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Handler<T> {
    pub const fn new(f: fn(T)) -> Self {
        Self { f }
    }

    pub fn call(self, value: T) {
        (self.f)(value);
    }
}

fn noop<T>(_value: T) {}

/// No-op callback value for places that need to construct a component
/// without wiring a parent handler.
pub const fn noop_handler<T>() -> Handler<T> {
    Handler::new(noop::<T>)
}

/// Reactive query handle declared inside an async origin-capable component.
///
/// The server-side type is an authoring stub. The proc macro lowers
/// `query_with_initial(...)` declarations and query accessors to the rsjs query
/// runtime; these methods only keep IDEs and rustc happy before macro
/// expansion.
#[derive(Clone, Copy)]
pub struct Query<T> {
    _marker: core::marker::PhantomData<T>,
}

/// JSON-shaped value returned by `Query::data()`, `Query::error()`, and
/// mutation result/error accessors. Inside RSJS-analyzed templates, direct field
/// access such as `query.data().name` is rewritten to a reactive projection.
#[derive(Clone, Copy)]
pub struct QueryValue<T = ()> {
    _marker: core::marker::PhantomData<T>,
}

impl<T> core::ops::Deref for QueryValue<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unreachable!(
            "rsjs::QueryValue field access is an authoring stub rewritten by the RSJS component attribute"
        )
    }
}

/// Query refetch policy recognised by `rsjs::query_with_initial(...)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryOptions {
    pub interval_ms: Option<u32>,
    pub refetch_on_focus: bool,
    pub download_progress: bool,
    pub stream_text: bool,
    pub retry_count: u32,
    pub stale_time_ms: u32,
}

/// HTTP method recognised by [`FetchRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

/// Fetch credentials mode recognised by [`FetchRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchCredentials {
    SameOrigin,
    Include,
    Omit,
}

impl FetchCredentials {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SameOrigin => "same-origin",
            Self::Include => "include",
            Self::Omit => "omit",
        }
    }
}

/// Response decoder used by the query/mutation runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    Json,
    Text,
    Bytes,
    Empty,
}

impl ResponseKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::Bytes => "bytes",
            Self::Empty => "empty",
        }
    }
}

/// Static request body shape recognised by [`FetchRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestBody {
    Json(&'static str),
    Text(&'static str),
    BytesBase64(&'static str),
    BytesBase64Url(&'static str),
}

/// Static fetch request descriptor recognised by `rsjs::query_with_initial(...)`
/// and `rsjs::mutation(...)`.
///
/// The macro lowers this struct literal to a compact JSON descriptor
/// consumed by the browser runtime. Fields are intentionally static:
/// request shape belongs to the island definition, while dynamic
/// mutation payloads still flow through `mutation.run(args)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchRequest {
    pub url: &'static str,
    pub method: Option<HttpMethod>,
    pub credentials: Option<FetchCredentials>,
    pub response: Option<ResponseKind>,
    pub headers: &'static [(&'static str, &'static str)],
    pub body: Option<RequestBody>,
    pub timeout_ms: Option<u32>,
}

/// Structural cache key for `rsjs::query_with_initial(...)` and
/// `rsjs::mutation(...)`.
///
/// The proc macro recognizes `QueryKey::new(...)` and lowers tuple or array
/// parts into the browser runtime's structural key array. The value is only an
/// authoring marker; the macro reads the source expression before this stub is
/// ever used at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryKey;

impl QueryKey {
    pub fn new<T>(_parts: T) -> Self {
        Self
    }
}

/// Declare a fetch-backed query with a server-computed initial value.
///
/// The `initial` argument is ordinary server Rust. It can be a value loaded
/// through `cx.load_json(...).await?`, a precomputed prop, or any other value
/// that implements `serde::Serialize`. The component attribute records it in
/// the page-level query seed table, while adjacent per-island query metadata only
/// carries the effective key needed to find that seed.
pub fn query_with_initial<T>(
    _key: impl Sized,
    _initial: T,
    _client_fetcher: impl Sized,
    _options: QueryOptions,
) -> Query<T> {
    Query {
        _marker: core::marker::PhantomData,
    }
}

impl<T> Query<T> {
    pub fn data(&self) -> QueryValue<T> {
        QueryValue {
            _marker: core::marker::PhantomData,
        }
    }

    pub fn status(&self) -> String {
        String::new()
    }

    pub fn is_loading(&self) -> bool {
        false
    }

    pub fn is_success(&self) -> bool {
        false
    }

    pub fn is_error(&self) -> bool {
        false
    }

    pub fn is_refetching(&self) -> bool {
        false
    }

    pub fn is_stale(&self) -> bool {
        false
    }

    pub fn error(&self) -> QueryValue {
        QueryValue {
            _marker: core::marker::PhantomData,
        }
    }

    pub fn data_updated_at(&self) -> f64 {
        0.0
    }

    pub fn error_updated_at(&self) -> f64 {
        0.0
    }

    pub fn fetch_count(&self) -> u32 {
        0
    }

    pub fn download_loaded(&self) -> f64 {
        0.0
    }

    pub fn download_total(&self) -> Option<f64> {
        None
    }

    pub fn download_percent(&self) -> f64 {
        0.0
    }

    pub fn refetch(&self) {}

    pub fn invalidate(&self) {}
}

impl<T> QueryValue<T> {
    pub fn at(self, _json_pointer: &'static str) -> Self {
        self
    }

    pub fn as_str(self) -> String {
        String::new()
    }

    pub fn as_f64(self) -> f64 {
        0.0
    }

    /// Project a JSON value to an exact `i64`.
    ///
    /// Client lowering returns a BigInt. It accepts an integral safe
    /// JavaScript Number or a canonical decimal string in the full `i64`
    /// range; malformed, fractional, imprecise, and out-of-range inputs use
    /// the `0n` fallback. The server projection mirrors that rule as `0_i64`.
    pub fn as_i64(self) -> i64 {
        0
    }

    pub fn as_bool(self) -> bool {
        false
    }

    pub fn as_array(self) -> Self {
        self
    }

    pub fn as_object(self) -> Self {
        self
    }

    pub fn is_present(self) -> bool {
        false
    }

    pub fn is_non_empty(self) -> bool {
        false
    }

    pub fn eq<V>(self, _value: V) -> bool {
        false
    }

    pub fn neq<V>(self, _value: V) -> bool {
        false
    }

    pub fn coalesce<V>(self, _fallback: V) -> Self {
        self
    }
}

/// Reactive mutation handle declared inside an async origin-capable component.
#[derive(Clone, Copy)]
pub struct Mutation<Args, Result> {
    _marker: core::marker::PhantomData<(Args, Result)>,
}

/// Mutation invalidation policy after a successful `run(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationStrategy {
    Refetch,
    Reset,
}

/// Mutation retry / invalidation policy recognised by
/// `rsjs::mutation_with_options(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationOptions {
    pub retry_count: u32,
    pub on_success_invalidate_strategy: InvalidationStrategy,
}

impl Default for MutationOptions {
    fn default() -> Self {
        Self {
            retry_count: 0,
            on_success_invalidate_strategy: InvalidationStrategy::Refetch,
        }
    }
}

/// Declare a fetch-backed mutation with the v0.1 authoring contract:
/// `(key, client_runner, invalidates)`.
pub fn mutation<Args, Result>(
    _key: impl Sized,
    _client_runner: impl Sized,
    _invalidates: impl Sized,
) -> Mutation<Args, Result> {
    Mutation {
        _marker: core::marker::PhantomData,
    }
}

/// Declare a fetch-backed mutation with explicit retry / invalidation policy.
pub fn mutation_with_options<Args, Result>(
    _key: impl Sized,
    _client_runner: impl Sized,
    _invalidates: impl Sized,
    _options: MutationOptions,
) -> Mutation<Args, Result> {
    Mutation {
        _marker: core::marker::PhantomData,
    }
}

impl<Args, Result> Mutation<Args, Result> {
    pub fn run(&self, _args: Args) {}

    pub fn status(&self) -> String {
        String::new()
    }

    pub fn is_idle(&self) -> bool {
        true
    }

    pub fn is_pending(&self) -> bool {
        false
    }

    pub fn is_success(&self) -> bool {
        false
    }

    pub fn is_error(&self) -> bool {
        false
    }

    pub fn data(&self) -> QueryValue<Result> {
        QueryValue {
            _marker: core::marker::PhantomData,
        }
    }

    pub fn error(&self) -> QueryValue {
        QueryValue {
            _marker: core::marker::PhantomData,
        }
    }
}

/// Extension method for optional callback fields on reusable rsjs components.
///
/// Inside an RSJS-analyzed component, the proc macro recognizes
/// `self.on_change.call_if_some(payload)` as an optional callback-slot
/// invocation. The trait exists so the same source also type-checks in
/// rust-analyzer and in non-inlined server-render tests.
pub trait OptionalHandlerExt<T> {
    fn call_if_some(self, value: T);
}

impl<T> OptionalHandlerExt<T> for Option<Handler<T>> {
    fn call_if_some(self, value: T) {
        if let Some(handler) = self {
            handler.call(value);
        }
    }
}

/// Mark an HTML attribute value as server-only inside an RSJS-analyzed component.
///
/// The proc macro renders the wrapped value through the generated server
/// `html_sync!` body and deliberately emits no client-side binding for it.
/// Use this for static-per-render values such as recipe-computed class lists or
/// ids. Do not wrap signal reads here: server-only attributes are not updated
/// during hydration.
pub fn server_attr<T>(value: T) -> T {
    value
}

/// Client-side DOM node reference visible to rust-analyzer for RSJS authoring.
///
/// The proc macro recognizes `let input = node_ref::<HtmlInputElement>()`
/// plus `ref={input}` and lowers supported methods to direct JavaScript DOM
/// operations. The server-side type is a zero-sized authoring handle only.
#[derive(Clone, Copy)]
pub struct NodeRef<T> {
    _marker: core::marker::PhantomData<T>,
}

/// Declare a DOM node ref for use with `ref={...}` inside an rsjs template.
pub fn node_ref<T>() -> NodeRef<T> {
    NodeRef {
        _marker: core::marker::PhantomData,
    }
}

// Keep the original root names as compatibility re-exports. Node refs and
// typed event current targets now share these generated marker identities.
pub use rsjs_web::dom::{
    Element, HtmlDialogElement, HtmlElement, HtmlFormElement, HtmlInputElement, HtmlSelectElement,
    HtmlTextAreaElement,
};

// The facade schema generates this entire inherent method surface. Rust trait
// bounds decide which operations exist for each canonical element marker;
// the analyzer consumes the matching generated operation registry.
rsjs_web::__impl_dom_node_ref_methods!(NodeRef);

/// Element marker types for typed `currentTarget` event handles.
///
/// Event generics model the element the handler is attached to, equivalent to
/// React's `currentTarget`, not the raw bubbling `target`.
pub mod elem {
    pub use rsjs_web::dom::{
        Anchor, Any, Button, Dialog, Div, Element, ElementMarker, Form,
        HasChecked as CheckedElementMarker, HasValue as ValueElementMarker,
        HtmlElement as HTMLElement, Input, Label, OptionElement, Select, Span, Textarea,
    };

    pub type HTMLButtonElement = Button;
    pub type HTMLInputElement = Input;
    pub type HTMLTextAreaElement = Textarea;
    pub type HTMLSelectElement = Select;
    pub type HTMLFormElement = Form;
    pub type HTMLAnchorElement = Anchor;
    pub type HTMLDivElement = Div;
    pub type HTMLSpanElement = Span;
    pub type HTMLLabelElement = Label;
    pub type HTMLOptionElement = OptionElement;
    pub type HTMLDialogElement = Dialog;
}

/// Typed handle for an event's matched RSJS DOM target.
///
/// This generated authoring/compiler handle is shared with [`web`]. It is not
/// a server-side DOM object; RSJS erases it while lowering supported reads to
/// direct JavaScript.
pub use rsjs_web::CurrentTarget;

macro_rules! event_handle {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $name<E: rsjs_web::dom::DomTargetMarker = elem::Any>(
            core::marker::PhantomData<fn() -> E>,
        );
    };
}

event_handle!(
    Event,
    "Browser event handle visible to rust-analyzer for RSJS handler closures."
);

event_handle!(
    MouseEvent,
    "Mouse-event handle for typed RSJS handler parameters."
);

event_handle!(
    PointerEvent,
    "Pointer-event handle for typed RSJS handler parameters."
);

event_handle!(
    TouchEvent,
    "Touch-event handle for typed RSJS handler parameters."
);

event_handle!(
    KeyboardEvent,
    "Keyboard-event handle for typed RSJS handler parameters."
);

event_handle!(
    InputEvent,
    "Input-event handle for typed RSJS handler parameters."
);

event_handle!(
    SubmitEvent,
    "Submit-event handle for typed RSJS handler parameters."
);

event_handle!(
    ChangeEvent,
    "Change-event handle for typed RSJS handler parameters."
);

event_handle!(
    ClipboardEvent,
    "Clipboard-event handle for typed RSJS paste handlers."
);

event_handle!(
    WheelEvent,
    "Wheel-event handle for typed RSJS wheel handlers."
);

event_handle!(
    CompositionEvent,
    "Composition-event handle for typed RSJS IME handlers."
);

event_handle!(
    DragEvent,
    "Drag-event handle for typed RSJS drag/drop handlers."
);

event_handle!(
    ToggleEvent,
    "Toggle-event handle for typed RSJS toggle/beforetoggle handlers."
);

event_handle!(
    AnimationEvent,
    "Animation-event handle for typed RSJS CSS animation handlers."
);

event_handle!(
    TransitionEvent,
    "Transition-event handle for typed RSJS CSS transition handlers."
);

event_handle!(
    PopStateEvent,
    "PopState-event handle for typed RSJS history handlers."
);

event_handle!(
    HashChangeEvent,
    "HashChange-event handle for typed RSJS hashchange handlers."
);

event_handle!(
    StorageEvent,
    "Storage-event handle for typed RSJS cross-document storage handlers."
);

macro_rules! impl_event_param_stub {
    ($($ty:ident),* $(,)?) => {
        $(
            impl<E: rsjs_web::dom::DomTargetMarker> EventParamStub for $ty<E> {
                type CurrentTarget = E;
            }
        )*
    };
}

impl_event_param_stub!(
    Event,
    MouseEvent,
    PointerEvent,
    TouchEvent,
    KeyboardEvent,
    InputEvent,
    SubmitEvent,
    ChangeEvent,
    ClipboardEvent,
    WheelEvent,
    CompositionEvent,
    DragEvent,
    ToggleEvent,
    AnimationEvent,
    TransitionEvent,
    PopStateEvent,
    HashChangeEvent,
    StorageEvent,
);

macro_rules! impl_event_base {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Return the statically typed DOM target on which this RSJS
            /// handler was registered.
            pub fn current_target(&self) -> CurrentTarget<E> {
                CurrentTarget::__compiler_stub()
            }

            /// Prevent the browser's default action for the event.
            pub fn prevent_default(&self) {}

            /// Stop the event from bubbling further through the DOM.
            pub fn stop_propagation(&self) {}

            /// DOM event type, or an empty string when absent.
            pub fn type_(&self) -> String {
                String::new()
            }

            /// DOM event type, or an empty string when absent.
            pub fn event_type(&self) -> String {
                String::new()
            }

            /// Event target value, or an empty string when the target has no
            /// value property.
            pub fn value(&self) -> String {
                String::new()
            }

            /// Event target checked state.
            pub fn checked(&self) -> bool {
                false
            }

            /// Event target value, or an empty string when the target has no
            /// value property.
            pub fn target_value(&self) -> String {
                String::new()
            }

            /// Event target checked state.
            pub fn target_checked(&self) -> bool {
                false
            }

            /// Closest event-target `data-date` value, or an empty string when absent.
            pub fn target_date(&self) -> String {
                String::new()
            }

            /// Closest event-target `data-date` value, or an empty string when absent.
            pub fn date(&self) -> String {
                String::new()
            }

            /// Event target element id, or an empty string when absent.
            pub fn target_id(&self) -> String {
                String::new()
            }

            /// Direct event target attribute value, or an empty string when absent.
            pub fn target_attr(&self, _name: &'static str) -> String {
                String::new()
            }

            /// Direct event target attribute value, or an empty string when absent.
            pub fn target_attribute(&self, _name: &'static str) -> String {
                String::new()
            }

            /// Closest event target ancestor attribute value, or an empty string when absent.
            pub fn closest_attr(&self, _name: &'static str) -> String {
                String::new()
            }

            /// Closest event target ancestor attribute value, or an empty string when absent.
            pub fn closest_attribute(&self, _name: &'static str) -> String {
                String::new()
            }

            /// Whether this event's target is inside the bound node ref.
            pub fn target_inside<T>(&self, _node: NodeRef<T>) -> bool {
                false
            }

            /// Whether this event's target is outside the bound node ref.
            pub fn target_outside<T>(&self, _node: NodeRef<T>) -> bool {
                false
            }

            /// Whether the Shift key was pressed.
            pub fn shift_key(&self) -> bool {
                false
            }

            /// Whether the Meta key was pressed.
            pub fn meta_key(&self) -> bool {
                false
            }

            /// Whether the Ctrl key was pressed.
            pub fn ctrl_key(&self) -> bool {
                false
            }

            /// Whether the Alt key was pressed.
            pub fn alt_key(&self) -> bool {
                false
            }
        }
    };
}

macro_rules! impl_current_target_element_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::HasElementApi> $ty<E> {
            /// Matched rsjs handler element id, or an empty string when absent.
            pub fn current_target_id(&self) -> String {
                String::new()
            }

            /// Matched rsjs handler element `scrollTop`, or zero when absent.
            pub fn current_target_scroll_top(&self) -> f64 {
                0.0
            }

            /// Matched rsjs handler element `scrollLeft`, or zero when absent.
            pub fn current_target_scroll_left(&self) -> f64 {
                0.0
            }

            /// Matched rsjs handler element attribute value, or an empty string when absent.
            pub fn current_target_attr(&self, _name: &'static str) -> String {
                String::new()
            }

            /// Matched rsjs handler element attribute value, or an empty string when absent.
            pub fn current_target_attribute(&self, _name: &'static str) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_current_target_value_event {
    ($ty:ident) => {
        impl<E: elem::ValueElementMarker> $ty<E> {
            /// Matched handler element value, or an empty string when absent.
            pub fn current_value(&self) -> String {
                String::new()
            }

            /// Matched handler element value, or an empty string when absent.
            pub fn current_target_value(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_current_target_checked_event {
    ($ty:ident) => {
        impl<E: elem::CheckedElementMarker> $ty<E> {
            /// Matched handler element checked state.
            pub fn current_checked(&self) -> bool {
                false
            }

            /// Matched handler element checked state.
            pub fn current_target_checked(&self) -> bool {
                false
            }
        }
    };
}

macro_rules! impl_keyboard_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Keyboard event key, or an empty string for non-keyboard events.
            pub fn key(&self) -> String {
                String::new()
            }

            /// Keyboard event physical key code, or an empty string when absent.
            pub fn code(&self) -> String {
                String::new()
            }

            /// Whether this is an auto-repeat key event.
            pub fn repeat(&self) -> bool {
                false
            }

            /// Whether Command (Meta) or Ctrl is pressed and the event key
            /// matches `key`, case-insensitively.
            pub fn is_cmd_or_ctrl(&self, _key: &'static str) -> bool {
                false
            }
        }
    };
}

macro_rules! impl_mouse_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            pub fn client_x(&self) -> f64 {
                0.0
            }

            pub fn client_y(&self) -> f64 {
                0.0
            }

            pub fn page_x(&self) -> f64 {
                0.0
            }

            pub fn page_y(&self) -> f64 {
                0.0
            }

            pub fn screen_x(&self) -> f64 {
                0.0
            }

            pub fn screen_y(&self) -> f64 {
                0.0
            }

            pub fn offset_x(&self) -> f64 {
                0.0
            }

            pub fn offset_y(&self) -> f64 {
                0.0
            }

            pub fn movement_x(&self) -> f64 {
                0.0
            }

            pub fn movement_y(&self) -> f64 {
                0.0
            }

            pub fn button(&self) -> i32 {
                0
            }

            pub fn buttons(&self) -> u32 {
                0
            }

            pub fn detail(&self) -> i32 {
                0
            }

            /// Synthetic event detail interpreted with Rust's exact
            /// `i32 as u32` semantics.
            pub fn detail_u32(&self) -> u32 {
                0
            }
        }
    };
}

macro_rules! impl_wheel_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            pub fn delta_x(&self) -> f64 {
                0.0
            }

            pub fn delta_y(&self) -> f64 {
                0.0
            }

            pub fn delta_z(&self) -> f64 {
                0.0
            }

            pub fn delta_mode(&self) -> u32 {
                0
            }
        }
    };
}

macro_rules! impl_pointer_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            pub fn pointer_id(&self) -> i32 {
                0
            }

            pub fn pointer_type(&self) -> String {
                String::new()
            }

            pub fn is_primary(&self) -> bool {
                false
            }

            pub fn pressure(&self) -> f64 {
                0.0
            }

            pub fn tangential_pressure(&self) -> f64 {
                0.0
            }

            pub fn tilt_x(&self) -> i32 {
                0
            }

            pub fn tilt_y(&self) -> i32 {
                0
            }

            pub fn twist(&self) -> u32 {
                0
            }

            pub fn pointer_width(&self) -> f64 {
                0.0
            }

            pub fn pointer_height(&self) -> f64 {
                0.0
            }
        }
    };
}

macro_rules! impl_touch_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Number of changed touch points.
            pub fn changed_touch_count(&self) -> u32 {
                0
            }

            /// Number of target touch points.
            pub fn target_touch_count(&self) -> u32 {
                0
            }

            /// Number of active touch points.
            pub fn touch_count(&self) -> u32 {
                0
            }

            /// Identifier for the first changed touch point, or zero when absent.
            pub fn changed_touch_identifier(&self) -> i32 {
                0
            }

            /// `clientX` for the first changed touch point, or zero when absent.
            pub fn changed_touch_client_x(&self) -> f64 {
                0.0
            }

            /// `clientY` for the first changed touch point, or zero when absent.
            pub fn changed_touch_client_y(&self) -> f64 {
                0.0
            }

            /// `pageX` for the first changed touch point, or zero when absent.
            pub fn changed_touch_page_x(&self) -> f64 {
                0.0
            }

            /// `pageY` for the first changed touch point, or zero when absent.
            pub fn changed_touch_page_y(&self) -> f64 {
                0.0
            }

            /// `screenX` for the first changed touch point, or zero when absent.
            pub fn changed_touch_screen_x(&self) -> f64 {
                0.0
            }

            /// `screenY` for the first changed touch point, or zero when absent.
            pub fn changed_touch_screen_y(&self) -> f64 {
                0.0
            }

            /// Identifier for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_identifier_at<I>(&self, _index: I) -> i32 {
                0
            }

            /// `clientX` for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_client_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `clientY` for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_client_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `pageX` for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_page_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `pageY` for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_page_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `screenX` for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_screen_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `screenY` for the changed touch point at `index`, or zero when absent.
            pub fn changed_touch_screen_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// Identifier for the target touch point at `index`, or zero when absent.
            pub fn target_touch_identifier_at<I>(&self, _index: I) -> i32 {
                0
            }

            /// `clientX` for the target touch point at `index`, or zero when absent.
            pub fn target_touch_client_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `clientY` for the target touch point at `index`, or zero when absent.
            pub fn target_touch_client_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `pageX` for the target touch point at `index`, or zero when absent.
            pub fn target_touch_page_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `pageY` for the target touch point at `index`, or zero when absent.
            pub fn target_touch_page_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `screenX` for the target touch point at `index`, or zero when absent.
            pub fn target_touch_screen_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `screenY` for the target touch point at `index`, or zero when absent.
            pub fn target_touch_screen_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// Identifier for the active touch point at `index`, or zero when absent.
            pub fn touch_identifier_at<I>(&self, _index: I) -> i32 {
                0
            }

            /// `clientX` for the active touch point at `index`, or zero when absent.
            pub fn touch_client_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `clientY` for the active touch point at `index`, or zero when absent.
            pub fn touch_client_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `pageX` for the active touch point at `index`, or zero when absent.
            pub fn touch_page_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `pageY` for the active touch point at `index`, or zero when absent.
            pub fn touch_page_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `screenX` for the active touch point at `index`, or zero when absent.
            pub fn touch_screen_x_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// `screenY` for the active touch point at `index`, or zero when absent.
            pub fn touch_screen_y_at<I>(&self, _index: I) -> f64 {
                0.0
            }
        }
    };
}

macro_rules! impl_input_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Input event type, e.g. `insertText`, or an empty string when absent.
            pub fn input_type(&self) -> String {
                String::new()
            }

            /// Input event inserted data, or an empty string when absent.
            pub fn data(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_toggle_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Previous toggle state, usually `"open"` or `"closed"`.
            pub fn old_state(&self) -> String {
                String::new()
            }

            /// New toggle state, usually `"open"` or `"closed"`.
            pub fn new_state(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_animation_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// CSS animation name, or an empty string when absent.
            pub fn animation_name(&self) -> String {
                String::new()
            }

            /// Elapsed animation time in seconds, excluding paused time.
            pub fn elapsed_time(&self) -> f64 {
                0.0
            }

            /// Pseudo-element the animation runs on, or an empty string.
            pub fn pseudo_element(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_transition_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// CSS property name associated with the transition.
            pub fn property_name(&self) -> String {
                String::new()
            }

            /// Elapsed transition time in seconds, excluding delay.
            pub fn elapsed_time(&self) -> f64 {
                0.0
            }

            /// Pseudo-element the transition runs on, or an empty string.
            pub fn pseudo_element(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_storage_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Storage key that changed, or an empty string for `clear()`.
            pub fn storage_key(&self) -> String {
                String::new()
            }

            /// Previous string value, or an empty string when absent.
            pub fn storage_old_value(&self) -> String {
                String::new()
            }

            /// New string value, or an empty string when absent.
            pub fn storage_new_value(&self) -> String {
                String::new()
            }

            /// URL of the document that changed storage, or an empty string when absent.
            pub fn storage_url(&self) -> String {
                String::new()
            }

            /// `"local"`, `"session"`, or an empty string when the storage area is unknown.
            pub fn storage_area(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_hash_change_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Previous URL, or an empty string when absent.
            pub fn old_url(&self) -> String {
                String::new()
            }

            /// New URL, or an empty string when absent.
            pub fn new_url(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_clipboard_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Plain text from the clipboard payload, or an empty string when absent.
            pub fn clipboard_text(&self) -> String {
                String::new()
            }
        }
    };
}

macro_rules! impl_data_transfer_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Plain text drag/drop payload, or an empty string when absent.
            pub fn data_transfer_text(&self) -> String {
                String::new()
            }

            /// Number of files in the drag/drop payload.
            pub fn data_transfer_file_count(&self) -> u32 {
                0
            }

            /// Number of drag/drop data-transfer items.
            pub fn data_transfer_item_count(&self) -> u32 {
                0
            }

            /// First dragged/dropped file name, or an empty string when absent.
            pub fn data_transfer_first_file_name(&self) -> String {
                String::new()
            }

            /// First dragged/dropped file MIME type, or an empty string when absent.
            pub fn data_transfer_first_file_type(&self) -> String {
                String::new()
            }

            /// First dragged/dropped file size in bytes, or zero when absent.
            pub fn data_transfer_first_file_size(&self) -> f64 {
                0.0
            }

            /// Dragged/dropped file name at `index`, or an empty string when absent.
            pub fn data_transfer_file_name_at<I>(&self, _index: I) -> String {
                String::new()
            }

            /// Dragged/dropped file MIME type at `index`, or an empty string when absent.
            pub fn data_transfer_file_type_at<I>(&self, _index: I) -> String {
                String::new()
            }

            /// Dragged/dropped file size at `index`, or zero when absent.
            pub fn data_transfer_file_size_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// Data-transfer item kind at `index`, or an empty string when absent.
            pub fn data_transfer_item_kind_at<I>(&self, _index: I) -> String {
                String::new()
            }

            /// Data-transfer item MIME type at `index`, or an empty string when absent.
            pub fn data_transfer_item_type_at<I>(&self, _index: I) -> String {
                String::new()
            }

            /// Read the first dragged/dropped file as text into a signal.
            pub fn data_transfer_first_file_text_into<T>(&self, _target: &crate::Signal<T>) {}

            /// Read the first dragged/dropped file as text into a signal, or use a fallback.
            pub fn data_transfer_first_file_text_into_or<T, F>(
                &self,
                _target: &crate::Signal<T>,
                _fallback: F,
            ) {
            }
        }
    };
}

macro_rules! impl_file_target_event {
    ($ty:ident) => {
        impl<E: rsjs_web::dom::DomTargetMarker> $ty<E> {
            /// Number of files selected on the event target.
            pub fn target_file_count(&self) -> u32 {
                0
            }

            /// First selected file name, or an empty string when absent.
            pub fn target_first_file_name(&self) -> String {
                String::new()
            }

            /// First selected file MIME type, or an empty string when absent.
            pub fn target_first_file_type(&self) -> String {
                String::new()
            }

            /// First selected file size in bytes, or zero when absent.
            pub fn target_first_file_size(&self) -> f64 {
                0.0
            }

            /// Selected file name at `index`, or an empty string when absent.
            pub fn target_file_name_at<I>(&self, _index: I) -> String {
                String::new()
            }

            /// Selected file MIME type at `index`, or an empty string when absent.
            pub fn target_file_type_at<I>(&self, _index: I) -> String {
                String::new()
            }

            /// Selected file size at `index`, or zero when absent.
            pub fn target_file_size_at<I>(&self, _index: I) -> f64 {
                0.0
            }

            /// Read the first selected file as text into a signal.
            pub fn target_first_file_text_into<T>(&self, _target: &crate::Signal<T>) {}

            /// Read the first selected file as text into a signal, or use a fallback.
            pub fn target_first_file_text_into_or<T, F>(
                &self,
                _target: &crate::Signal<T>,
                _fallback: F,
            ) {
            }
        }
    };
}

impl_event_base!(Event);
impl_event_base!(MouseEvent);
impl_event_base!(PointerEvent);
impl_event_base!(TouchEvent);
impl_event_base!(KeyboardEvent);
impl_event_base!(InputEvent);
impl_event_base!(SubmitEvent);
impl_event_base!(ChangeEvent);
impl_event_base!(ClipboardEvent);
impl_event_base!(WheelEvent);
impl_event_base!(CompositionEvent);
impl_event_base!(DragEvent);
impl_event_base!(ToggleEvent);
impl_event_base!(AnimationEvent);
impl_event_base!(TransitionEvent);
impl_event_base!(PopStateEvent);
impl_event_base!(HashChangeEvent);
impl_event_base!(StorageEvent);

impl_current_target_element_event!(Event);
impl_current_target_element_event!(MouseEvent);
impl_current_target_element_event!(PointerEvent);
impl_current_target_element_event!(TouchEvent);
impl_current_target_element_event!(KeyboardEvent);
impl_current_target_element_event!(InputEvent);
impl_current_target_element_event!(SubmitEvent);
impl_current_target_element_event!(ChangeEvent);
impl_current_target_element_event!(ClipboardEvent);
impl_current_target_element_event!(WheelEvent);
impl_current_target_element_event!(CompositionEvent);
impl_current_target_element_event!(DragEvent);
impl_current_target_element_event!(ToggleEvent);
impl_current_target_element_event!(AnimationEvent);
impl_current_target_element_event!(TransitionEvent);
impl_current_target_element_event!(PopStateEvent);
impl_current_target_element_event!(HashChangeEvent);
impl_current_target_element_event!(StorageEvent);

impl_current_target_value_event!(Event);
impl_current_target_value_event!(MouseEvent);
impl_current_target_value_event!(PointerEvent);
impl_current_target_value_event!(TouchEvent);
impl_current_target_value_event!(KeyboardEvent);
impl_current_target_value_event!(InputEvent);
impl_current_target_value_event!(SubmitEvent);
impl_current_target_value_event!(ChangeEvent);
impl_current_target_value_event!(ClipboardEvent);
impl_current_target_value_event!(WheelEvent);
impl_current_target_value_event!(CompositionEvent);
impl_current_target_value_event!(DragEvent);
impl_current_target_value_event!(ToggleEvent);
impl_current_target_value_event!(AnimationEvent);
impl_current_target_value_event!(TransitionEvent);
impl_current_target_value_event!(PopStateEvent);
impl_current_target_value_event!(HashChangeEvent);
impl_current_target_value_event!(StorageEvent);

impl_current_target_checked_event!(Event);
impl_current_target_checked_event!(MouseEvent);
impl_current_target_checked_event!(PointerEvent);
impl_current_target_checked_event!(TouchEvent);
impl_current_target_checked_event!(KeyboardEvent);
impl_current_target_checked_event!(InputEvent);
impl_current_target_checked_event!(SubmitEvent);
impl_current_target_checked_event!(ChangeEvent);
impl_current_target_checked_event!(ClipboardEvent);
impl_current_target_checked_event!(WheelEvent);
impl_current_target_checked_event!(CompositionEvent);
impl_current_target_checked_event!(DragEvent);
impl_current_target_checked_event!(ToggleEvent);
impl_current_target_checked_event!(AnimationEvent);
impl_current_target_checked_event!(TransitionEvent);
impl_current_target_checked_event!(PopStateEvent);
impl_current_target_checked_event!(HashChangeEvent);
impl_current_target_checked_event!(StorageEvent);

impl_keyboard_event!(Event);
impl_keyboard_event!(KeyboardEvent);
impl_mouse_event!(Event);
impl_mouse_event!(MouseEvent);
impl_mouse_event!(PointerEvent);
impl_mouse_event!(DragEvent);
impl_pointer_event!(Event);
impl_pointer_event!(PointerEvent);
impl_wheel_event!(Event);
impl_wheel_event!(WheelEvent);
impl_touch_event!(Event);
impl_touch_event!(TouchEvent);
impl_input_event!(Event);
impl_input_event!(InputEvent);
impl_input_event!(CompositionEvent);
impl_toggle_event!(Event);
impl_toggle_event!(ToggleEvent);
impl_animation_event!(AnimationEvent);
impl_transition_event!(TransitionEvent);
impl<E: rsjs_web::dom::DomTargetMarker> Event<E> {
    /// CSS animation name, or an empty string when absent.
    pub fn animation_name(&self) -> String {
        String::new()
    }

    /// CSS property name associated with a transition, or an empty string.
    pub fn property_name(&self) -> String {
        String::new()
    }

    /// Elapsed animation/transition time in seconds.
    pub fn elapsed_time(&self) -> f64 {
        0.0
    }

    /// Pseudo-element the animation/transition runs on, or an empty string.
    pub fn pseudo_element(&self) -> String {
        String::new()
    }
}
impl_storage_event!(Event);
impl_storage_event!(StorageEvent);
impl_hash_change_event!(Event);
impl_hash_change_event!(HashChangeEvent);
impl_clipboard_event!(Event);
impl_clipboard_event!(ClipboardEvent);
impl_data_transfer_event!(Event);
impl_data_transfer_event!(DragEvent);
impl_file_target_event!(Event);
impl_file_target_event!(ChangeEvent);

/// Built-in client-side console helpers recognised inside RSJS-analyzed
/// handler bodies. The real calls are emitted into the island JS; the
/// Rust functions are no-op stubs for IDE resolution.
pub mod console {
    pub fn log<T>(_value: T) {}
    pub fn info<T>(_value: T) {}
    pub fn debug<T>(_value: T) {}
    pub fn warn<T>(_value: T) {}
    pub fn error<T>(_value: T) {}
    pub fn table<T>(_value: T) {}
    pub fn time<T>(_label: T) {}
    pub fn time_end<T>(_label: T) {}
    pub fn group<T>(_label: T) {}
    pub fn group_end() {}
}

/// Built-in client-side window helpers recognised inside RSJS-analyzed
/// handler bodies.
pub mod window {
    /// Read `window.innerWidth`, or zero during SSR.
    pub fn inner_width() -> i32 {
        0
    }

    /// Read `window.innerHeight`, or zero during SSR.
    pub fn inner_height() -> i32 {
        0
    }

    /// Read `window.scrollX`, or zero during SSR.
    pub fn scroll_x() -> f64 {
        0.0
    }

    /// Read `window.scrollY`, or zero during SSR.
    pub fn scroll_y() -> f64 {
        0.0
    }

    /// Call `window.scrollTo(x, y)` when available.
    pub fn scroll_to<X, Y>(_x: X, _y: Y) {}

    /// Call `window.scrollBy(x, y)` when available.
    pub fn scroll_by<X, Y>(_x: X, _y: Y) {}

    /// Call `window.print()` when available.
    pub fn print() {}
}

/// Built-in client-side navigator helpers recognised inside RSJS-analyzed
/// island expressions and handler bodies.
pub mod navigator {
    /// Read `navigator.language` (the user's preferred UI language as
    /// a BCP 47 tag, e.g. `"en-US"`), or an empty string during SSR.
    pub fn language() -> String {
        String::new()
    }
}

/// Built-in client-side location helpers recognised inside RSJS-analyzed
/// island expressions and handler bodies.
pub mod location {
    /// Read `window.location.href`, or an empty string during SSR.
    pub fn href() -> String {
        String::new()
    }

    /// Read `window.location.origin`, or an empty string during SSR.
    pub fn origin() -> String {
        String::new()
    }

    /// Read `window.location.protocol`, or an empty string during SSR.
    pub fn protocol() -> String {
        String::new()
    }

    /// Read `window.location.host`, or an empty string during SSR.
    pub fn host() -> String {
        String::new()
    }

    /// Read `window.location.hostname`, or an empty string during SSR.
    pub fn hostname() -> String {
        String::new()
    }

    /// Read `window.location.port`, or an empty string during SSR.
    pub fn port() -> String {
        String::new()
    }

    /// Read `window.location.pathname`, or an empty string during SSR.
    pub fn pathname() -> String {
        String::new()
    }

    /// Read `window.location.search`, or an empty string during SSR.
    pub fn search() -> String {
        String::new()
    }

    /// Read `window.location.hash`, or an empty string during SSR.
    pub fn hash() -> String {
        String::new()
    }

    /// Set `window.location.hash`.
    pub fn set_hash<T>(_value: T) {}

    /// Call `window.location.assign(url)`.
    pub fn assign<T>(_url: T) {}

    /// Call `window.location.replace(url)`.
    pub fn replace<T>(_url: T) {}

    /// Call `window.location.reload()`.
    pub fn reload() {}
}

/// Built-in client-side history helpers recognised inside RSJS-analyzed
/// handler bodies.
pub mod history {
    /// Call `window.history.pushState(null, "", url)`.
    pub fn push_state<T>(_url: T) {}

    /// Call `window.history.replaceState(null, "", url)`.
    pub fn replace_state<T>(_url: T) {}

    /// Call `window.history.back()`.
    pub fn back() {}

    /// Call `window.history.forward()`.
    pub fn forward() {}

    /// Call `window.history.go(delta)`.
    pub fn go<T>(_delta: T) {}
}

/// Built-in client-side URL/query-string helpers recognised inside
/// RSJS-analyzed expressions and handler bodies.
pub mod url {
    /// Read one value from the current page query string, returning an
    /// empty string when the key is missing or URLSearchParams is
    /// unavailable.
    pub fn search_param<K>(_key: K) -> String {
        String::new()
    }

    /// Read one value from the current page query string, returning
    /// `fallback` when the key is missing or URLSearchParams is
    /// unavailable.
    pub fn search_param_or<K, F>(_key: K, fallback: F) -> String
    where
        F: Into<String>,
    {
        fallback.into()
    }

    /// Set one query-string key on the current URL and replace the
    /// current history entry.
    pub fn set_search_param<K, V>(_key: K, _value: V) {}

    /// Set one query-string key on the current URL and push a new
    /// history entry.
    pub fn push_search_param<K, V>(_key: K, _value: V) {}

    /// Remove one query-string key from the current URL and replace the
    /// current history entry.
    pub fn delete_search_param<K>(_key: K) {}

    /// Remove one query-string key from the current URL and push a new
    /// history entry.
    pub fn push_delete_search_param<K>(_key: K) {}
}

/// Built-in client-side document helpers recognised inside RSJS-analyzed
/// handler bodies.
/// The document root element (`document.documentElement`, i.e. `<html>`).
///
/// Page-wide presentation state lives here: the theme class and anything
/// keyed off it. An island cannot reach the root with a `node_ref`, because
/// `ref=` only binds elements the island itself renders and `<html>` belongs
/// to the document shell.
///
/// These are server-side stubs. Reads render as `false` during SSR; effects
/// are no-ops. The real behavior is the JS the analyzer emits from the
/// matching `rsjs_ir::browser_registry` entry.
pub mod root {
    /// Whether `document.documentElement` currently carries `class`.
    ///
    /// Renders `false` during SSR, so seed island state from it only where a
    /// server-side `false` is the correct initial paint.
    pub fn has_class<C>(_class: C) -> bool {
        false
    }

    /// Add or remove `class` on `document.documentElement`.
    ///
    /// Mirrors `classList.toggle(class, on)`: `on == true` adds, `false`
    /// removes.
    pub fn set_class<C>(_class: C, _on: bool) {}
}

pub mod document {
    /// Dispatch `new CustomEvent(name)` on `document`.
    pub fn dispatch_event<T>(_name: T) {}

    /// Dispatch `new CustomEvent(name, { detail })` on `document`.
    pub fn dispatch_event_with_detail<N, D>(_name: N, _detail: D) {}

    /// Set `document.title`.
    pub fn set_title<T>(_title: T) {}
}

/// Built-in client-side clipboard helpers recognised inside RSJS-analyzed
/// handler bodies.
pub mod clipboard {
    /// Call `navigator.clipboard.writeText(text)` when available.
    pub fn write_text<T>(_text: T) {}

    /// Read `navigator.clipboard.readText()` and write the result into
    /// the target signal. Missing APIs, denied permissions, and read
    /// failures write an empty string.
    pub fn read_text_into<T>(_target: &crate::Signal<T>) {}

    /// Read `navigator.clipboard.readText()` and write the result into
    /// the target signal. Missing APIs, denied permissions, and read
    /// failures write `fallback`.
    pub fn read_text_into_or<T, F>(_target: &crate::Signal<T>, _fallback: F) {}
}

/// Built-in WebAuthn/Credential Management helpers recognised inside
/// RSJS-analyzed handler bodies.
///
/// These helpers expect server-provided JSON strings using the WebAuthn
/// Level 3 JSON representation: binary fields such as `challenge` and
/// credential ids are base64url strings. Generated JS parses the JSON
/// with `PublicKeyCredential.parseCreationOptionsFromJSON` or
/// `parseRequestOptionsFromJSON`, calls `navigator.credentials`, and
/// writes `JSON.stringify(credential)` into the target signal on
/// success.
pub mod webauthn {
    /// Start a WebAuthn registration ceremony and write the resulting
    /// credential JSON into `target`. Unsupported browsers, parse
    /// errors, user cancellation, and credential failures write an empty
    /// string.
    pub fn create_json_into<O, T>(_options_json: O, _target: &crate::Signal<T>) {}

    /// Start a WebAuthn registration ceremony and write the resulting
    /// credential JSON into `target`. Unsupported browsers, parse
    /// errors, user cancellation, and credential failures write
    /// `fallback`.
    pub fn create_json_into_or<O, T, F>(
        _options_json: O,
        _target: &crate::Signal<T>,
        _fallback: F,
    ) {
    }

    /// Start a WebAuthn authentication ceremony and write the resulting
    /// credential JSON into `target`. Unsupported browsers, parse
    /// errors, user cancellation, and credential failures write an empty
    /// string.
    pub fn get_json_into<O, T>(_options_json: O, _target: &crate::Signal<T>) {}

    /// Start a WebAuthn authentication ceremony and write the resulting
    /// credential JSON into `target`. Unsupported browsers, parse
    /// errors, user cancellation, and credential failures write
    /// `fallback`.
    pub fn get_json_into_or<O, T, F>(_options_json: O, _target: &crate::Signal<T>, _fallback: F) {}
}

/// Byte buffer marker used by `rsjs::encoding::*` IDE stubs.
///
/// In generated browser code this corresponds to a `Uint8Array`. On the
/// server side the helper functions below use this concrete wrapper so
/// SSR initial values can mirror simple string/byte transforms when the
/// input is statically known.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bytes(Vec<u8>);

impl Bytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl<const N: usize> From<[u8; N]> for Bytes {
    fn from(value: [u8; N]) -> Self {
        Self(value.to_vec())
    }
}

impl From<&[u8]> for Bytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

/// Built-in browser encoding helpers recognised inside RSJS-analyzed
/// island expressions and handler bodies.
pub mod encoding {
    use base64::Engine;

    use super::Bytes;

    /// Encode UTF-8 text to bytes (`TextEncoder.encode(...)` in JS).
    pub fn utf8_bytes<T>(text: T) -> Bytes
    where
        T: AsRef<str>,
    {
        Bytes::new(text.as_ref().as_bytes().to_vec())
    }

    /// Decode UTF-8 bytes to text (`TextDecoder.decode(...)` in JS).
    pub fn utf8_string<B>(bytes: B) -> String
    where
        B: AsRef<[u8]>,
    {
        String::from_utf8_lossy(bytes.as_ref()).into_owned()
    }

    /// Encode bytes to a padded base64 string.
    pub fn base64_encode<B>(bytes: B) -> String
    where
        B: AsRef<[u8]>,
    {
        base64::engine::general_purpose::STANDARD.encode(bytes.as_ref())
    }

    /// Decode a padded base64 string to bytes. Invalid input returns an
    /// empty byte buffer, matching the generated JS helper's no-throw
    /// behavior.
    pub fn base64_decode<T>(text: T) -> Bytes
    where
        T: AsRef<str>,
    {
        base64::engine::general_purpose::STANDARD
            .decode(text.as_ref())
            .map(Bytes::new)
            .unwrap_or_default()
    }

    /// Encode bytes to an unpadded base64url string.
    pub fn base64url_encode<B>(bytes: B) -> String
    where
        B: AsRef<[u8]>,
    {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes.as_ref())
    }

    /// Decode a base64url string to bytes. Both padded and unpadded
    /// input are accepted; invalid input returns an empty byte buffer.
    pub fn base64url_decode<T>(text: T) -> Bytes
    where
        T: AsRef<str>,
    {
        let text = text.as_ref();
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(text)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(text))
            .map(Bytes::new)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod encoding_tests {
    use super::encoding;

    #[test]
    fn utf8_and_base64url_round_trip() {
        let bytes = encoding::utf8_bytes("hello");
        let encoded = encoding::base64url_encode(bytes);
        assert_eq!(encoded, "aGVsbG8");

        let decoded = encoding::base64url_decode(encoded);
        assert_eq!(encoding::utf8_string(decoded), "hello");
    }

    #[test]
    fn invalid_base64_returns_empty_bytes() {
        assert_eq!(encoding::base64_decode("not valid !!!").as_ref(), b"");
        assert_eq!(encoding::base64url_decode("not valid !!!").as_ref(), b"");
    }
}

/// Built-in client-side localStorage helpers recognised inside RSJS-analyzed
/// island expressions and handler bodies.
pub mod local_storage {
    /// Read `window.localStorage.getItem(key)`, returning an empty
    /// string when the key is missing or storage is unavailable.
    pub fn get_item<K>(_key: K) -> String {
        String::new()
    }

    /// Read `window.localStorage.getItem(key)`, returning `fallback`
    /// when the key is missing or storage is unavailable.
    pub fn get_item_or<K, F>(_key: K, fallback: F) -> String
    where
        F: Into<String>,
    {
        fallback.into()
    }

    /// Read and JSON-parse a boolean localStorage value, returning `fallback`
    /// when the key is missing, storage is unavailable, or parsing fails.
    pub fn get_bool_or<K>(_key: K, fallback: bool) -> bool {
        fallback
    }

    /// Read a canonical base-10 `i64` from localStorage.
    ///
    /// The client result and fallback are BigInt-backed across the full `i64`
    /// range. Returns `fallback` when the key is missing, storage is
    /// unavailable, or the stored text is non-canonical or out of range.
    pub fn get_i64_or<K>(_key: K, fallback: i64) -> i64 {
        fallback
    }

    /// Read and JSON-parse a finite-number localStorage value, returning
    /// `fallback` when the key is missing, storage is unavailable, or parsing fails.
    pub fn get_f64_or<K>(_key: K, fallback: f64) -> f64 {
        fallback
    }

    /// Call `window.localStorage.setItem(key, value)` when available.
    pub fn set_item<K, V>(_key: K, _value: V) {}

    /// Call `window.localStorage.removeItem(key)` when available.
    pub fn remove_item<K>(_key: K) {}
}

/// Built-in client-side sessionStorage helpers recognised inside RSJS-analyzed
/// island expressions and handler bodies.
pub mod session_storage {
    /// Read `window.sessionStorage.getItem(key)`, returning an empty
    /// string when the key is missing or storage is unavailable.
    pub fn get_item<K>(_key: K) -> String {
        String::new()
    }

    /// Read `window.sessionStorage.getItem(key)`, returning `fallback`
    /// when the key is missing or storage is unavailable.
    pub fn get_item_or<K, F>(_key: K, fallback: F) -> String
    where
        F: Into<String>,
    {
        fallback.into()
    }

    /// Read and JSON-parse a boolean sessionStorage value, returning `fallback`
    /// when the key is missing, storage is unavailable, or parsing fails.
    pub fn get_bool_or<K>(_key: K, fallback: bool) -> bool {
        fallback
    }

    /// Read a canonical base-10 `i64` from sessionStorage.
    ///
    /// The client result and fallback are BigInt-backed across the full `i64`
    /// range. Returns `fallback` when the key is missing, storage is
    /// unavailable, or the stored text is non-canonical or out of range.
    pub fn get_i64_or<K>(_key: K, fallback: i64) -> i64 {
        fallback
    }

    /// Read and JSON-parse a finite-number sessionStorage value, returning
    /// `fallback` when the key is missing, storage is unavailable, or parsing fails.
    pub fn get_f64_or<K>(_key: K, fallback: f64) -> f64 {
        fallback
    }

    /// Call `window.sessionStorage.setItem(key, value)` when available.
    pub fn set_item<K, V>(_key: K, _value: V) {}

    /// Call `window.sessionStorage.removeItem(key)` when available.
    pub fn remove_item<K>(_key: K) {}
}

/// Stub methods exposed so rust-analyzer (and other IDE tooling that does
/// not expand the `#[rsjs(component...)]` proc macro) can resolve `.get()` /
/// `.set()` / `.update()` calls on `Signal<T>`. Without these, go-to-definition
/// and hover-docs on signal call sites inside RSJS-analyzed blocks yield nothing
/// because the rsjs compiler rewrites those calls at expansion time and
/// no `impl Signal<T>` exists in the published crate.
///
/// At runtime on the server these methods are no-ops (or return the
/// captured initial value); the reactive behaviour lives in the
/// generated client JS + `rsjs-runtime.js`. The proc macro intercepts
/// the call sites before they reach these definitions, so the bodies
/// here only affect documentation, IDE navigation, and any (rare)
/// code that calls `Signal` methods outside an RSJS-analyzed block.
impl<T> Signal<T> {
    /// Read the current value. Inside an RSJS-analyzed block this subscribes
    /// the surrounding reactive scope; on the server it returns the
    /// initial value the signal was constructed with.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self._value.clone()
    }

    /// Return whether the current value is non-empty. Inside an
    /// RSJS-analyzed block this lowers to a small client-side predicate.
    pub fn is_non_empty(&self) -> bool
    where
        T: AsRef<str>,
    {
        !self._value.as_ref().is_empty()
    }

    /// Replace the value. Inside an RSJS-analyzed block this triggers a
    /// microtask-batched re-render; on the server it is a no-op.
    pub fn set(&self, _value: T) {}

    /// Apply a functional update. Inside an RSJS-analyzed block the closure
    /// is required to be a single expression (no intermediate `let`,
    /// no `if` / `match` blocks); see `docs/agent/footguns.md` for the
    /// full rule. On the server it is a no-op.
    pub fn update<F>(&self, _f: F)
    where
        F: FnOnce(T) -> T,
    {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_helper_string_semantics_match_rust() {
        use __runtime::{client_len, client_string_transform, ClientValue};

        assert_eq!(
            client_len(ClientValue::String("한😊".to_string())),
            ClientValue::I64("한😊".len() as i64),
        );
        assert_eq!(
            client_string_transform("trim", ClientValue::String("\u{0085}value\u{0085}".into())),
            ClientValue::String("value".into()),
        );
        assert_eq!(
            client_string_transform("trim", ClientValue::String("\u{feff}value\u{feff}".into())),
            ClientValue::String("\u{feff}value\u{feff}".into()),
        );
    }

    #[test]
    fn helper_internal_objects_share_mutation_across_linked_call_carriers() {
        use __runtime::{
            client_field, client_object, client_set_field, ClientArgEncode, ClientObjectValue,
            ClientReturnValue, ClientValue,
        };

        let object = client_object(vec![("open", ClientValue::Bool(false))]);
        let carrier = ClientObjectValue::from_client_value(object.clone());
        let mut called = carrier.encode_client_arg();
        client_set_field(&mut called, "open", ClientValue::Bool(true));

        assert_eq!(
            client_field(object, "open"),
            ClientValue::Bool(true),
            "a linked receiver helper must observe and mutate the caller's object identity"
        );
    }

    #[test]
    fn event_handler_authoring_stub_matches_template_call_shapes() {
        let refresh_on_click = event_handler(|_: Event| {});
        refresh_on_click();

        let mouse_on_click = event_handler(|event: MouseEvent| {
            let _ = event.client_x();
        });
        mouse_on_click();

        let typed_button_on_click = event_handler(|event: MouseEvent<elem::Button>| {
            let _ = event.client_x();
            let _ = event.current_target_id();
        });
        let typed_button_binding = typed_button_on_click();
        __private::assert_event_binding::<elem::Button, __private::event_interface::MouseEvent, _>(
            &typed_button_binding,
        );

        let typed_input_on_change = event_handler(|event: InputEvent<elem::Input>| {
            let _ = event.current_value();
            let _ = event.current_checked();
            let _ = event.current_target().value();
            let _ = event.current_target().checked();
        });
        let typed_input_binding = typed_input_on_change();
        __private::assert_event_binding::<elem::Input, __private::event_interface::InputEvent, _>(
            &typed_input_binding,
        );

        let broad_on_click = event_handler(|_: Event| {});
        __private::assert_event_binding::<elem::Button, __private::event_interface::MouseEvent, _>(
            &broad_on_click(),
        );

        let pick_on_click = event_handler(|_: Event, id: u8| {
            let _ = id;
        });
        pick_on_click(7);

        let key_on_click = event_handler(|event: KeyboardEvent, key_id: u8| {
            let _ = event.key();
            let _ = event.is_cmd_or_ctrl("k");
            let _ = key_id;
        });
        key_on_click(9);
    }
}
