// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stage 2: build rsjs IR side-tables from the lifted `html!` parse tree.
//!
//! The analyser walks the [`crate::parse::ParsedIsland`] produced by
//! stage 1 and:
//!
//! - Scans `prelude_lets` for signal declarations
//!   (`let foo = signal(<lit>)` / `signal(<runtime expr>)`); registers each in an
//!   [`rsjs_ir::IslandBuilder`] and tracks a `name -> SignalId` map.
//! - Walks the HTML parse tree element-by-element, allocating a
//!   `NodeId` per `Element` and building the view tree.
//! - For each element, processes attributes:
//!   - `on_click` / `on_input` / `on_change` / `on_blur` /
//!     `on_submit` lower to event handlers (the value must be a
//!     closure expression `|event| ...` / `move |event| ...`, or a call to a
//!     local `rsjs::handler(...)` / `rsjs::event_handler(...)` value).
//!   - `bind_value` / `bind_checked` / `bind_selected` lower to
//!     [`rsjs_ir::InputBinding`] entries; the attribute value is the
//!     signal name resolved against the prelude map.
//!   - Other attributes are passed through unchanged (they are
//!     emit_rust's concern, not the rsjs analyser's).
//! - For each child:
//!   - `Text(LitStr)` becomes a static-text node.
//!   - `Element` recurses.
//!   - `Fragment` recurses, flattening children under the same
//!     parent.
//!   - `Block(Expr)`:
//!     - If the expr is `if cond { html_sync!{...} } else { ... }`,
//!       lower it as a [`rsjs_ir::ConditionalSection`] keyed to the
//!       parent NodeId. Sibling conditionals under the same parent
//!       are stored in source order.
//!     - Otherwise lower it as a text binding: a sole child may bind
//!       the parent's complete text content, while mixed content gets
//!       a dedicated binding slot so sibling order and ownership survive
//!       hydration updates.
//!
//! Output: a fully-built [`rsjs_ir::Island`] that the optimisation
//! passes (`default_passes()`) and the emitters (emit_js, emit_rust)
//! consume unchanged.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ast_expr::{BinOp, BrowserRead, FrontendExpr};
use proa_dsl::parse::{Attribute, Element, HtmlRoot, Node};
use proc_macro2::Span;
use quote::ToTokens;
use rsjs_ir::abi::HelperCallEdgeId;
use rsjs_ir::analysis::typing::{numeric_width, NumericAnchors, NumericWidth, TypeEnv};
use rsjs_ir::client::{
    CheckedIntegerOp, ClientBinaryOp, ClientBlock, ClientBrowserEffect, ClientBrowserRead,
    ClientExpr, ClientFunctionRef, ClientLocalId, ClientNodeRefEffect, ClientSimpleClosure,
    ClientStmt, ClientType, ClientUnaryOp, IntegerDomain,
};
use rsjs_ir::lists::{ListDecl, ListItemExpr, ListSource, ListTemplateNode};
use rsjs_ir::markers::{MARKER_COMPONENT, MARKER_COMPONENT_MOUNT, MARKER_LIST};
use rsjs_ir::{
    AsType, AttrBindingKind, CallbackSlotId, ComponentFragmentCallback, ComponentFragmentNodeAlias,
    ComponentFragmentSignalAlias, ConsoleLevel, EncodingOp, EventAttributeSource, EventCollection,
    EventCollectionField, EventFileSource, EventHandlerId, EventName, EventValue, ExprId,
    InputKind, InvalidationStrategy, Island, IslandBuilder, JsonPointer, LocalId, LocationField,
    MutationAccessor, MutationId, MutationOptions, NodeId, NodeRefAccessor, NodeRefId,
    QueryAccessor, QueryId, QueryOptions, SharedStoreId, SignalId, StorageArea, StringPredicateOp,
    StringTransformOp, UrlHistoryMode, Value, ViewNode,
};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use syn::fold::{self, Fold};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Block, Expr, ExprArray, ExprAssign, ExprBinary, ExprBlock, ExprCall, ExprClosure, ExprForLoop,
    ExprIf, ExprLit, ExprMacro, ExprMatch, ExprMethodCall, ExprPath, ExprRange, ExprStruct,
    ExprUnary, Lit, Local, Pat, RangeLimits, Stmt,
};

use crate::parse::ParsedIsland;

mod ast_expr;
mod browser;
mod client_lowering;
mod components;
mod events;
mod handlers;
mod hir_json;
mod prelude;
mod queries;
mod reactive_expr;
mod template;
#[cfg(test)]
mod tests;
mod text;

pub(crate) use hir_json::hir_sidecar_json;
#[cfg(test)]
pub(crate) use prelude::signal_initializers;
pub(crate) use prelude::{
    is_handler_local, is_memo_local, is_mutation_local, is_node_ref_local, is_query_local,
    is_query_runtime_initial_local, is_signal_local, path_ends_with, query_initializers,
    signal_call_explicit_type,
};

use browser::*;
use client_lowering::*;
use components::*;
use events::*;
use handlers::*;
#[cfg(test)]
use hir_json::*;
use prelude::*;
use queries::*;
use reactive_expr::*;
use template::*;

/// Run the analyser on a parsed island, producing an [`Island`] IR and
/// frontend HIR metadata for conformance tooling.
pub(crate) fn analyze(parsed: &ParsedIsland) -> syn::Result<AnalysisOutput> {
    let generic_static_slot = generic_static_slot_field(parsed);
    let mut a = Analyzer {
        builder: IslandBuilder::new(),
        shared_stores: HashMap::new(),
        shared_fields: HashMap::new(),
        signals: HashMap::new(),
        unsupported_integer_signals: HashMap::new(),
        memo_cells: HashSet::new(),
        queries: HashMap::new(),
        mutations: HashMap::new(),
        node_refs: HashMap::new(),
        inbound_node_refs: HashMap::new(),
        handlers: HashMap::new(),
        server_prelude_locals: parsed
            .prelude_lets
            .iter()
            .filter_map(local_ident)
            .map(|ident| ident.to_string())
            .collect(),
        client_prelude_locals: HashMap::new(),
        unsupported_client_prelude_locals: HashMap::new(),
        hir_sidecar: HirSidecar::default(),
        client_module_imports: Vec::new(),
        loop_scopes: Vec::new(),
        component_lifecycle_stack: Vec::new(),
        keyed_list_guards: Vec::new(),
        payload_counter: 0,
        next_handler_local_id: 0,
        direct_component_edges: Vec::new(),
        shared_state_provider_sites: Vec::new(),
        direct_helper_edges: Vec::new(),
        server_expressions: HashMap::new(),
        direct_component_site_nodes: HashMap::new(),
        inbound_inputs: InboundInputMetadata::default(),
        inbound_live_signals: HashMap::new(),
        generic_static_slot_field: generic_static_slot.as_ref().map(|slot| slot.name.clone()),
        inbound_temporal_context: InboundTemporalMode::LiveRead,
        // Direct component syntax is a Rust-resolved canonical linker edge.
        // Descriptor compilation must never copy a discoverable schema-v9
        // child's legacy graph into its parent: schema v9 cannot describe the
        // copied fragment's ownership/exclusion boundary, so canonical linking
        // would otherwise materialize the child graph a second time. Keep the
        // old inliner reachable only to focused analyzer compatibility tests.
        preserve_direct_component_sites: true,
        is_async: parsed.render_kind.is_async(),
        client_origin_allowed: parsed.client_origin_allowed,
        query_initializers: Vec::new(),
        mutation_initializers: Vec::new(),
        signal_initializers: Vec::new(),
        shared_state_initializers: Vec::new(),
        prelude_signal_ids: HashMap::new(),
        prelude_shared_store_ids: HashMap::new(),
        prelude_query_ids: HashMap::new(),
        prelude_mutation_ids: HashMap::new(),
        prelude_node_ref_ids: HashMap::new(),
        shared_prelude_local_indices: HashSet::new(),
        client_prelude_local_indices: HashSet::new(),
    };
    a.builder
        .set_hydration_strategy(parsed.hydration_strategy.clone());
    a.collect_prelude_bindings(&parsed.prelude_statements, &parsed.prelude_lets)?;
    a.validate_opaque_server_statements(&parsed.prelude_statements)?;
    a.walk_root(&parsed.html_tree)?;
    a.validate_keyed_list_containers()?;
    let direct_payload_handler_values = a.materialize_direct_payload_handler_values()?;
    if parsed.render_kind.is_async() {
        a.builder.set_async(true);
    }
    Ok(AnalysisOutput {
        island: a.builder.build(),
        hir_sidecar: a.hir_sidecar,
        signal_initializers: a.signal_initializers,
        query_initializers: a.query_initializers,
        mutation_initializers: a.mutation_initializers,
        shared_state_initializers: a.shared_state_initializers,
        prelude_signal_ids: a.prelude_signal_ids,
        prelude_shared_store_ids: a.prelude_shared_store_ids,
        prelude_query_ids: a.prelude_query_ids,
        prelude_mutation_ids: a.prelude_mutation_ids,
        prelude_node_ref_ids: a.prelude_node_ref_ids,
        shared_prelude_local_indices: a.shared_prelude_local_indices,
        client_prelude_local_indices: a.client_prelude_local_indices,
        direct_component_edges: a.direct_component_edges,
        shared_state_provider_sites: a.shared_state_provider_sites,
        direct_helper_edges: a.direct_helper_edges,
        server_expressions: a.server_expressions,
        direct_component_site_nodes: a.direct_component_site_nodes,
        inbound_inputs: a.inbound_inputs,
        generic_static_slot,
        direct_payload_handler_values,
    })
}

/// Infer the first deliberately narrow generic immutable-slot shape.
///
/// Rust type resolution is unavailable while this proc macro is lowering the
/// template. A direct `self.field` is therefore normally a live component prop.
/// One useful closed exception is the conventional `children` field on an open
/// render wrapper with exactly one generic type argument and exactly one direct
/// `self.children` use. The generated Rust expansion emits a type-equality
/// probe proving that the field really has that generic type; a mismatch fails
/// compilation instead of silently changing a normal scalar prop into a server
/// slot. An arbitrary generic `self.value` remains a live prop.
///
/// The parent-side relocation still requires a nameable component family.
/// Today that means the child constructor needs a defaulted generic argument;
/// non-default open generic edge identities remain fail-closed.
fn generic_static_slot_field(parsed: &ParsedIsland) -> Option<GenericStaticSlotField> {
    let generic_ty = sole_direct_self_type_parameter(parsed)?;
    let mut direct_children = Vec::<String>::new();
    let mut uses = HashMap::<String, usize>::new();
    collect_template_self_field_uses(&parsed.html_tree.nodes, &mut direct_children, &mut uses);
    for statement in &parsed.prelude_statements {
        let mut counter = SelfFieldUseCounter { uses: &mut uses };
        counter.visit_stmt(statement);
    }

    if direct_children.len() != 1 {
        return None;
    }
    let name = direct_children.pop().expect("length checked above");
    if name != "children" {
        return None;
    }
    (uses.get(&name) == Some(&1)).then_some(GenericStaticSlotField {
        name,
        ty: generic_ty,
    })
}

fn sole_direct_self_type_parameter(parsed: &ParsedIsland) -> Option<syn::Type> {
    use syn::{GenericArgument, PathArguments, Type};

    let type_parameters = parsed
        .impl_generics
        .type_params()
        .map(|param| param.ident.to_string())
        .collect::<HashSet<_>>();
    let Type::Path(self_ty) = &parsed.self_ty else {
        return None;
    };
    let PathArguments::AngleBracketed(arguments) = &self_ty.path.segments.last()?.arguments else {
        return None;
    };
    let mut matching = arguments.args.iter().filter_map(|argument| {
        let GenericArgument::Type(Type::Path(ty)) = argument else {
            return None;
        };
        let ident = ty.path.get_ident()?;
        type_parameters
            .contains(&ident.to_string())
            .then(|| Type::Path(ty.clone()))
    });
    let ty = matching.next()?;
    matching.next().is_none().then_some(ty)
}

fn collect_template_self_field_uses(
    nodes: &[Node],
    direct_children: &mut Vec<String>,
    uses: &mut HashMap<String, usize>,
) {
    for node in nodes {
        match node {
            Node::Element(element) => {
                for attribute in &element.attrs {
                    if let Attribute::Dynamic { value, .. } = attribute {
                        let mut counter = SelfFieldUseCounter { uses };
                        counter.visit_expr(value);
                    }
                }
                collect_template_self_field_uses(&element.children, direct_children, uses);
            }
            Node::Fragment(children) => {
                collect_template_self_field_uses(children, direct_children, uses);
            }
            Node::Block(expression) => {
                if let Some(name) = exact_self_field_name(expression) {
                    direct_children.push(name);
                }
                let mut counter = SelfFieldUseCounter { uses };
                counter.visit_expr(expression);
            }
            Node::Text(_) | Node::Doctype(_) => {}
        }
    }
}

fn exact_self_field_name(expression: &Expr) -> Option<String> {
    let Expr::Field(field) = expression else {
        return None;
    };
    if !expr_is_self_path(&field.base) {
        return None;
    }
    let syn::Member::Named(name) = &field.member else {
        return None;
    };
    Some(name.to_string())
}

struct SelfFieldUseCounter<'a> {
    uses: &'a mut HashMap<String, usize>,
}

impl<'ast> Visit<'ast> for SelfFieldUseCounter<'_> {
    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if expr_is_self_path(&field.base) {
            if let syn::Member::Named(name) = &field.member {
                *self.uses.entry(name.to_string()).or_default() += 1;
            }
        }
        visit::visit_expr_field(self, field);
    }
}

impl Analyzer {
    /// Materialize the callback values selected by direct live-component edges.
    ///
    /// The source edge carries an exact prelude-local index rather than
    /// pretending that syntax provenance is already a canonical graph value.
    /// This pass resolves that index back to the one payload handler visible
    /// during template analysis and lowers its captured closure exactly once.
    /// DOM `event_handler(...)` declarations are intentionally ineligible.
    fn materialize_direct_payload_handler_values(
        &mut self,
    ) -> syn::Result<Vec<DirectPayloadHandlerValue>> {
        let referenced = self
            .direct_component_edges
            .iter()
            .flat_map(|edge| edge.fields.iter())
            .filter_map(|field| match field.binding {
                DirectComponentBindingClassification::LiveCallback {
                    handler_prelude_local,
                    ..
                } => Some(handler_prelude_local),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if referenced.is_empty() {
            return Ok(Vec::new());
        }

        let mut handlers_by_prelude = BTreeMap::new();
        for handler in self.handlers.values().cloned() {
            if handlers_by_prelude
                .insert(handler.prelude_local_index, handler.clone())
                .is_some()
            {
                return Err(syn::Error::new(
                    handler.name.span(),
                    format!(
                        "rsjs/direct-live-callback-duplicate-source: more than one handler \
                         declaration claims prelude binding {}; callback value provenance \
                         must be unique",
                        handler.prelude_local_index
                    ),
                ));
            }
        }

        let mut values = Vec::with_capacity(referenced.len());
        for prelude_local_index in referenced {
            let handler = handlers_by_prelude
                .remove(&prelude_local_index)
                .ok_or_else(|| {
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        format!(
                            "rsjs/direct-live-callback-missing-source: component transport \
                             references payload handler prelude binding {prelude_local_index}, \
                             but no such declaration is available"
                        ),
                    )
                })?;
            if handler.kind != HandlerLocalKind::Payload {
                return Err(syn::Error::new(
                    handler.name.span(),
                    format!(
                        "rsjs/direct-live-callback-event-handler: prelude binding \
                         {prelude_local_index} is an `rsjs::event_handler(...)`; direct \
                         component callbacks require `rsjs::handler(...)`"
                    ),
                ));
            }

            let parameter = payload_handler_param(&handler.closure)?;
            let payload = (parameter.arity == 1).then_some(ClientExpr::CallbackPayload);
            let body_ir = self.inline_handler_local_client_block_with_payload(
                &handler,
                payload,
                &HandlerScope::default(),
                handler.name.span(),
            )?;
            let dependencies = direct_payload_handler_dependencies(&body_ir)?;
            values.push(DirectPayloadHandlerValue {
                prelude_local_index,
                payload_arity: u8::try_from(parameter.arity).expect("handler arity is at most one"),
                source_anchor: handler.source_anchor,
                dependencies,
                body_ir,
            });
        }
        Ok(values)
    }
}

/// Expand non-reactive prelude locals into server-only descriptor tokens.
///
/// Standalone component rendering still replays the user's prelude lets
/// verbatim. This descriptor-only pass exists for component inlining: parent
/// islands consume only the child descriptor, so a child-local server helper
/// like `let classes = input_classes(self.size);` must be visible inside
/// `class={rsjs::server_attr(classes)}` before the parent can substitute
/// `self.size`.
pub(crate) fn expand_server_prelude_locals_for_descriptor(
    island: &mut Island,
    lets: &[Local],
) -> syn::Result<()> {
    let locals = collect_server_prelude_local_exprs(lets);
    if locals.is_empty() {
        return Ok(());
    }

    for idx in 0..island.view_tree.len() {
        let node = NodeId(idx as u32);
        match island.view_tree.get_mut(node) {
            ViewNode::ServerOnly(server) => {
                server.tokens = rewrite_expr_tokens_with_server_locals(&server.tokens, &locals)?;
            }
            ViewNode::Element {
                server_attrs,
                payload_attrs,
                ..
            } => {
                for attr in server_attrs {
                    attr.tokens = rewrite_expr_tokens_with_server_locals(&attr.tokens, &locals)?;
                }
                for attr in payload_attrs {
                    attr.tokens = rewrite_expr_tokens_with_server_locals(&attr.tokens, &locals)?;
                }
            }
            ViewNode::ForLoop {
                expr, body_prefix, ..
            } => {
                *expr = rewrite_expr_tokens_with_server_locals(expr, &locals)?;
                *body_prefix = rewrite_statement_tokens_with_server_locals(body_prefix, &locals)?;
            }
            ViewNode::TextBindingSlot | ViewNode::StaticText(_) => {}
            _ => {}
        }
    }

    for expr in island.exprs.iter_client_exprs_mut() {
        rewrite_client_expr_server_locals(expr, &locals)?;
    }

    Ok(())
}

fn rewrite_client_expr_server_locals(
    expr: &mut ClientExpr,
    locals: &HashMap<String, Expr>,
) -> syn::Result<()> {
    match expr {
        ClientExpr::NodePayload { tokens, .. } => {
            *tokens = rewrite_expr_tokens_with_server_locals(tokens, locals)?;
        }
        ClientExpr::QueryKey(parts) | ClientExpr::Call { args: parts, .. } => {
            for part in parts {
                rewrite_client_expr_server_locals(part, locals)?;
            }
        }
        ClientExpr::EventCollectionRead { index, .. }
        | ClientExpr::DomRead { target: index, .. }
        | ClientExpr::Encoding { value: index, .. }
        | ClientExpr::IsNonEmpty { target: index }
        | ClientExpr::StringToLowercase { target: index }
        | ClientExpr::StringTransform { target: index, .. }
        | ClientExpr::Project { target: index, .. }
        | ClientExpr::AsType { target: index, .. }
        | ClientExpr::IsPresent { target: index }
        | ClientExpr::Eq { target: index, .. }
        | ClientExpr::Neq { target: index, .. }
        | ClientExpr::Coalesce { target: index, .. }
        | ClientExpr::Unary { expr: index, .. }
        | ClientExpr::Field { base: index, .. } => {
            rewrite_client_expr_server_locals(index, locals)?;
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_client_expr_server_locals(condition, locals)?;
            rewrite_client_expr_server_locals(then_expr, locals)?;
            rewrite_client_expr_server_locals(else_expr, locals)?;
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        }
        | ClientExpr::Binary {
            left: haystack,
            right: needle,
            ..
        } => {
            rewrite_client_expr_server_locals(haystack, locals)?;
            rewrite_client_expr_server_locals(needle, locals)?;
        }
        ClientExpr::Index { base, index } => {
            rewrite_client_expr_server_locals(base, locals)?;
            rewrite_client_expr_server_locals(index, locals)?;
        }
        ClientExpr::BrowserRead(ClientBrowserRead::Storage { key, fallback, .. })
        | ClientExpr::BrowserRead(ClientBrowserRead::UrlSearchParam { key, fallback }) => {
            rewrite_client_expr_server_locals(key, locals)?;
            if let Some(fallback) = fallback {
                rewrite_client_expr_server_locals(fallback, locals)?;
            }
        }
        ClientExpr::BrowserRead(ClientBrowserRead::Declared { args, .. }) => {
            for arg in args {
                rewrite_client_expr_server_locals(arg, locals)?;
            }
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
        | ClientExpr::ArrayLit(_)
        | ClientExpr::Local(_)
        | ClientExpr::SignalRead(_)
        | ClientExpr::QueryRead { .. }
        | ClientExpr::MutationRead { .. }
        | ClientExpr::EventRead(_)
        | ClientExpr::EventCurrentTarget
        | ClientExpr::EventCollectionLen(_)
        | ClientExpr::NodeRefRead { .. }
        | ClientExpr::EventTargetWithinNodeRef { .. }
        | ClientExpr::EventAttribute { .. }
        | ClientExpr::EventPayload { .. }
        | ClientExpr::CallbackPayload
        | ClientExpr::BrowserRead(ClientBrowserRead::Window(_))
        | ClientExpr::BrowserRead(ClientBrowserRead::Location(_)) => {}
        _ => {}
    }

    Ok(())
}

pub(crate) fn expand_server_prelude_expr_for_descriptor(expr: &Expr, lets: &[Local]) -> Expr {
    let locals = collect_server_prelude_local_exprs(lets);
    if locals.is_empty() {
        return expr.clone();
    }
    let mut folder = ServerPreludeLocalSubstitution { locals: &locals };
    folder.fold_expr(expr.clone())
}

#[derive(Clone)]
struct Analyzer {
    builder: IslandBuilder,
    /// Source local -> shared-store acquisition id.
    shared_stores: HashMap<String, SharedStoreId>,
    /// One signal id per `(provider acquisition, top-level field)` even when
    /// authoring uses the accessor chain in multiple places.
    shared_fields: HashMap<(SharedStoreId, String), SignalId>,
    /// Map from the source-level `let <name> = signal(...);` or
    /// `let <name> = memo(|| ...);` ident string to the
    /// [`SignalId`] the builder allocated. Memo cells share the
    /// signal id space (see `rsjs_ir::SignalDecl::derived`), so reads
    /// resolve through this one map; `memo_cells` records which of
    /// these ids are read-only derived cells.
    signals: HashMap<String, SignalId>,
    /// Explicit fixed-width integer signal types outside the checked v1
    /// domain set. Their Number/BigInt representation is known, but arithmetic
    /// must reject rather than silently inherit JavaScript overflow behavior.
    unsupported_integer_signals: HashMap<SignalId, String>,
    /// Ids in `signals` that were declared with `memo(...)`.
    /// Writes (`.set`, `.update`, `bind_*`, `*_into(&...)` targets)
    /// against these ids are rejected.
    memo_cells: HashSet<SignalId>,
    /// Map from `let <name> = query_with_initial(...);` locals to query ids.
    queries: HashMap<String, QueryId>,
    /// Map from `let <name> = mutation(...);` locals to mutation ids.
    mutations: HashMap<String, MutationId>,
    /// Map from `let <name> = node_ref::<T>();` locals to node-ref ids.
    node_refs: HashMap<String, NodeRefId>,
    /// Required inbound `NodeRef<T>` fields keyed by their Rust field name.
    ///
    /// These refs are aliases supplied by a statically composed parent. They
    /// are deliberately separate from locally originated `node_ref::<T>()`
    /// declarations: consuming one is participation and therefore does not
    /// require `#[rsjs(component, client)]`.
    inbound_node_refs: HashMap<String, NodeRefId>,
    /// Map from local handler declarations to their source closure.
    ///
    /// `rsjs::handler(|payload| ...)` locals are payload-only callback refs.
    /// `rsjs::event_handler(|event, payload| ...)` locals are DOM-event-aware
    /// refs. Event attributes may call either form directly
    /// (`on_click={name(())}`) and the analyser inlines the closure body into
    /// the concrete event handler it builds.
    handlers: HashMap<String, HandlerLocal>,
    /// Every plain identifier bound by a pre-template `let`.
    ///
    /// This lets tolerant SSR distinguish an ordinary server object's
    /// `.get()` method from a likely misspelled/unbound RSJS signal read. The
    /// client maps above remain authoritative when a local is compiler-owned.
    server_prelude_locals: HashSet<String>,
    /// Ordinary prelude locals whose initializer lowered to a client-reachable
    /// expression, e.g. `let display = count.get();` or
    /// `let formatted = pricing::format(count.get());`.
    ///
    /// These locals are aliases, not server render state. Template reads must
    /// lower through the stored expression instead of being inferred as simple
    /// SSR slots, and emit_rust must not replay the source `let` because it can
    /// reference rsjs declarations that were already replaced by generated
    /// initial-value locals.
    client_prelude_locals: HashMap<String, FrontendExpr>,
    /// Ordinary prelude locals that mention client-reachable state but use Rust
    /// syntax the Client IR cannot lower.
    ///
    /// They remain normal server locals unless a later reactive/client position
    /// references them. This allows opaque server computations to seed signals
    /// while still rejecting `{display}` when `display` came from unsupported
    /// client-derived syntax such as `format!("{}", count.get())`.
    unsupported_client_prelude_locals: HashMap<String, UnsupportedClientPreludeLocal>,
    /// Stable HIR artifacts for macro-expanded conformance sidecars.
    hir_sidecar: HirSidecar,
    /// Transitional bundle imports inherited from descriptor composition.
    ///
    /// New source cannot declare component-level `clients(...)` allowlists.
    /// Helper eligibility and identity come from rustc-resolved standalone
    /// helper edges. This field remains only while the older manifest/IR schema
    /// can carry bundle metadata from already-produced descriptors.
    client_module_imports: Vec<String>,
    /// Active template-loop lexical scopes. Used while lowering event handlers
    /// inside loop bodies so loop-local payload expressions can be serialized
    /// into per-element payload attributes.
    loop_scopes: Vec<LoopScope>,
    /// Active parent-reactive conditional regions, outermost first.
    ///
    /// One active region is a conditional-single component occurrence. Two or
    /// more active regions are deliberately retained as an unknown lifecycle
    /// until the instance ABI defines conjunction/identity semantics.
    component_lifecycle_stack: Vec<DirectConditionalLifecycleScope>,
    /// Keyed lists awaiting the post-walk only-child container check.
    keyed_list_guards: Vec<KeyedListGuard>,
    /// Deterministic suffix for generated loop-payload attributes.
    payload_counter: u32,
    /// Deterministic id source for handler-local `let` bindings.
    next_handler_local_id: u32,
    /// Direct struct-literal component sites in source traversal order.
    ///
    /// The proc macro cannot resolve imported aliases itself. Each entry keeps
    /// the authored type path so generated Rust can ask
    /// `<ResolvedType as ComponentAbi>::WITNESS` and let rustc perform normal
    /// import, alias, and re-export resolution.
    direct_component_edges: Vec<DirectComponentSourceEdge>,
    /// Static compiler-recognized `SharedStateProvider` topology sites.
    ///
    /// Each provider retains one ordinary composed child edge for rustc type
    /// resolution while this table authenticates the wrapperless provider
    /// declaration and its SharedState schema witness separately.
    shared_state_provider_sites: Vec<DirectSharedStateProviderSite>,
    /// Direct helper-call syntax retained for relocation-record emission.
    direct_helper_edges: Vec<DirectHelperSourceEdge>,
    /// Original opaque SSR expressions keyed by their exact local view node.
    ///
    /// The syn-free IR keeps a redacted/tokenized representation for
    /// descriptors and diagnostics. Local Rust emission must use these source
    /// nodes instead so proc-macro hygiene and spans are never reconstructed
    /// from a string.
    server_expressions: HashMap<NodeId, Expr>,
    /// Opaque server nodes that are exact direct component syntax sites.
    ///
    /// The value is the parent-local component edge ID. Final linked emission
    /// wraps the original `syn::ExprStruct` with the selected child site
    /// rather than reconstructing the imported Rust path.
    direct_component_site_nodes: HashMap<NodeId, u32>,
    /// Latent component-input schema, independent of ordinary island signals
    /// and browser-obligation roots.
    ///
    /// These records describe how `self.<field>` would behave if a linker
    /// later connects the component to a live parent value. Merely recording
    /// one never creates a client root or forces hydration.
    inbound_inputs: InboundInputMetadata,
    /// Direct `self.<field>` value reads share one producer-local signal.
    ///
    /// The signal is a symbolic, read-only input in the component descriptor,
    /// not locally owned state. Every syntactic read still records its own
    /// `LiveRead` prop-use site, while all of those sites target this one
    /// signal so a linked parent can install one current-value alias.
    inbound_live_signals: HashMap<String, SignalId>,
    /// One rustc-validated open-generic field that is rendered exactly once as
    /// immutable server DOM rather than becoming a client value/codec edge.
    generic_static_slot_field: Option<String>,
    /// Temporal meaning of a direct inbound field read in the active lowering
    /// context.
    ///
    /// Ordinary reactive/template lowering uses `LiveRead`. Structural query
    /// key lowering temporarily selects `QueryKeyDependency` so the semantic
    /// linker can rebind an observer when the parent value changes without
    /// allocating a second synthetic signal or treating the field as locally
    /// originated state.
    inbound_temporal_context: InboundTemporalMode,
    /// Final linked compilation keeps direct component calls intact instead
    /// of using the transitional filesystem-sidecar inliner.
    preserve_direct_component_sites: bool,
    /// Whether the source impl used async `WebRender`.
    is_async: bool,
    /// Whether the authored component explicitly permits locally originated
    /// browser behavior through `#[rsjs(component, client)]`.
    client_origin_allowed: bool,
    /// Query initializers copied from child component descriptors and remapped
    /// into this parent island's query id space.
    query_initializers: Vec<QueryInitializer>,
    /// Mutations whose structural key must be serialized at server-render time
    /// because the static browser expression cannot lower it.
    mutation_initializers: Vec<MutationInitializer>,
    /// Runtime-valued signal initializers paired with the exact signal id
    /// allocated while walking the prelude in lexical order.
    signal_initializers: Vec<RuntimeSignalInitializer>,
    /// Original Rust types retained outside the syn-free IR for generated SSR
    /// provider acquisition.
    shared_state_initializers: Vec<SharedStateInitializer>,
    /// Exact lexical source binding -> signal/shared-field id mapping.
    prelude_signal_ids: HashMap<usize, SignalId>,
    /// Exact lexical source acquisition -> shared-store id mapping.
    prelude_shared_store_ids: HashMap<usize, SharedStoreId>,
    /// Exact lexical source declaration -> query id mapping.
    prelude_query_ids: HashMap<usize, QueryId>,
    /// Exact lexical source declaration -> mutation id mapping.
    prelude_mutation_ids: HashMap<usize, MutationId>,
    /// Exact lexical source declaration -> node-ref id mapping.
    prelude_node_ref_ids: HashMap<usize, NodeRefId>,
    /// Positions (indices into the island prelude) of the `let` bindings
    /// consumed by shared-state lowering.
    ///
    /// Names alone cannot say which binding shared-state lowering took: a
    /// server local may deliberately shadow a shared field, or be shadowed by
    /// one. The replay filter and the shadow resolution both key on this
    /// position so every other same-named `let` keeps its own meaning.
    shared_prelude_local_indices: HashSet<usize>,
    /// Positions of the `let` bindings recorded as client-prelude aliases
    /// (supported or unsupported), for the same positional reason.
    client_prelude_local_indices: HashSet<usize>,
}

#[derive(Clone, Copy)]
enum ClientOriginKind {
    Signal,
    Handler,
    Timer,
    Query,
    Mutation,
    SharedState,
    NodeRef,
    BrowserRead,
}

impl ClientOriginKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Signal => "writable local signal",
            Self::Handler => "new event-handler body",
            Self::Timer => "timer",
            Self::Query => "query acquisition",
            Self::Mutation => "mutation acquisition",
            Self::SharedState => "SharedState acquisition",
            Self::NodeRef => "node-ref acquisition",
            Self::BrowserRead => "browser capability read",
        }
    }
}

impl Analyzer {
    pub(super) fn record_server_expression(
        &mut self,
        parent: NodeId,
        expression: &Expr,
        source: rsjs_ir::ServerSurfaceSource,
        source_anchor: Option<rsjs_ir::ServerSurfaceAnchor>,
    ) -> NodeId {
        let id = self.builder.server_only_node_with_source_anchor(
            parent,
            &expression.to_token_stream().to_string(),
            source,
            source_anchor,
        );
        let previous = self.server_expressions.insert(id, expression.clone());
        debug_assert!(
            previous.is_none(),
            "a freshly allocated server-expression node must be unique"
        );
        id
    }

    fn require_client_origin(
        &self,
        span: proc_macro2::Span,
        kind: ClientOriginKind,
    ) -> syn::Result<()> {
        if self.client_origin_allowed {
            return Ok(());
        }
        Err(syn::Error::new(
            span,
            format!(
                "this {} originates browser behavior, but the component is \
                 participant-only\nhelp: change the attribute to \
                 `#[rsjs(component, client)]`, or move the origin into a \
                 client-capable child",
                kind.label()
            ),
        ))
    }

    fn record_inbound_signal_seed(
        &mut self,
        field_name: String,
        field_span: proc_macro2::Span,
        signal: SignalId,
    ) -> syn::Result<()> {
        self.record_inbound_value_use(
            field_name,
            field_span,
            InboundTemporalMode::SeedOnce,
            InboundPropTarget::SignalSeed(signal),
        )
    }

    fn record_inbound_live_read(
        &mut self,
        field_name: String,
        field_span: proc_macro2::Span,
        signal: SignalId,
    ) -> syn::Result<()> {
        self.record_inbound_value_use(
            field_name,
            field_span,
            InboundTemporalMode::LiveRead,
            InboundPropTarget::LiveSignal(signal),
        )
    }

    fn ensure_inbound_live_signal(
        &mut self,
        field: &syn::ExprField,
    ) -> syn::Result<(String, SignalId)> {
        let syn::Member::Named(member) = &field.member else {
            return Err(server_owned_self_error(&Expr::Field(field.clone())));
        };
        let field_name = member.to_string();
        let signal = if let Some(signal) = self.inbound_live_signals.get(&field_name).copied() {
            signal
        } else if let Some(projection) =
            self.inbound_inputs
                .projection_fields
                .iter()
                .find(|projection| {
                    self.inbound_inputs.fields[projection.field as usize].name == field_name
                })
        {
            let signal = projection.signal;
            self.inbound_live_signals.insert(field_name.clone(), signal);
            signal
        } else {
            let signal = self.builder.signal_typed(Value::Null, ClientType::Unknown);
            self.builder.set_signal_source(
                signal,
                Some(format!("self.{field_name}")),
                Some(rsjs_source_anchor(field.span())),
            );
            self.signal_initializers.push(RuntimeSignalInitializer {
                id: signal,
                expr: Expr::Field(field.clone()),
                explicit_ty: None,
            });
            self.inbound_live_signals.insert(field_name.clone(), signal);
            signal
        };
        Ok((field_name, signal))
    }

    fn lower_inbound_live_field(&mut self, field: &syn::ExprField) -> syn::Result<FrontendExpr> {
        let (field_name, signal) = self.ensure_inbound_live_signal(field)?;
        self.record_inbound_value_use(
            field_name,
            field.span(),
            self.inbound_temporal_context,
            InboundPropTarget::LiveSignal(signal),
        )?;
        Ok(FrontendExpr::SignalRead(signal))
    }

    fn record_inbound_boundary_crossing(
        &mut self,
        field_name: String,
        field_span: proc_macro2::Span,
    ) -> syn::Result<()> {
        self.record_inbound_value_use(
            field_name,
            field_span,
            InboundTemporalMode::ForbiddenBoundaryCrossing,
            InboundPropTarget::ExplicitIslandBoundary,
        )
    }

    fn record_inbound_provider_seed(
        &mut self,
        field_name: String,
        field_span: proc_macro2::Span,
    ) -> syn::Result<()> {
        self.record_inbound_value_use(
            field_name,
            field_span,
            InboundTemporalMode::ForbiddenProviderSeed,
            InboundPropTarget::SharedStateProviderSeed,
        )
    }

    fn record_inbound_callback(
        &mut self,
        field_name: &str,
        field_span: proc_macro2::Span,
        shape: InboundCallbackShape,
    ) -> syn::Result<()> {
        let field =
            self.ensure_inbound_field(field_name, InboundFieldRole::Callback, field_span)?;
        let existing = &mut self.inbound_inputs.fields[field as usize];
        match existing.callback_shape {
            Some(existing_shape) if existing_shape != shape => {
                return Err(syn::Error::new(
                    field_span,
                    format!(
                        "inbound callback field `self.{field_name}` has conflicting callback \
                         shapes: it was first invoked as `{}` but this use requires `{}`\n\
                         help: required callbacks use `Handler<T>` with `.call(payload)`; \
                         optional callbacks use `Option<Handler<T>>` with \
                         `.call_if_some(payload)`",
                        existing_shape.as_str(),
                        shape.as_str(),
                    ),
                ));
            }
            Some(_) => {}
            None => existing.callback_shape = Some(shape),
        }
        Ok(())
    }

    /// Resolve or create the child-local alias identity for `self.<field>`.
    ///
    /// Unlike `node_ref::<T>()`, this does not create a browser origin. The
    /// field's generated Rust witness proves that its concrete type is a
    /// required `NodeRef<T>`, while final direct composition must supply the
    /// exact parent ref object selected by the linked handle edge.
    fn ensure_inbound_node_ref(
        &mut self,
        field: &syn::ExprField,
    ) -> syn::Result<(String, NodeRefId)> {
        let syn::Member::Named(member) = &field.member else {
            return Err(syn::Error::new(
                field.span(),
                "rsjs/node-ref-handle-field: inbound node refs must use a named field such as `self.input_ref`",
            ));
        };
        if !matches!(
            field.base.as_ref(),
            Expr::Path(path) if path.path.is_ident("self")
        ) {
            return Err(syn::Error::new(
                field.span(),
                "rsjs/node-ref-handle-field: inbound node refs must be direct `self.<field>` projections",
            ));
        }
        let field_name = member.to_string();
        if let Some(node_ref) = self.inbound_node_refs.get(&field_name).copied() {
            return Ok((field_name, node_ref));
        }

        let field_id =
            self.ensure_inbound_field(&field_name, InboundFieldRole::NodeRef, field.span())?;
        let (value, crossing) = {
            let schema = &self.inbound_inputs.fields[field_id as usize];
            (
                schema
                    .value
                    .expect("client-handle fields always allocate one local value"),
                schema.crossing,
            )
        };
        let node_ref = if let Some(projection) = self
            .inbound_inputs
            .projection_fields
            .iter()
            .find(|projection| projection.field == field_id)
        {
            projection.node_ref
        } else {
            let node_ref = self.builder.node_ref();
            self.builder.set_node_ref_source(
                node_ref,
                Some(format!("self.{field_name}")),
                Some(rsjs_source_anchor(field.span())),
            );
            node_ref
        };
        self.inbound_inputs
            .node_ref_fields
            .push(InboundNodeRefField {
                field: field_id,
                value,
                crossing,
                node_ref,
                bind_target: None,
                exact_bind_target: None,
                source_anchor: rsjs_source_anchor(field.span()),
            });
        self.inbound_node_refs.insert(field_name.clone(), node_ref);
        Ok((field_name, node_ref))
    }

    /// Record a direct composed `self.<field>` projection whose browser role
    /// is determined only after rustc resolves the child component edge.
    ///
    /// The raw descriptor carries both possible identity-bearing paths:
    /// `signal` supports a value prop without taking an SSR snapshot, while
    /// `node_ref` supports an exact handle alias. Callback forwarding uses the
    /// field's symbolic inbound value directly and therefore needs no
    /// intermediate callback slot.
    fn ensure_deferred_inbound_projection(
        &mut self,
        field: &syn::ExprField,
    ) -> syn::Result<InboundProjectionField> {
        let syn::Member::Named(member) = &field.member else {
            return Err(server_owned_self_error(&Expr::Field(field.clone())));
        };
        let field_name = member.to_string();
        let field_id = self.ensure_inbound_field(
            &field_name,
            InboundFieldRole::DeferredProjection,
            field.span(),
        )?;
        if let Some(projection) = self
            .inbound_inputs
            .projection_fields
            .iter()
            .find(|projection| projection.field == field_id)
        {
            return Ok(projection.clone());
        }

        let schema = &self.inbound_inputs.fields[field_id as usize];
        let value = schema
            .value
            .expect("deferred projection fields own one symbolic value");
        let crossing = schema.crossing;
        let source_anchor = rsjs_source_anchor(field.span());

        let signal = self.builder.signal_typed(Value::Null, ClientType::Unknown);
        self.builder.set_signal_source(
            signal,
            Some(format!("self.{field_name}")),
            Some(source_anchor),
        );
        self.signal_initializers.push(RuntimeSignalInitializer {
            id: signal,
            expr: Expr::Field(field.clone()),
            explicit_ty: None,
        });

        let node_ref = self.builder.node_ref();
        self.builder.set_node_ref_source(
            node_ref,
            Some(format!("self.{field_name}")),
            Some(source_anchor),
        );

        let projection = InboundProjectionField {
            field: field_id,
            value,
            crossing,
            signal,
            node_ref,
            source_anchor,
        };
        self.inbound_inputs
            .projection_fields
            .push(projection.clone());
        Ok(projection)
    }

    fn record_inbound_node_ref_binding(
        &mut self,
        node_ref: NodeRefId,
        tag: &str,
        span: proc_macro2::Span,
    ) -> syn::Result<()> {
        let Some(field) = self
            .inbound_inputs
            .node_ref_fields
            .iter_mut()
            .find(|field| field.node_ref == node_ref)
        else {
            // A locally originated ref is checked by its own declaration/use
            // path; only inbound aliases need a component-field witness.
            return Ok(());
        };
        let exact_target = rsjs_ir::dom_registry::find_dom_tag(tag)
            .map(|spec| spec.element)
            .unwrap_or(rsjs_ir::dom_registry::DomElementKind::HtmlElement);
        let target = NodeRefBindTarget::for_tag(tag);
        if let Some(existing) = field.bind_target {
            return Err(syn::Error::new(
                span,
                format!(
                    "rsjs/node-ref-handle-duplicate-binding: inbound node-ref field is already bound as `{}`; the complete alias chain may bind exactly one stable element",
                    existing.as_str()
                ),
            ));
        }
        field.bind_target = Some(target);
        field.exact_bind_target = Some(exact_target);
        Ok(())
    }

    fn record_inbound_value_use(
        &mut self,
        field_name: String,
        field_span: proc_macro2::Span,
        temporal: InboundTemporalMode,
        target: InboundPropTarget,
    ) -> syn::Result<()> {
        let field = self.ensure_inbound_field(&field_name, InboundFieldRole::Value, field_span)?;
        let value = self.inbound_inputs.fields[field as usize]
            .value
            .expect("value fields always allocate one local value");
        let crossing = self.inbound_inputs.fields[field as usize].crossing;
        let id = dense_input_id(
            self.inbound_inputs.prop_sites.len(),
            "inbound prop-use sites",
            field_span,
        )?;
        self.inbound_inputs.prop_sites.push(InboundPropSite {
            id,
            field,
            value,
            crossing,
            temporal,
            target,
            source_anchor: rsjs_source_anchor(field_span),
        });
        Ok(())
    }

    fn ensure_inbound_field(
        &mut self,
        field_name: &str,
        role: InboundFieldRole,
        field_span: proc_macro2::Span,
    ) -> syn::Result<u32> {
        if let Some(existing_index) = self
            .inbound_inputs
            .fields
            .iter()
            .position(|field| field.name == field_name)
        {
            let existing = &mut self.inbound_inputs.fields[existing_index];
            if existing.role == InboundFieldRole::DeferredProjection
                && role != InboundFieldRole::DeferredProjection
            {
                existing.role = role;
                self.inbound_inputs.crossings[existing.crossing as usize].kind =
                    InboundCrossingKind::for_role(role);
            } else if role != InboundFieldRole::DeferredProjection && existing.role != role {
                return Err(syn::Error::new(
                    field_span,
                    format!(
                        "inbound component field `self.{field_name}` has conflicting client \
                         roles: it was first inferred as `{}` but this use requires `{}`\n\
                         help: use distinct fields for value props and compiler-owned callback \
                         slots; callback fields must be invoked as `self.on_*.call(payload)`",
                        existing.role.as_str(),
                        role.as_str(),
                    ),
                ));
            }
            return Ok(existing.id);
        }

        let id = dense_input_id(
            self.inbound_inputs.fields.len(),
            "inbound component fields",
            field_span,
        )?;
        let crossing = dense_input_id(
            self.inbound_inputs.crossings.len(),
            "inbound crossing hooks",
            field_span,
        )?;
        let source_anchor = rsjs_source_anchor(field_span);
        // Every client-visible inbound field owns a symbolic value. Callback
        // fields need one just as much as value props do: the linked graph
        // connects the parent's current callback value to this child-local
        // node so callback forwarding never degrades into an SSR snapshot.
        let value = dense_input_id(
            self.inbound_inputs.values.len(),
            "inbound component values",
            field_span,
        )?;
        self.inbound_inputs.values.push(InboundValue {
            id: value,
            field: id,
            source_anchor,
        });
        let kind = InboundCrossingKind::for_role(role);
        self.inbound_inputs.crossings.push(InboundCrossing {
            id: crossing,
            field: id,
            kind,
            source_anchor,
        });
        self.inbound_inputs.fields.push(InboundField {
            id,
            name: field_name.to_owned(),
            role,
            value: Some(value),
            callback_shape: None,
            crossing,
            source_anchor,
        });
        Ok(id)
    }
}

fn dense_input_id(len: usize, collection: &str, span: proc_macro2::Span) -> syn::Result<u32> {
    u32::try_from(len).map_err(|_| {
        syn::Error::new(
            span,
            format!("an analyzed RSJS component cannot contain more than 2^32 {collection}"),
        )
    })
}

#[derive(Debug, Clone)]
struct UnsupportedClientPreludeLocal {
    dependency_name: String,
}

#[derive(Clone)]
struct LoopScope {
    locals: HashSet<String>,
    /// Exact fixed-width domains proven by static iterator syntax for a
    /// subset of `locals`.
    ///
    /// This remains separate from the name set because an inner untyped
    /// binding must still shadow an outer typed one, and most server
    /// iterators provide no client-representation proof to the proc macro.
    integer_domains: HashMap<String, IntegerDomain>,
}

struct SignalLocal {
    name: syn::Ident,
    source_anchor: rsjs_ir::SourceAnchor,
    fallback: Value,
    runtime_expr: Option<Expr>,
    explicit_ty: Option<syn::Type>,
    integer_source: Option<IntegerSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IntegerSource {
    Supported(IntegerDomain),
    Unsupported(String),
}

fn merge_range_integer_sources(
    lhs: Option<IntegerSource>,
    rhs: Option<IntegerSource>,
    span: proc_macro2::Span,
) -> syn::Result<Option<IntegerSource>> {
    match (lhs, rhs) {
        (Some(IntegerSource::Supported(lhs)), Some(IntegerSource::Supported(rhs)))
            if lhs != rhs =>
        {
            Err(syn::Error::new(
                span,
                format!(
                    "rsjs/integer-range-domain-mismatch: range bounds use `{}` and `{}`",
                    lhs.as_str(),
                    rhs.as_str()
                ),
            ))
        }
        (Some(IntegerSource::Unsupported(lhs)), Some(IntegerSource::Unsupported(rhs)))
            if lhs != rhs =>
        {
            Err(syn::Error::new(
                span,
                format!("rsjs/integer-range-domain-mismatch: range bounds use `{lhs}` and `{rhs}`"),
            ))
        }
        (Some(IntegerSource::Unsupported(name)), Some(IntegerSource::Supported(domain)))
        | (Some(IntegerSource::Supported(domain)), Some(IntegerSource::Unsupported(name))) => {
            Err(syn::Error::new(
                span,
                format!(
                    "rsjs/integer-range-domain-mismatch: range bounds use `{name}` and `{}`",
                    domain.as_str()
                ),
            ))
        }
        (Some(source), Some(_)) | (Some(source), None) | (None, Some(source)) => Ok(Some(source)),
        (None, None) => Ok(None),
    }
}

pub(crate) struct AnalysisOutput {
    pub(crate) island: Island,
    pub(crate) hir_sidecar: HirSidecar,
    pub(crate) signal_initializers: Vec<RuntimeSignalInitializer>,
    pub(crate) query_initializers: Vec<QueryInitializer>,
    pub(crate) mutation_initializers: Vec<MutationInitializer>,
    pub(crate) shared_state_initializers: Vec<SharedStateInitializer>,
    /// Exact lexical source binding -> signal/shared-field id mapping.
    pub(crate) prelude_signal_ids: HashMap<usize, SignalId>,
    /// Exact lexical source acquisition -> shared-store id mapping.
    pub(crate) prelude_shared_store_ids: HashMap<usize, SharedStoreId>,
    /// Exact lexical source declaration -> query id mapping.
    pub(crate) prelude_query_ids: HashMap<usize, QueryId>,
    /// Exact lexical source declaration -> mutation id mapping.
    pub(crate) prelude_mutation_ids: HashMap<usize, MutationId>,
    /// Exact lexical source declaration -> node-ref id mapping.
    ///
    /// Most node-ref declarations disappear from the server program. A ref
    /// passed through a direct component literal remains a real zero-sized
    /// Rust field value during SSR, so the lossless rewriter uses this map to
    /// retain only those required declarations.
    pub(crate) prelude_node_ref_ids: HashMap<usize, NodeRefId>,
    /// Prelude positions consumed by SharedState acquisition/field lowering.
    pub(crate) shared_prelude_local_indices: HashSet<usize>,
    /// Prelude positions consumed by client-alias lowering.
    pub(crate) client_prelude_local_indices: HashSet<usize>,
    /// Direct component syntax edges retained for relocation-record emission.
    pub(crate) direct_component_edges: Vec<DirectComponentSourceEdge>,
    /// Static wrapperless SharedState provider topology in source order.
    pub(crate) shared_state_provider_sites: Vec<DirectSharedStateProviderSite>,
    /// Direct client-helper calls retained for relocation-record emission.
    pub(crate) direct_helper_edges: Vec<DirectHelperSourceEdge>,
    /// Exact local source expressions for opaque/server-only view nodes.
    pub(crate) server_expressions: HashMap<NodeId, Expr>,
    /// Exact local view node -> direct component edge ID.
    pub(crate) direct_component_site_nodes: HashMap<NodeId, u32>,
    /// Dense latent component-input records for descriptor schema v9.
    pub(crate) inbound_inputs: InboundInputMetadata,
    /// Narrow open-generic field whose exact Rust type is validated by the
    /// generated expansion and whose descriptor role is an immutable slot.
    pub(crate) generic_static_slot: Option<GenericStaticSlotField>,
    /// Exact browser callback values referenced by direct live-component
    /// fields, ordered by authored prelude binding.
    pub(crate) direct_payload_handler_values: Vec<DirectPayloadHandlerValue>,
}

#[derive(Clone, Debug)]
pub(crate) struct GenericStaticSlotField {
    pub(crate) name: String,
    pub(crate) ty: syn::Type,
}

/// One materialized `rsjs::handler(...)` callback value selected by a direct
/// component edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectPayloadHandlerValue {
    /// Exact source `let` position used by the direct binding record.
    pub(crate) prelude_local_index: u32,
    /// Authored payload arity (`0` or `1`).
    pub(crate) payload_arity: u8,
    /// Redacted source location of the handler constructor.
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
    /// Closed-over client graph dependencies, derived from `body_ir`.
    pub(crate) dependencies: DirectPayloadHandlerDependencies,
    /// Client IR with a one-argument handler parameter represented by
    /// `ClientExpr::CallbackPayload`.
    pub(crate) body_ir: ClientBlock,
}

/// Deterministic external references made by a materialized callback body.
///
/// Read and write signal sets stay separate because writes are graph effects,
/// not reactive subscriptions. Every vector uses first-encounter order and is
/// deduplicated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirectPayloadHandlerDependencies {
    pub(crate) signal_reads: Vec<SignalId>,
    pub(crate) signal_writes: Vec<SignalId>,
    pub(crate) queries: Vec<QueryId>,
    pub(crate) mutations: Vec<MutationId>,
    pub(crate) node_refs: Vec<NodeRefId>,
    pub(crate) callback_slots: Vec<CallbackSlotId>,
    pub(crate) helper_edges: Vec<HelperCallEdgeId>,
    pub(crate) client_module_calls: Vec<DirectClientModuleCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectClientModuleCall {
    pub(crate) module: String,
    pub(crate) name: String,
}

fn direct_payload_handler_dependencies(
    block: &ClientBlock,
) -> syn::Result<DirectPayloadHandlerDependencies> {
    let dependencies = rsjs_ir::collect_client_block_external_dependencies(block).map_err(
        |error| match error {
            rsjs_ir::ClientBlockDependencyError::LocalFunction(_) => syn::Error::new(
                proc_macro2::Span::call_site(),
                "rsjs/direct-live-callback-local-function: a materialized component callback \
                 cannot reference a component-local client function without transporting that \
                 function definition",
            ),
            rsjs_ir::ClientBlockDependencyError::UnresolvedFunction(name) => syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "rsjs/direct-live-callback-unresolved-function: callback body still contains \
                     unresolved client function `{name}`"
                ),
            ),
        },
    )?;

    Ok(DirectPayloadHandlerDependencies {
        signal_reads: dependencies.signal_reads,
        signal_writes: dependencies.signal_writes,
        queries: dependencies.queries,
        mutations: dependencies.mutations,
        node_refs: dependencies.node_refs,
        callback_slots: dependencies.callback_slots,
        helper_edges: dependencies.helper_edges,
        client_module_calls: dependencies
            .client_module_calls
            .into_iter()
            .map(|call| DirectClientModuleCall {
                module: call.module,
                name: call.name,
            })
            .collect(),
    })
}

/// Descriptor-only metadata for inbound component values and callbacks.
///
/// This deliberately does not live in [`Island`]. An inbound field can be
/// completely latent in a standalone server render, while a linked parent may
/// later specialize one of its recorded use sites.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InboundInputMetadata {
    pub(crate) fields: Vec<InboundField>,
    pub(crate) values: Vec<InboundValue>,
    pub(crate) prop_sites: Vec<InboundPropSite>,
    pub(crate) crossings: Vec<InboundCrossing>,
    /// Exact child-local node-ref identity attached to inbound handle fields.
    pub(crate) node_ref_fields: Vec<InboundNodeRefField>,
    /// Type-ambiguous direct `self.<field>` projections retained until the
    /// relocation-linked component graph resolves their browser role.
    pub(crate) projection_fields: Vec<InboundProjectionField>,
}

/// Remove inbound signal-seed sites whose local signal never reaches browser
/// work, then compact every coupled ID space in source order.
///
/// A dead `signal(self.initial)` remains ordinary server Rust. Keeping it in
/// the client ABI would leave a prop site pointing at a signal removed by the
/// normal IR passes and could incorrectly fuse an otherwise server-only child
/// into its parent's owner. Callback fields and explicit-island boundary
/// checks are not signal seeds and remain part of the latent contract.
pub(crate) fn prune_dead_signal_seed_inputs(
    inputs: &mut InboundInputMetadata,
    reachable_signals: &[SignalId],
) {
    let reachable_signals = reachable_signals.iter().copied().collect::<HashSet<_>>();
    inputs.prop_sites.retain(|site| match site.target {
        InboundPropTarget::LiveSignal(signal) | InboundPropTarget::SignalSeed(signal) => {
            reachable_signals.contains(&signal)
        }
        InboundPropTarget::ExplicitIslandBoundary | InboundPropTarget::SharedStateProviderSeed => {
            true
        }
    });

    let fields_with_value_sites = inputs
        .prop_sites
        .iter()
        .map(|site| site.field)
        .collect::<HashSet<_>>();
    let old_fields = std::mem::take(&mut inputs.fields);
    let old_values = std::mem::take(&mut inputs.values);
    let old_crossings = std::mem::take(&mut inputs.crossings);
    let old_node_ref_fields = std::mem::take(&mut inputs.node_ref_fields);
    let old_projection_fields = std::mem::take(&mut inputs.projection_fields);
    let projection_field_ids = old_projection_fields
        .iter()
        .map(|projection| projection.field)
        .collect::<HashSet<_>>();
    let mut field_remap = HashMap::<u32, (u32, u32, u32)>::new();

    for mut field in old_fields {
        if field.role == InboundFieldRole::Value
            && !fields_with_value_sites.contains(&field.id)
            && !projection_field_ids.contains(&field.id)
        {
            continue;
        }

        let old_field = field.id;
        let old_value = field
            .value
            .expect("every client-visible inbound field owns a symbolic value");
        let old_crossing = field.crossing;
        let value_source_anchor = old_values
            .iter()
            .find(|value| value.id == old_value && value.field == old_field)
            .map(|value| value.source_anchor)
            .expect("inbound field value metadata must be internally consistent");
        let crossing = old_crossings
            .iter()
            .find(|crossing| crossing.id == old_crossing && crossing.field == old_field)
            .expect("inbound field crossing metadata must be internally consistent");

        let new_field = inputs.fields.len() as u32;
        let new_value = inputs.values.len() as u32;
        let new_crossing = inputs.crossings.len() as u32;
        field.id = new_field;
        field.value = Some(new_value);
        field.crossing = new_crossing;
        inputs.values.push(InboundValue {
            id: new_value,
            field: new_field,
            source_anchor: value_source_anchor,
        });
        inputs.crossings.push(InboundCrossing {
            id: new_crossing,
            field: new_field,
            kind: crossing.kind,
            source_anchor: crossing.source_anchor,
        });
        inputs.fields.push(field);
        field_remap.insert(old_field, (new_field, new_value, new_crossing));
    }

    for mut node_ref in old_node_ref_fields {
        let (field, value, crossing) = field_remap
            .get(&node_ref.field)
            .copied()
            .expect("retained inbound node-ref field must retain its field");
        node_ref.field = field;
        node_ref.value = value;
        node_ref.crossing = crossing;
        inputs.node_ref_fields.push(node_ref);
    }

    for mut projection in old_projection_fields {
        let (field, value, crossing) = field_remap
            .get(&projection.field)
            .copied()
            .expect("retained deferred projection must retain its field");
        projection.field = field;
        projection.value = value;
        projection.crossing = crossing;
        inputs.projection_fields.push(projection);
    }

    for (index, site) in inputs.prop_sites.iter_mut().enumerate() {
        let (field, value, crossing) = field_remap
            .get(&site.field)
            .copied()
            .expect("retained inbound prop site must retain its field");
        site.id = index as u32;
        site.field = field;
        site.value = value;
        site.crossing = crossing;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboundField {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) role: InboundFieldRole,
    pub(crate) value: Option<u32>,
    pub(crate) callback_shape: Option<InboundCallbackShape>,
    pub(crate) crossing: u32,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundFieldRole {
    Value,
    Callback,
    NodeRef,
    DeferredProjection,
}

impl InboundFieldRole {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Callback => "callback",
            Self::NodeRef => "node_ref",
            Self::DeferredProjection => "deferred_projection",
        }
    }
}

/// A raw direct projection whose concrete role is fixed by the linked child
/// field rather than guessed from path tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboundProjectionField {
    pub(crate) field: u32,
    pub(crate) value: u32,
    pub(crate) crossing: u32,
    pub(crate) signal: SignalId,
    pub(crate) node_ref: NodeRefId,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

/// One required inbound node-ref handle.
///
/// `node_ref` is the child-local legacy IR identity that final linked
/// composition must override with the parent's exact ref object. A binding
/// target is present when this component owns the unique `ref={...}` use in
/// the alias chain; read-only/pass-through participants leave it absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboundNodeRefField {
    pub(crate) field: u32,
    pub(crate) value: u32,
    pub(crate) crossing: u32,
    pub(crate) node_ref: NodeRefId,
    pub(crate) bind_target: Option<NodeRefBindTarget>,
    /// Exact generated marker used only by the current producer's rustc
    /// witness. The stable descriptor retains the coarser runtime family.
    pub(crate) exact_bind_target: Option<rsjs_ir::dom_registry::DomElementKind>,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

/// Element family used to rustc-check an inbound `NodeRef<T>` binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeRefBindTarget {
    Element,
    HtmlElement,
    Input,
    TextArea,
    Select,
    Form,
    Dialog,
}

impl NodeRefBindTarget {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Element => "element",
            Self::HtmlElement => "html_element",
            Self::Input => "input",
            Self::TextArea => "textarea",
            Self::Select => "select",
            Self::Form => "form",
            Self::Dialog => "dialog",
        }
    }

    pub(crate) fn for_tag(tag: &str) -> Self {
        use rsjs_ir::dom_registry::DomElementKind;

        match rsjs_ir::dom_registry::find_dom_tag(tag).map(|spec| spec.element) {
            Some(DomElementKind::Element) => Self::Element,
            Some(DomElementKind::HtmlInputElement) => Self::Input,
            Some(DomElementKind::HtmlTextAreaElement) => Self::TextArea,
            Some(DomElementKind::HtmlSelectElement) => Self::Select,
            Some(DomElementKind::HtmlFormElement) => Self::Form,
            Some(DomElementKind::HtmlDialogElement) => Self::Dialog,
            _ => Self::HtmlElement,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundCallbackShape {
    Required,
    Optional,
}

impl InboundCallbackShape {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboundValue {
    pub(crate) id: u32,
    pub(crate) field: u32,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboundPropSite {
    pub(crate) id: u32,
    pub(crate) field: u32,
    pub(crate) value: u32,
    pub(crate) crossing: u32,
    pub(crate) temporal: InboundTemporalMode,
    pub(crate) target: InboundPropTarget,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundTemporalMode {
    LiveRead,
    SeedOnce,
    QueryKeyDependency,
    ForbiddenProviderSeed,
    ForbiddenBoundaryCrossing,
}

impl InboundTemporalMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LiveRead => "live_read",
            Self::SeedOnce => "seed_once",
            Self::QueryKeyDependency => "query_key_dependency",
            Self::ForbiddenProviderSeed => "forbidden_provider_seed",
            Self::ForbiddenBoundaryCrossing => "forbidden_boundary_crossing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundPropTarget {
    LiveSignal(SignalId),
    SignalSeed(SignalId),
    SharedStateProviderSeed,
    ExplicitIslandBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboundCrossing {
    pub(crate) id: u32,
    pub(crate) field: u32,
    pub(crate) kind: InboundCrossingKind,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboundCrossingKind {
    Value,
    CallbackResolveCurrent,
    NodeRefAlias,
    DeferredProjection,
}

impl InboundCrossingKind {
    pub(crate) const fn for_role(role: InboundFieldRole) -> Self {
        match role {
            InboundFieldRole::Value => Self::Value,
            InboundFieldRole::Callback => Self::CallbackResolveCurrent,
            InboundFieldRole::NodeRef => Self::NodeRefAlias,
            InboundFieldRole::DeferredProjection => Self::DeferredProjection,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::CallbackResolveCurrent => "callback_resolve_current",
            Self::NodeRefAlias => "node_ref_alias",
            Self::DeferredProjection => "deferred_projection",
        }
    }
}

/// One direct struct-literal component edge in an analyzed template.
///
/// `local_id` is dense and deterministic within the authored component. It is
/// deliberately independent of path tokens: the emitted relocation, not the
/// spelling stored here, is the component identity.
#[derive(Clone)]
pub(crate) struct DirectComponentSourceEdge {
    pub(crate) local_id: u32,
    pub(crate) component_ty: syn::Path,
    pub(crate) fields: Vec<DirectComponentSourceField>,
    pub(crate) has_rest: bool,
    pub(crate) boundary: DirectComponentBoundary,
    pub(crate) lifecycle: DirectComponentLifecycle,
    /// Exact schema-v9 view node that renders this component occurrence.
    ///
    /// It is populated when the preserved server expression is attached to
    /// the view tree. Transport requires it so the adapter can prove a
    /// conditional lifecycle's control and branch polarity against the
    /// descriptor topology.
    pub(crate) site_node: Option<NodeId>,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

/// One compiler-recognized static `rsjs::SharedStateProvider` declaration.
///
/// `site_node` names the server node that preserves the complete provider
/// expression. `child_edge` names the ordinary composed component edge for
/// the provider's direct child; both deliberately refer to the same node so a
/// later final server pass can replace only `children` without re-evaluating
/// or wrapping the provider.
#[derive(Debug, Clone)]
pub(crate) struct DirectSharedStateProviderSite {
    pub(crate) id: rsjs_ir::abi::ProviderSiteId,
    pub(crate) state_ty: syn::Path,
    pub(crate) child_edge: u32,
    pub(crate) site_node: NodeId,
    pub(crate) source_anchor: rsjs_ir::SourceAnchor,
}

/// One authored field assignment and the strongest source classification the
/// tolerant frontend can prove without Rust type information.
#[derive(Clone)]
pub(crate) struct DirectComponentSourceField {
    pub(crate) name: String,
    pub(crate) binding: DirectComponentBindingClassification,
}

/// Parent-side binding information carried into the canonical linker.
///
/// `ServerOnly` is a negative syntax/dataflow proof: the expression contains
/// no compiler-owned client value or browser operation. The child descriptor
/// must independently declare the resolved field server-only before semantic
/// linking accepts the edge. On an explicit-island edge, a direct
/// `self.field` also uses this concrete binding while a separate inbound
/// `ForbiddenBoundaryCrossing` site guards its provenance: static callers are
/// valid, but a linked live caller is rejected rather than snapshotted.
///
/// `LiveValue` is intentionally limited to one exact parent signal cell. A
/// direct `.get()` and a compiler-proven alias of exactly that read carry the
/// same legacy `SignalId`; derived expressions remain unsupported until the
/// parent descriptor exposes a canonical symbolic value for them.
///
/// `LiveCallback` carries the exact prelude binding that declared an
/// `rsjs::handler(...)`. That transport-local binding index is not a
/// `LocalValueId`: the semantic adapter must form the callback graph value and
/// prove its relationship to the handler body. `rsjs::event_handler(...)`
/// declarations never enter this classification.
///
/// `StaticSlotExactOne` is deliberately narrower. It is emitted only for a
/// direct `html_sync!` / `html!` field value on an explicit island edge whose
/// body is one fully static root element. That syntax proves an immutable
/// one-node SSR range without inspecting a callee body, guessing a Rust type,
/// or allowing an unplanned nested owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectComponentBindingClassification {
    ServerOnly,
    LiveValue {
        signal: SignalId,
        source: DirectLiveValueSource,
    },
    StaticCallback {
        form: DirectStaticCallbackForm,
    },
    LiveCallback {
        handler_prelude_local: u32,
        form: DirectLiveCallbackForm,
    },
    NodeRefHandle {
        node_ref: NodeRefId,
    },
    DeferredProjection {
        field: u32,
        signal: SignalId,
        node_ref: NodeRefId,
    },
    StaticSlotExactOne,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectLiveValueSource {
    SignalRead,
    ClientAlias,
    InboundField,
}

impl DirectLiveValueSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SignalRead => "direct_signal_read",
            Self::ClientAlias => "client_alias",
            Self::InboundField => "inbound_field",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectLiveCallbackForm {
    Handler,
    SomeHandler,
}

impl DirectLiveCallbackForm {
    pub(crate) const fn callback_shape(self) -> &'static str {
        match self {
            Self::Handler => "required",
            Self::SomeHandler => "optional",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectStaticCallbackForm {
    None,
    NoopHandler,
    SomeNoopHandler,
}

impl DirectStaticCallbackForm {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NoopHandler => "noop_handler",
            Self::SomeNoopHandler => "some_noop_handler",
        }
    }

    pub(crate) const fn callback_shape(self) -> &'static str {
        match self {
            Self::None | Self::SomeNoopHandler => "optional",
            Self::NoopHandler => "required",
        }
    }
}

impl DirectComponentBindingClassification {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ServerOnly => "server_only",
            Self::LiveValue { .. } => "live_value",
            Self::StaticCallback { .. } => "static_callback",
            Self::LiveCallback { .. } => "live_callback",
            Self::NodeRefHandle { .. } => "node_ref_handle",
            Self::DeferredProjection { .. } => "deferred_projection",
            Self::StaticSlotExactOne => "immutable_static_slot_exact_one",
            Self::Unsupported => "unsupported",
        }
    }

    pub(crate) const fn proof(self) -> &'static str {
        match self {
            Self::ServerOnly => "visible_syntax_has_no_client_dependency_v1",
            Self::LiveValue {
                source: DirectLiveValueSource::SignalRead,
                ..
            } => "exact_signal_read_v1",
            Self::LiveValue {
                source: DirectLiveValueSource::ClientAlias,
                ..
            } => "client_alias_resolves_to_exact_signal_read_v1",
            Self::LiveValue {
                source: DirectLiveValueSource::InboundField,
                ..
            } => "inbound_live_field_reuses_exact_live_signal_v1",
            Self::StaticCallback {
                form: DirectStaticCallbackForm::None,
            } => "literal_none_callback_candidate_v1",
            Self::StaticCallback {
                form: DirectStaticCallbackForm::NoopHandler,
            } => "rsjs_noop_handler_v1",
            Self::StaticCallback {
                form: DirectStaticCallbackForm::SomeNoopHandler,
            } => "some_rsjs_noop_handler_v1",
            Self::LiveCallback {
                form: DirectLiveCallbackForm::Handler,
                ..
            } => "payload_handler_prelude_binding_v1",
            Self::LiveCallback {
                form: DirectLiveCallbackForm::SomeHandler,
                ..
            } => "some_payload_handler_prelude_binding_v1",
            Self::NodeRefHandle { .. } => "exact_node_ref_handle_alias_v1",
            Self::DeferredProjection { .. } => {
                "rustc_linked_child_field_resolves_exact_projection_role_v1"
            }
            Self::StaticSlotExactOne => "direct_fully_static_single_root_html_macro_v1",
            Self::Unsupported => "not_proven",
        }
    }
}

/// One branch of a schema-v9 conditional control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectConditionalBranch {
    Then,
    Else,
}

/// Analyzer-local locator for one conditional control.
///
/// The descriptor serializer canonically orders conditionals by parent node
/// and then by this per-parent section ordinal. Transport resolves this pair
/// to the dense canonical control ID after analysis is complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirectConditionalLifecycleScope {
    pub(crate) parent: NodeId,
    pub(crate) section_ordinal: u32,
    pub(crate) branch: DirectConditionalBranch,
}

/// Runtime multiplicity/lifecycle information for one direct syntax site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectComponentLifecycle {
    /// One statically present syntax site outside a template loop or reactive
    /// structural region.
    StaticSingle,
    /// One occurrence governed by exactly one canonical conditional section.
    ConditionalSingle(DirectConditionalLifecycleScope),
    /// An occurrence inside a template loop.
    Repeated,
    /// An occurrence governed by multiple nested structural controls.
    UnknownNestedConditionals,
}

#[derive(Clone, Copy)]
pub(crate) enum DirectComponentBoundary {
    Composed,
    ExplicitIsland,
}

impl DirectComponentBoundary {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Composed => "composed",
            Self::ExplicitIsland => "explicit_island",
        }
    }
}

/// One direct `#[rsjs::client]` helper call in analyzed source.
///
/// `helper_path` deliberately keeps the authored Rust path rather than a
/// token-derived identity. The generated relocation lets rustc resolve imports,
/// aliases, and re-exports to the helper's defining function symbol.
#[derive(Clone)]
pub(crate) struct DirectHelperSourceEdge {
    pub(crate) local_id: u32,
    pub(crate) helper_path: syn::Path,
    /// Number of authored arguments. The generated Rust witness uses this to
    /// validate a contextually inferred return domain without reconstructing
    /// parameter types in the proc macro.
    pub(crate) argument_count: usize,
    /// Fixed-width return domain inferred from an enclosing typed Rust
    /// boundary (for example, `u32_signal.set(helper(...))`). `None` keeps the
    /// helper result intentionally unknown until it reaches such a boundary.
    pub(crate) expected_return_domain: Option<IntegerDomain>,
}

#[derive(Clone)]
pub(crate) struct RuntimeSignalInitializer {
    pub(crate) id: SignalId,
    pub(crate) expr: Expr,
    pub(crate) explicit_ty: Option<syn::Type>,
}

#[derive(Clone)]
pub(crate) struct SharedStateInitializer {
    pub(crate) id: SharedStoreId,
    pub(crate) source_name: syn::Ident,
    pub(crate) ty: syn::Type,
    /// See `emit_rust::SharedStateInitializer::expected_type_key`.
    pub(crate) expected_type_key: Option<proc_macro2::TokenStream>,
}

#[derive(Clone)]
pub(crate) struct QueryInitializer {
    pub(crate) id: QueryId,
    pub(crate) expr: Expr,
    pub(crate) key_expr: Option<Expr>,
}

#[derive(Clone)]
pub(crate) struct MutationInitializer {
    pub(crate) id: MutationId,
    pub(crate) key_expr: Expr,
    pub(crate) invalidates_expr: Option<Expr>,
}

#[derive(Default, Clone)]
pub(crate) struct HirSidecar {
    handlers: Vec<HandlerHirArtifact>,
    intervals: Vec<IntervalHirArtifact>,
    component_fragment_mounts: Vec<ComponentFragmentMountHirArtifact>,
    lists: Vec<ListHirArtifact>,
}

/// A keyed list recorded during the walk, re-checked after the whole
/// tree is analysed: the loop must be its container's only child and
/// the container must not also carry a text binding, because the
/// runtime's `bind_list` owns every element child of the container.
#[derive(Clone)]
struct KeyedListGuard {
    container: NodeId,
    loop_node: NodeId,
    span: proc_macro2::Span,
}

struct QueryListIterator {
    expr: ExprId,
    ssr_iter: String,
}

#[derive(Clone)]
struct ListHirArtifact {
    container: NodeId,
    source: ListHirSource,
    key: String,
}

#[derive(Clone)]
enum ListHirSource {
    Signal(SignalId),
    Expr(ExprId),
}

#[derive(Clone)]
struct HandlerHirArtifact {
    id: EventHandlerId,
    body: Vec<HandlerHirStmt>,
}

#[derive(Clone)]
struct IntervalHirArtifact {
    ms: u64,
    body: Vec<HandlerHirStmt>,
}

#[derive(Clone)]
struct ComponentFragmentMountHirArtifact {
    root: NodeId,
    asset_id: String,
    callbacks: Vec<ComponentFragmentCallbackHirArtifact>,
}

#[derive(Clone)]
struct ComponentFragmentCallbackHirArtifact {
    slot: CallbackSlotId,
    body: Vec<HandlerHirStmt>,
}

#[derive(Clone)]
struct HandlerLocal {
    name: syn::Ident,
    closure: ExprClosure,
    kind: HandlerLocalKind,
    /// Exact authored prelude-`let` position for transport provenance.
    ///
    /// This is deliberately not a canonical `LocalValueId`. Signals already
    /// have a stable legacy `SignalId`, but payload handlers are source
    /// declarations whose client-value nodes are formed only by the semantic
    /// linker. Carrying the lexical binding index lets that linker allocate
    /// and deduplicate the eventual callback value without guessing from the
    /// local name or source tokens.
    prelude_local_index: u32,
    source_anchor: rsjs_ir::SourceAnchor,
    /// Client bindings visible while this handler initializer was evaluated.
    ///
    /// Named handlers lower later, at a template event or component callback
    /// use. Retaining the declaration-site environment prevents subsequent
    /// same-named declarations from changing the closure's lexical meaning.
    /// The environment is captured before this handler is inserted, so the
    /// analyzer does not invent self-recursion.
    capture_env: Option<std::sync::Arc<PreludeClientBindingEnv>>,
}

#[derive(Clone)]
struct PreludeClientBindingEnv {
    signals: HashMap<String, SignalId>,
    queries: HashMap<String, QueryId>,
    mutations: HashMap<String, MutationId>,
    node_refs: HashMap<String, NodeRefId>,
    handlers: HashMap<String, HandlerLocal>,
    client_prelude_locals: HashMap<String, FrontendExpr>,
    unsupported_client_prelude_locals: HashMap<String, UnsupportedClientPreludeLocal>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HandlerLocalKind {
    Payload,
    Event,
}

#[derive(Clone)]
struct HandlerLocalBinding {
    name: String,
    id: LocalId,
    mutable: bool,
    /// Client value shape of the binding, computed from its
    /// initializer by the whole-island typing pass
    /// ([`rsjs_ir::analysis::typing`]) at `let` lowering time. Drives
    /// truncating-division selection and BigInt literal coercion for
    /// later reads of the local, and annotates the emitted
    /// [`ClientStmt::Let`].
    ty: ClientType,
    /// Rust fixed-width arithmetic domain, separate from `ty`'s browser
    /// representation. `None` means the source domain is unproven.
    integer_domain: Option<IntegerDomain>,
    /// Explicit unsupported source width (`u8`, `usize`, ...), retained for a
    /// targeted diagnostic if this local participates in arithmetic.
    unsupported_integer_domain: Option<String>,
    /// Immutable literal-only initializer eligible for source-level constant
    /// propagation. This preserves Rust contextual integer inference for
    /// writes such as `let one = 1; wide.set(one)`: the write target can
    /// promote the literal to the wide domain before Client IR is frozen.
    constant: Option<FrontendExpr>,
}

struct HandlerLowering {
    body_ir: ClientBlock,
    body_hir: Vec<HandlerHirStmt>,
    /// Trailing-edge debounce window from an
    /// `on_*={rsjs::debounce(|event| ..., delay_ms)}` attribute value, or
    /// `None` for an ordinary handler.
    debounce_ms: Option<u64>,
    source_name: Option<String>,
    source_anchor: Option<rsjs_ir::SourceAnchor>,
}

#[derive(Default, Clone)]
struct ComponentInlineMaps {
    shared_stores: HashMap<SharedStoreId, SharedStoreId>,
    signals: HashMap<SignalId, SignalId>,
    prop_signals: HashMap<SignalId, FrontendExpr>,
    prop_signal_source_anchors: HashMap<SignalId, rsjs_ir::SourceAnchor>,
    server_props: HashMap<String, Expr>,
    exprs: HashMap<ExprId, ExprId>,
    queries: HashMap<QueryId, QueryId>,
    mutations: HashMap<MutationId, MutationId>,
    handlers: HashMap<EventHandlerId, EventHandlerId>,
    node_refs: HashMap<NodeRefId, NodeRefId>,
    nodes: HashMap<NodeId, NodeId>,
    locals: HashMap<LocalId, LocalId>,
    callback_handlers: HashMap<CallbackSlotId, Option<HandlerLocal>>,
}

struct ComponentInlineProps {
    callbacks: HashMap<String, Option<HandlerLocal>>,
    props: HashMap<String, ComponentInlineProp>,
    server_props: HashMap<String, Expr>,
}

#[derive(Clone)]
struct ComponentInlineProp {
    expr: FrontendExpr,
    source_anchor: rsjs_ir::SourceAnchor,
}

struct ComponentFragmentMountPlan {
    callbacks: Vec<ComponentFragmentCallback>,
    callback_hirs: Vec<ComponentFragmentCallbackHirArtifact>,
    signal_aliases: Vec<ComponentFragmentSignalAlias>,
}

fn guided_boundary_error<I, S>(
    span: proc_macro2::Span,
    what: impl AsRef<str>,
    why: impl AsRef<str>,
    fixes: I,
    tracked_feature: Option<&str>,
) -> syn::Error
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut message = String::new();
    message.push_str("rsjs boundary: ");
    message.push_str(what.as_ref());
    message.push_str("\n\nWhy:\n  ");
    message.push_str(why.as_ref());
    message.push_str("\n\nUse one of:");
    for fix in fixes {
        message.push_str("\n  - ");
        message.push_str(fix.as_ref());
    }
    if let Some(feature) = tracked_feature {
        message.push_str("\n\nTracked feature:\n  ");
        message.push_str(feature);
    }
    syn::Error::new(span, message)
}

/// Lexical scope used while lowering reactive expressions.
///
/// v0.1's only scoped binding is the closure parameter inside
/// `signal.update(|c| <body>)`: that parameter aliases the receiver
/// signal's current value. References to the parameter inside the
/// body lower to a `SignalRead` of the receiver, not to a name lookup
/// in the prelude `signals` map.
#[derive(Default, Clone, Copy)]
struct ExprScope<'a> {
    closure_param: Option<(&'a str, SignalId)>,
    inline_param: Option<(&'a str, &'a FrontendExpr)>,
    handler_param: Option<(&'a str, &'a FrontendExpr)>,
    event_param: Option<&'a str>,
    handler_locals: &'a [HandlerLocalBinding],
    node_payload_target: Option<NodeId>,
}

fn shared_signal_receiver_error(span: proc_macro2::Span, method: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "`#[rsjs]` expects `.{method}` on a signal local or a typed shared-state field, \
             e.g. `count.{method}(...)` or `workbench.button_bg().{method}(...)`"
        ),
    )
}

fn rsjs_source_anchor(span: proc_macro2::Span) -> rsjs_ir::SourceAnchor {
    let start = span.start();
    rsjs_ir::SourceAnchor {
        start_line: start.line,
        start_column: start.column + 1,
    }
}

/// Recognize the existing component-prop signal seed grammar while retaining
/// the field's real source span for schema-v9 diagnostics.
pub(crate) fn inbound_signal_self_field(expr: &Expr) -> Option<(String, proc_macro2::Span)> {
    match expr {
        Expr::Field(field) if expr_is_self_path(&field.base) => match &field.member {
            syn::Member::Named(ident) => Some((ident.to_string(), ident.span())),
            syn::Member::Unnamed(_) => None,
        },
        Expr::MethodCall(call) if call.method == "clone" && call.args.is_empty() => {
            inbound_signal_self_field(&call.receiver)
        }
        Expr::MethodCall(call) if call.method == "unwrap_or" && call.args.len() == 1 => {
            inbound_signal_self_field(&call.receiver)
        }
        Expr::Paren(paren) => inbound_signal_self_field(&paren.expr),
        Expr::Group(group) => inbound_signal_self_field(&group.expr),
        Expr::Reference(reference) => inbound_signal_self_field(&reference.expr),
        _ => None,
    }
}

/// Lexical scope used while lowering an event-handler body.
#[derive(Default, Clone)]
struct HandlerScope<'a> {
    handler_param: Option<(&'a str, &'a FrontendExpr)>,
    event_param: Option<&'a str>,
    handler_locals: Vec<HandlerLocalBinding>,
    event_node: Option<NodeId>,
}

impl<'a> HandlerScope<'a> {
    fn expr_scope(&'a self) -> ExprScope<'a> {
        ExprScope {
            closure_param: None,
            inline_param: None,
            handler_param: self.handler_param,
            event_param: self.event_param,
            handler_locals: &self.handler_locals,
            node_payload_target: None,
        }
    }
}

impl Analyzer {
    /// Lower a frontend expression to Client IR v3 and store it in the
    /// island expression arena, returning the allocated [`ExprId`].
    ///
    /// `span` is only used for diagnostics; the analyzer never produces
    /// frontend shapes without a Client IR v3 form.
    pub(crate) fn alloc_expr(
        &mut self,
        expr: FrontendExpr,
        span: proc_macro2::Span,
    ) -> syn::Result<ExprId> {
        let client = client_lowering::lower_handler_ir_expr_to_client_expr(&expr, span)?;
        Ok(self.builder.expr(client))
    }

    /// Resolve a path/expr that should reference a signal by name.
    /// Used by `bind_value={my_signal}` and by `signal.get()` /
    /// `signal.set()` receiver lookup.
    fn resolve_signal_path(&self, expr: &Expr) -> syn::Result<SignalId> {
        let path = match expr {
            Expr::Path(p) => &p.path,
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "`#[rsjs]` expects a signal name (a bare ident) here",
                ));
            }
        };
        let ident = path.get_ident().ok_or_else(|| {
            syn::Error::new(
                path.span(),
                "`#[rsjs]` expects a signal name (a bare ident) here",
            )
        })?;
        self.lookup_signal(&ident.to_string(), ident.span())
    }

    fn resolve_signal_target_arg(&self, expr: &Expr, helper: &str) -> syn::Result<SignalId> {
        let id = match expr {
            Expr::Reference(reference) => self.resolve_signal_path(&reference.expr)?,
            Expr::Path(_) => self.resolve_signal_path(expr)?,
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    format!("`{helper}` expects a signal target such as `&result_signal`"),
                ));
            }
        };
        // `*_into(&target)` helpers write the target signal, so a
        // memo cell can never be the destination.
        self.reject_memo_write(id, expr.span(), &format!("pass it to `{helper}`"))?;
        Ok(id)
    }

    /// Reject `action` against a memo (derived) cell.
    ///
    /// `action` completes the sentence "you cannot <action> ...", e.g.
    /// "call `.set(...)` on it" or "bind it with `bind_value`".
    fn reject_memo_write(
        &self,
        id: SignalId,
        span: proc_macro2::Span,
        action: &str,
    ) -> syn::Result<()> {
        if !self.memo_cells.contains(&id) {
            return Ok(());
        }
        let name = self
            .signals
            .iter()
            .find(|(_, signal_id)| **signal_id == id)
            .map(|(name, _)| name.as_str())
            .unwrap_or("this cell");
        Err(syn::Error::new(
            span,
            format!(
                "`{name}` is a memo cell, and memo cells are read-only: their value \
                 always derives from the signals their closure reads, so you cannot {action}. \
                 Write to one of the source signals instead, or declare `{name}` with \
                 `signal(...)` if it should hold independent state"
            ),
        ))
    }

    /// Resolve a path/expr that should reference a node ref by name.
    fn resolve_node_ref_path(&mut self, expr: &Expr) -> syn::Result<NodeRefId> {
        match expr {
            Expr::Field(field)
                if matches!(
                    field.base.as_ref(),
                    Expr::Path(path) if path.path.is_ident("self")
                ) =>
            {
                return self
                    .ensure_inbound_node_ref(field)
                    .map(|(_, node_ref)| node_ref);
            }
            Expr::Paren(paren) => return self.resolve_node_ref_path(&paren.expr),
            _ => {}
        }
        let path = match expr {
            Expr::Path(p) => &p.path,
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "`#[rsjs]` expects a node-ref name or required inbound `self.<field>` here",
                ));
            }
        };
        let ident = path.get_ident().ok_or_else(|| {
            syn::Error::new(
                path.span(),
                "`#[rsjs]` expects a node-ref name or required inbound `self.<field>` here",
            )
        })?;
        self.lookup_node_ref(&ident.to_string(), ident.span())
    }

    fn expect_signal_receiver(&mut self, receiver: &Expr, method: &str) -> syn::Result<SignalId> {
        match receiver {
            Expr::Path(p) => {
                let ident = p.path.get_ident().ok_or_else(|| {
                    syn::Error::new(
                        p.path.span(),
                        format!(
                            "`#[rsjs]` expects `.{}` to be called on a signal name (a bare ident)",
                            method
                        ),
                    )
                })?;
                self.lookup_signal(&ident.to_string(), ident.span())
            }
            Expr::MethodCall(field_call) => {
                let Expr::Path(store_path) = field_call.receiver.as_ref() else {
                    return Err(shared_signal_receiver_error(receiver.span(), method));
                };
                let Some(store_name) = store_path.path.get_ident() else {
                    return Err(shared_signal_receiver_error(receiver.span(), method));
                };
                let Some(store) = self.shared_stores.get(&store_name.to_string()).copied() else {
                    return Err(shared_signal_receiver_error(receiver.span(), method));
                };
                if !field_call.args.is_empty() || field_call.turbofish.is_some() {
                    return Err(syn::Error::new(
                        field_call.span(),
                        "rsjs/shared-state-field-arguments: shared-state field accessors take no arguments",
                    ));
                }
                let field = field_call.method.to_string();
                if let Some(signal) = self.shared_fields.get(&(store, field.clone())) {
                    return Ok(*signal);
                }
                let ty = self.shared_field_client_type(store, &field, field_call.method.span())?;
                let signal = self.builder.shared_field(store, field.clone(), ty);
                self.builder.set_signal_source(
                    signal,
                    Some(format!("{store_name}.{field}")),
                    Some(rsjs_source_anchor(field_call.method.span())),
                );
                self.shared_fields.insert((store, field), signal);
                Ok(signal)
            }
            other => Err(shared_signal_receiver_error(other.span(), method)),
        }
    }

    /// Resolve a receiver that is already known to be a local node ref.
    ///
    /// `allow_inbound` must only be true after the caller has recognized a
    /// NodeRef-specific method. The proc macro cannot know the Rust type of an
    /// arbitrary `self.<field>` projection, so eagerly treating every method
    /// receiver as an inbound NodeRef would misclassify ordinary expressions
    /// such as `self.label.as_ref()` or `self.items.len()`.
    fn node_ref_receiver(
        &mut self,
        receiver: &Expr,
        allow_inbound: bool,
    ) -> syn::Result<Option<NodeRefId>> {
        match receiver {
            Expr::Path(p) => {
                let Some(ident) = p.path.get_ident() else {
                    return Ok(None);
                };
                Ok(self.node_refs.get(&ident.to_string()).copied())
            }
            Expr::Field(field)
                if matches!(
                    field.base.as_ref(),
                    Expr::Path(path) if path.path.is_ident("self")
                ) && allow_inbound =>
            {
                self.ensure_inbound_node_ref(field)
                    .map(|(_, node_ref)| Some(node_ref))
            }
            Expr::Paren(paren) => self.node_ref_receiver(&paren.expr, allow_inbound),
            _ => Ok(None),
        }
    }

    fn query_receiver(&self, receiver: &Expr) -> Option<QueryId> {
        match receiver {
            Expr::Path(p) => {
                let ident = p.path.get_ident()?;
                self.queries.get(&ident.to_string()).copied()
            }
            Expr::Paren(paren) => self.query_receiver(&paren.expr),
            _ => None,
        }
    }

    fn mutation_receiver(&self, receiver: &Expr) -> Option<MutationId> {
        match receiver {
            Expr::Path(p) => {
                let ident = p.path.get_ident()?;
                self.mutations.get(&ident.to_string()).copied()
            }
            Expr::Paren(paren) => self.mutation_receiver(&paren.expr),
            _ => None,
        }
    }

    fn lower_query_key_expr(&mut self, expr: &Expr) -> syn::Result<FrontendExpr> {
        self.lower_structural_key_expr(expr, InboundTemporalMode::QueryKeyDependency)
    }

    fn lower_mutation_key_expr(&mut self, expr: &Expr) -> syn::Result<FrontendExpr> {
        self.lower_structural_key_expr(expr, InboundTemporalMode::LiveRead)
    }

    fn lower_structural_key_expr(
        &mut self,
        expr: &Expr,
        inbound_temporal: InboundTemporalMode,
    ) -> syn::Result<FrontendExpr> {
        if let Expr::Call(call) = expr {
            if is_query_key_new_call(&call.func) {
                expect_call_arg_count(call, 1, "rsjs::QueryKey::new(parts)")?;
                return Ok(FrontendExpr::QueryKey(
                    self.lower_structural_key_parts(&call.args[0], inbound_temporal)?,
                ));
            }
        }
        self.lower_reactive_key_part(expr, inbound_temporal)
    }

    fn lower_structural_key_parts(
        &mut self,
        expr: &Expr,
        inbound_temporal: InboundTemporalMode,
    ) -> syn::Result<Vec<FrontendExpr>> {
        match expr {
            Expr::Array(array) => array
                .elems
                .iter()
                .map(|elem| self.lower_reactive_key_part(elem, inbound_temporal))
                .collect(),
            Expr::Tuple(tuple) => tuple
                .elems
                .iter()
                .map(|elem| self.lower_reactive_key_part(elem, inbound_temporal))
                .collect(),
            Expr::Reference(reference) => {
                self.lower_structural_key_parts(&reference.expr, inbound_temporal)
            }
            Expr::Paren(paren) => self.lower_structural_key_parts(&paren.expr, inbound_temporal),
            Expr::Group(group) => self.lower_structural_key_parts(&group.expr, inbound_temporal),
            other => Ok(vec![self.lower_reactive_key_part(other, inbound_temporal)?]),
        }
    }

    fn lower_reactive_key_part(
        &mut self,
        expr: &Expr,
        inbound_temporal: InboundTemporalMode,
    ) -> syn::Result<FrontendExpr> {
        let previous = self.inbound_temporal_context;
        self.inbound_temporal_context = inbound_temporal;
        let result = self.lower_reactive_expr_in(expr, &ExprScope::default());
        self.inbound_temporal_context = previous;
        result
    }

    fn lower_invalidation_exprs(&mut self, expr: &Expr) -> syn::Result<Vec<FrontendExpr>> {
        match expr {
            Expr::Array(array) => array
                .elems
                .iter()
                .map(|elem| self.lower_mutation_key_expr(elem))
                .collect(),
            Expr::Reference(reference) => self.lower_invalidation_exprs(&reference.expr),
            Expr::Tuple(tuple) if tuple.elems.is_empty() => Ok(Vec::new()),
            Expr::Tuple(tuple) => tuple
                .elems
                .iter()
                .map(|elem| self.lower_mutation_key_expr(elem))
                .collect(),
            other => Ok(vec![self.lower_mutation_key_expr(other)?]),
        }
    }

    fn lookup_signal(&self, name: &str, span: proc_macro2::Span) -> syn::Result<SignalId> {
        self.signals.get(name).copied().ok_or_else(|| {
            syn::Error::new(
                span,
                format!(
                    "`#[rsjs]` could not find a signal named `{}`; \
                     declare it with `let {} = signal(<initial>);` before the `html_sync!` body",
                    name, name
                ),
            )
        })
    }

    fn lookup_node_ref(&self, name: &str, span: proc_macro2::Span) -> syn::Result<NodeRefId> {
        self.node_refs.get(name).copied().ok_or_else(|| {
            syn::Error::new(
                span,
                format!(
                    "`#[rsjs]` could not find a node ref named `{}`; \
                     declare it with `let {} = node_ref::<HtmlElement>();` before the `html_sync!` body",
                    name, name
                ),
            )
        })
    }
}
