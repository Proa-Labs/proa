// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! # rsjs_ir
//!
//! Side-table intermediate representation for rsjs, the reactive
//! island compiler. This crate owns:
//!
//! - Identifier types ([`SignalId`], [`EventHandlerId`], [`NodeId`],
//!   [`ExprId`], etc.) used to key the side-tables.
//! - The constrained Client IR v3 expression enum
//!   ([`client::ClientExpr`], the JS-lowerable subset of Rust
//!   expressions).
//! - Reactive declarations: `SignalDecl`, `EventHandler`,
//!   `QueryDecl`, and `MutationDecl` (M8/M9; landing alongside the
//!   side-tables).
//! - The bindings side-tables (text bindings, attribute bindings,
//!   event handler bindings, conditional sections, input bindings)
//!   keyed by NodeIds in the lifted `html!` parse tree.
//! - The `Island` struct that owns the side-tables and references
//!   the lifted parse tree.
//! - Builder constructors that frontends use to assemble an IR by
//!   hand (used in tests).
//! - Optimisation passes that rewrite the IR in place
//!   (`optimize::HandlerDedup`,
//!   `optimize::DeadSignalElimination`); see `optimize::default_passes`.
//! - The JS emitter (`emit_js`) that turns an optimised IR into a
//!   per-island JavaScript source string — landing in M2.
//! - The versioned compiler ABI model ([`abi`]) used by the staged
//!   descriptor/link pipeline.
//!
//! The Rust-`TokenStream` emitter that rewrites an authored
//! `WebRenderSync<L>` / `WebRender<L>` impl lives in
//! `rsjs_macros::emit_rust`, not here. Emitting Rust wants
//! `quote!`, spans, and macro hygiene; keeping it out of this crate
//! preserves the "no `syn` / `proc-macro2` dependency" property
//! below.
//!
//! Deliberately **independent of `syn` and `proc_macro`.** The
//! proc-macro crate (`rsjs_macros`) builds an IR from a lifted
//! `html!` parse tree using `syn`, but the IR itself doesn't carry
//! `syn` types. A future frontend (programmatic builder, JSON
//! ingestion, alternative authoring path) could build the same IR
//! without going through the macro.
//!
//! Depends on `proa_core` for the rendering-trait contracts the
//! rewritten Rust references (`WebRenderSync`, `WebRender`,
//! `WebContext`, `WriteBuf`, `WriteError`). This dependency is
//! foundational; rsjs and proa are not framework-agnostic siblings.
//! RSJS layers client analysis and artifact generation on Proa's server
//! rendering contracts.
//!
//! # Why a separate crate
//!
//! - **Testable in isolation** — unit tests construct side-tables
//!   by hand and assert the JS emitter output, no macro expansion
//!   required.
//! - **Lighter to depend on** — consumers that want to inspect or
//!   transform an IR don't pull in `syn` or `proc-macro2`.
//!
//! # Status
//!
//! v0.1 covers M1 through M9. The current API surface includes:
//!
//! - **M1**: identifier types, the constrained expression enum, signal
//!   and event-handler declarations, the [`Bindings`] side-tables,
//!   the [`Island`] container, and [`IslandBuilder`].
//! - **M2**: the JS emitter ([`emit_js::emit`]), turning an optimised
//!   IR into a per-island JavaScript source string.
//! - **M3**: the Rust emitter (lives in `rsjs_macros::emit_rust`);
//!   current production expansion preserves the authored
//!   `WebRenderSync` / `WebRender` impl header and rewrites its body.
//! - **M4-M5**: control-flow lowering. [`ConditionalSection`]s on
//!   the [`Bindings`] side-tables capture `{ if cond { ... } else
//!   { ... } }` children; input bindings ([`InputBinding`],
//!   [`InputKind`]) capture `bind_value` / `bind_checked` /
//!   `bind_selected`.
//! - **M6**: optimisation passes ([`HandlerDedup`],
//!   [`DeadSignalElimination`]) wired through [`PassRunner`] and
//!   [`default_passes`].
//! - **M7**: proc-macro frontend (`rsjs_macros::rsjs`) and manifest
//!   integration (`rsjs::manifest`).
//! - **M8**: server-driven query state ([`QueryDecl`],
//!   [`QueryOptions`], [`QueryAccessor`]) with seeded
//!   data + `data_updated_at` shape on the dehydration blob.
//! - **M9**: mutations ([`MutationDecl`], [`MutationOptions`],
//!   [`MutationAccessor`], [`InvalidationStrategy`]) with
//!   registration metadata on the dehydrated blob.
//!
//! Handler lowering, handler emission, client-module helpers, descriptor
//! handler bodies, and component-fragment callbacks now use Client IR v3 as
//! their source of truth. Remaining migration work is mostly about shrinking
//! compatibility construction inputs and broadening module-level optimization.
//! Legacy handler compatibility types and converters live under their explicit
//! `reactive` / `client::compat` modules rather than the crate-root prelude.
//! See `rsjs/STATUS.md` for current implementation coverage.

pub mod abi;
pub mod analysis;
pub mod bindings;
pub mod browser_registry;
pub mod build;
pub mod builder;
pub mod client;
pub mod dom_registry;
pub mod emit_js;
pub mod island;
pub mod link;
pub mod linked_owner_js;
pub mod lists;
pub mod markers;
pub mod optimize;
pub mod reactive;
pub mod types;
pub mod value;

pub use analysis::{
    classify_render_strategy, client_crossings, client_reachability_roots,
    client_reachability_roots_with_linked_roots, collect_client_block_external_dependencies,
    collect_client_block_helper_edges, collect_client_expr_external_dependencies,
    validate_client_helper_block, validate_client_helper_block_types, validate_client_helper_expr,
    walk_client_block, walk_client_expr, walk_client_stmt, ClientBlockDependencyError,
    ClientBlockExternalDependencies, ClientCrossing, ClientCrossingKind, ClientCrossingRequirement,
    ClientCrossingSurface, ClientHelperBodyTypeError, ClientHelperCallOccurrence,
    ClientHelperInferredType, ClientHelperTargetSignature, ClientHelperTypeSite,
    ClientHelperValidationError, ClientHelperValidationReport, ClientModuleCall,
    ClientReachabilityRoot, DeadClientPrimitive, DeadClientPrimitiveKind, RenderStrategy,
    StrategyAnalysis,
};
pub use bindings::{
    AttrBinding, AttrBindingKind, Bindings, ConditionalSection, EventInterface, EventName,
    InputBinding, InputKind, NodeRefBinding,
};
pub use builder::IslandBuilder;
pub use client::{
    client_block_json, client_expr_json, client_module_json, client_type_from_name,
    emit_client_block_js, emit_client_block_js_with_context,
    emit_client_block_js_with_context_and_indent, emit_client_expr_js,
    emit_client_expr_js_with_context, emit_client_stmt_js, emit_client_stmt_js_with_context,
    is_client_helper_runtime_intrinsic, parse_client_block_json, parse_client_block_json_str,
    parse_client_block_json_str_with_options, parse_client_block_json_with_options,
    parse_client_body_json, parse_client_body_json_str, parse_client_body_json_str_with_options,
    parse_client_body_json_with_options, parse_client_expr_json, parse_client_expr_json_str,
    parse_client_expr_json_str_with_options, parse_client_expr_json_with_options,
    parse_client_helper_type, parse_client_type_json, prune_unreachable_client_functions,
    write_client_block_json, write_client_expr_json, write_client_module_json,
    write_client_type_json, CheckedIntegerOp, ClientBinaryOp, ClientBlock, ClientBrowserEffect,
    ClientBrowserRead, ClientCapture, ClientConstant, ClientEmitContext, ClientEmitError,
    ClientExport, ClientExpr, ClientFunction, ClientFunctionBuilder, ClientFunctionId,
    ClientFunctionRef, ClientHelperTypeError, ClientHelperTypePosition, ClientImport,
    ClientJsonError, ClientJsonParseOptions, ClientLocalId, ClientModule, ClientModuleBuilder,
    ClientModuleKind, ClientNodeRefEffect, ClientObjectField, ClientParam, ClientPlace,
    ClientSimpleClosure, ClientStmt, ClientStructFieldValue, ClientType, ClientUnaryOp,
    ClientVisibility, IntegerDomain, LinkedHelperCallBinding, LinkedHelperCaller,
    LinkedHelperDefinition, LinkedHelperEmitError, LinkedHelperKey, LinkedHelperKeyError,
    LinkedHelperLinkError, LinkedHelperProgram, LinkedHelperSignaturePosition,
    CLIENT_HELPER_LEN_INTRINSIC_JS, CLIENT_VALUE_OBJECT_TYPE_PREFIX,
    UNLINKED_HELPER_IDENTIFIER_PREFIX,
};
pub use client::{
    AsType, BrowserStorageArea, EncodingOp, EventAttributeSource, EventCollection,
    EventCollectionField, EventValue, JsonPointer, LocationField, MutationAccessor,
    NodeRefAccessor, QueryAccessor, StorageValueType, StringPredicateOp, StringTransformOp,
    WindowField,
};
pub use island::{
    ComponentFragmentCallback, ComponentFragmentMount, ComponentFragmentNodeAlias,
    ComponentFragmentSignalAlias, ExprArena, HydrationStrategy, Island, PayloadAttr, ServerAttr,
    ServerOnlyNode, ServerSurfaceAnchor, ServerSurfaceSource, SourceAnchor, ViewNode, ViewTree,
    ViewTreeRef,
};
pub use linked_owner_js::{
    emit_linked_owner_module, emit_linked_owner_module_with_node_refs, LinkedOwnerCallbackEdge,
    LinkedOwnerJsError, LinkedOwnerLifecycle, LinkedOwnerLifecycleBranch, LinkedOwnerModulePlan,
    LinkedOwnerMount, LinkedOwnerNodeRefAlias, LinkedOwnerSignalAlias, LinkedOwnerSignalSeed,
    LINKED_CANDIDATE_ATTR,
};
pub use lists::{ListDecl, ListItemExpr, ListSource, ListTemplateNode};
pub use markers::{
    ISLAND_MARKER_ATTR, ISLAND_ROOT_ATTR, ISLAND_WRAPPER_TAG, MARKER_ATTR, MARKER_BIND,
    MARKER_HANDLER, MARKER_INPUT_CHECKED, MARKER_INPUT_SELECTED, MARKER_INPUT_VALUE, MARKER_LIST,
    MARKER_LIST_KEY, MARKER_REF,
};
pub use optimize::{
    default_passes, DeadSignalElimination, HandlerDedup, IslandPass, PassResult, PassRunner,
    PassStats,
};
pub use reactive::{
    ConsoleLevel, EventFileSource, EventHandler, IntervalDecl, InvalidationStrategy, MutationDecl,
    MutationOptions, NodeRefDecl, QueryDecl, QueryOptions, SharedFieldRef, SharedStoreDecl,
    SignalDecl, StorageArea, UrlHistoryMode,
};
pub use types::{
    CallbackSlotId, EventHandlerId, ExprId, LocalId, MutationId, NodeId, NodeRefId, QueryId,
    SharedStoreId, SignalId,
};
pub use value::Value;
