// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Collect the [`SignalId`]s an expression (or handler body) reads.
//!
//! The JS emitter uses this to know which signals each `bind_text`
//! call should subscribe to: a text binding whose expression contains
//! `count.get()` needs the runtime to re-evaluate the binding when
//! `count` changes, which means the runtime needs the dep list at
//! registration time.
//!
//! Dependencies are intentionally *not* stored on [`crate::reactive::SignalDecl`] or
//! [`crate::reactive::EventHandler`]; they are computed on demand by walking the
//! Client IR v3 expression. Dependencies are derived data rather than a second
//! source of truth.
//!
//! Output is deterministic: ids appear in the order they are first
//! encountered during a pre-order walk, with duplicates removed. This
//! gives stable, reproducible JS output across builds.
//!
//! Arena expressions are pure reads in Client IR v3 (signal writes
//! are statements), so a binding's dep list is exactly the signal
//! reads its expression contains. Handlers do contain writes, and
//! `collect_handler_deps` returns *read*-only dependencies: a handler
//! that writes a signal does not need to *subscribe* to it (handlers
//! aren't reactive bodies). That distinction is enforced below.

use crate::abi::HelperCallEdgeId;
use crate::client::{
    ClientBlock, ClientBrowserEffect, ClientExpr, ClientFunctionId, ClientFunctionRef,
    ClientNodeRefEffect, ClientStmt,
};
use crate::island::ExprArena;
#[cfg(test)]
use crate::reactive::EventHandler;
use crate::types::{CallbackSlotId, ExprId, MutationId, NodeRefId, QueryId, SignalId};

/// One imported client-module function reached by Client IR.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientModuleCall {
    pub module: String,
    pub name: String,
}

/// Complete set of graph-external values and actions reached by Client IR.
///
/// Every vector is ordered by first encounter and deduplicated. Signal reads
/// and writes remain separate because only reads subscribe; writes are graph
/// effects. This is the authoritative recomputation used when validating
/// transported callback bodies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientBlockExternalDependencies {
    pub signal_reads: Vec<SignalId>,
    pub signal_writes: Vec<SignalId>,
    pub queries: Vec<QueryId>,
    pub mutations: Vec<MutationId>,
    pub node_refs: Vec<NodeRefId>,
    pub callback_slots: Vec<CallbackSlotId>,
    pub helper_edges: Vec<HelperCallEdgeId>,
    pub client_module_calls: Vec<ClientModuleCall>,
    /// Whether the body reads the invocation payload supplied by its caller.
    pub uses_callback_payload: bool,
    /// Whether the body performs a browser capability read.
    pub uses_browser_read: bool,
    /// Whether the body performs an imperative browser or DOM capability
    /// operation.
    pub uses_browser_write: bool,
    /// Whether the body reads data supplied by its triggering DOM event.
    pub uses_event_payload: bool,
    /// Whether the body reads data from a compiler-owned DOM node payload.
    pub uses_node_payload: bool,
}

/// A Client IR body contains a function reference that cannot be closed over
/// by a standalone transported callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientBlockDependencyError {
    /// Component-local functions require their definition, not just a local ID.
    LocalFunction(ClientFunctionId),
    /// An unresolved source path is not relocation-authoritative.
    UnresolvedFunction(String),
}

/// Collect the [`SignalId`]s read by the arena expression at `root`.
///
/// Returns the deps in first-seen order, with duplicates removed.
pub fn collect_deps(arena: &ExprArena, root: ExprId) -> Vec<SignalId> {
    collect_client_expr_reactive_deps(arena.client_expr(root)).signals
}

/// Collect the [`SignalId`]s read by an [`EventHandler`]'s body.
///
/// Walks every statement in the cached Client IR v3 body. For `SignalSet` and
/// `SignalUpdate` statements, only the *value* / *closure body*
/// expressions contribute reads; the destination signal of a write
/// is not added to the dep list (handlers do not subscribe to the
/// signals they mutate).
///
/// Returns the deps in first-seen order, with duplicates removed.
///
/// Currently exercised only by this module's own tests, which pin
/// the contract for the analysis primitive against the day a real
/// consumer needs it (M7 had a candidate pipeline integration that
/// never materialised; the function and tests preserve the design
/// without re-deriving it). The `#[cfg(any(test, ...))]` keeps the
/// production build free of dead code while leaving the tests live.
#[cfg(test)]
pub(crate) fn collect_handler_deps(handler: &EventHandler) -> Vec<SignalId> {
    collect_client_block_deps(handler.body_ir())
}

/// Collect signal reads from a Client IR v3 block.
///
/// This is the Client IR equivalent of the handler dependency contract above:
/// signal write destinations are not dependencies, while expressions used by
/// statement payloads are walked in source order.
pub fn collect_client_block_deps(block: &ClientBlock) -> Vec<SignalId> {
    collect_client_block_reactive_deps(block).signals
}

/// Collect all reactive reads from a Client IR v3 block.
///
/// Returns ids in first-seen order with duplicates removed.
pub fn collect_client_block_reactive_deps(block: &ClientBlock) -> ReactiveDeps {
    let mut deps = ReactiveDeps::default();
    for stmt in &block.stmts {
        collect_client_stmt_reactive_deps(stmt, &mut deps);
    }
    deps
}

/// Collect every rustc-resolved client-helper call edge reachable from a
/// Client IR v3 block.
///
/// IDs are returned in first-encounter order with duplicates removed. This is
/// used by component-callback transport validation: a payload callback body
/// must not claim a helper closure different from the calls in its actual IR.
pub fn collect_client_block_helper_edges(block: &ClientBlock) -> Vec<HelperCallEdgeId> {
    let mut edges = Vec::new();
    walk_client_block(block, &mut |expr| {
        let ClientExpr::Call {
            function: ClientFunctionRef::Helper { edge, .. },
            ..
        } = expr
        else {
            return;
        };
        if !edges.contains(edge) {
            edges.push(*edge);
        }
    });
    edges
}

/// Recompute every graph-external dependency reached by a transported Client
/// IR callback body.
///
/// The collector rejects function references whose definitions are not
/// self-contained or relocation-resolved. Callers should compare the complete
/// result with any claimed transport metadata rather than trusting the claim.
pub fn collect_client_block_external_dependencies(
    block: &ClientBlock,
) -> Result<ClientBlockExternalDependencies, ClientBlockDependencyError> {
    let mut dependencies = ClientBlockExternalDependencies::default();
    collect_external_block_dependencies(block, &mut dependencies)?;
    Ok(dependencies)
}

/// Recompute every graph-external dependency reached by one Client IR
/// expression.
///
/// This uses the same traversal, ordering, deduplication, and unresolved-call
/// rejection rules as [`collect_client_block_external_dependencies`].
/// Statement-only categories such as signal writes and callback invocations
/// remain empty. Browser reads are classified and also contribute the
/// dependencies of their argument expressions.
pub fn collect_client_expr_external_dependencies(
    expression: &ClientExpr,
) -> Result<ClientBlockExternalDependencies, ClientBlockDependencyError> {
    let mut dependencies = ClientBlockExternalDependencies::default();
    collect_external_expr_dependencies(expression, &mut dependencies)?;
    Ok(dependencies)
}

fn collect_external_block_dependencies(
    block: &ClientBlock,
    dependencies: &mut ClientBlockExternalDependencies,
) -> Result<(), ClientBlockDependencyError> {
    for statement in &block.stmts {
        collect_external_stmt_dependencies(statement, dependencies)?;
    }
    Ok(())
}

fn collect_external_stmt_dependencies(
    statement: &ClientStmt,
    dependencies: &mut ClientBlockExternalDependencies,
) -> Result<(), ClientBlockDependencyError> {
    match statement {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. } => {
            collect_external_expr_dependencies(expr, dependencies)?;
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                collect_external_expr_dependencies(value, dependencies)?;
            }
        }
        ClientStmt::SignalSet { id, value } => {
            push_unique(&mut dependencies.signal_writes, *id);
            collect_external_expr_dependencies(value, dependencies)?;
        }
        ClientStmt::SignalUpdate { id, closure } => {
            push_unique(&mut dependencies.signal_writes, *id);
            collect_external_expr_dependencies(&closure.body, dependencies)?;
        }
        ClientStmt::EventPreventDefault | ClientStmt::EventStopPropagation => {}
        ClientStmt::Console { args, .. } => {
            collect_external_expr_list(args, dependencies)?;
        }
        ClientStmt::CallbackInvoke { callback, payload } => {
            push_unique(&mut dependencies.callback_slots, *callback);
            collect_external_expr_dependencies(payload, dependencies)?;
        }
        ClientStmt::RunMutation { mutation, args } => {
            push_unique(&mut dependencies.mutations, *mutation);
            collect_external_expr_list(args, dependencies)?;
        }
        ClientStmt::QueryRefetch { query } | ClientStmt::QueryInvalidate { query } => {
            push_unique(&mut dependencies.queries, *query);
        }
        ClientStmt::BrowserEffect(effect) => {
            dependencies.uses_browser_write = true;
            match effect {
                ClientBrowserEffect::ClipboardReadTextInto { target, .. }
                | ClientBrowserEffect::WebAuthnCreateJsonInto { target, .. }
                | ClientBrowserEffect::WebAuthnGetJsonInto { target, .. }
                | ClientBrowserEffect::EventFileTextInto { target, .. } => {
                    push_unique(&mut dependencies.signal_writes, *target);
                }
                _ => {}
            }
            for expression in client_browser_effect_args(effect) {
                collect_external_expr_dependencies(expression, dependencies)?;
            }
        }
        ClientStmt::NodeRefEffect { id, effect } => {
            dependencies.uses_browser_write = true;
            push_unique(&mut dependencies.node_refs, *id);
            for expression in client_node_ref_effect_args(effect) {
                collect_external_expr_dependencies(expression, dependencies)?;
            }
        }
        ClientStmt::Block(block) => {
            collect_external_block_dependencies(block, dependencies)?;
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_external_expr_dependencies(condition, dependencies)?;
            collect_external_block_dependencies(then_block, dependencies)?;
            if let Some(else_block) = else_block {
                collect_external_block_dependencies(else_block, dependencies)?;
            }
        }
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_external_expr_dependencies(start, dependencies)?;
            collect_external_expr_dependencies(end, dependencies)?;
            collect_external_block_dependencies(body, dependencies)?;
        }
    }
    Ok(())
}

fn collect_external_expr_list(
    expressions: &[ClientExpr],
    dependencies: &mut ClientBlockExternalDependencies,
) -> Result<(), ClientBlockDependencyError> {
    for expression in expressions {
        collect_external_expr_dependencies(expression, dependencies)?;
    }
    Ok(())
}

fn collect_external_expr_dependencies(
    expression: &ClientExpr,
    dependencies: &mut ClientBlockExternalDependencies,
) -> Result<(), ClientBlockDependencyError> {
    let mut error = None;
    walk_client_expr(expression, &mut |expression| {
        if error.is_some() {
            return;
        }
        match expression {
            ClientExpr::SignalRead(signal) => {
                push_unique(&mut dependencies.signal_reads, *signal);
            }
            ClientExpr::QueryRead { id, .. } => {
                push_unique(&mut dependencies.queries, *id);
            }
            ClientExpr::MutationRead { id, .. } => {
                push_unique(&mut dependencies.mutations, *id);
            }
            ClientExpr::NodeRefRead { id, .. }
            | ClientExpr::EventTargetWithinNodeRef { id, .. } => {
                push_unique(&mut dependencies.node_refs, *id);
            }
            ClientExpr::CallbackPayload => {
                dependencies.uses_callback_payload = true;
            }
            ClientExpr::BrowserRead(_) => {
                dependencies.uses_browser_read = true;
            }
            ClientExpr::EventRead(_)
            | ClientExpr::EventCurrentTarget
            | ClientExpr::EventCollectionLen(_)
            | ClientExpr::EventCollectionRead { .. }
            | ClientExpr::EventAttribute { .. }
            | ClientExpr::EventPayload { .. } => {
                dependencies.uses_event_payload = true;
            }
            ClientExpr::NodePayload { .. } => {
                dependencies.uses_node_payload = true;
            }
            ClientExpr::Call { function, .. } => match function {
                ClientFunctionRef::Helper { edge, .. } => {
                    push_unique(&mut dependencies.helper_edges, *edge);
                }
                ClientFunctionRef::Import { module, name } => {
                    push_unique(
                        &mut dependencies.client_module_calls,
                        ClientModuleCall {
                            module: module.clone(),
                            name: name.clone(),
                        },
                    );
                }
                ClientFunctionRef::Local(id) => {
                    error = Some(ClientBlockDependencyError::LocalFunction(*id));
                }
                ClientFunctionRef::Unresolved(path) => {
                    error = Some(ClientBlockDependencyError::UnresolvedFunction(path.clone()));
                }
                ClientFunctionRef::Runtime(_) => {}
            },
            _ => {}
        }
    });
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

/// Collect all reactive reads from a Client IR v3 expression.
///
/// Returns ids in first-seen order with duplicates removed.
pub fn collect_client_expr_reactive_deps(expr: &ClientExpr) -> ReactiveDeps {
    let mut deps = ReactiveDeps::default();
    walk_client_expr(expr, &mut |expr| {
        record_client_reactive_read(expr, &mut deps);
    });
    deps
}

fn collect_client_stmt_reactive_deps(stmt: &ClientStmt, deps: &mut ReactiveDeps) {
    walk_client_stmt(stmt, &mut |expr| record_client_reactive_read(expr, deps));
}

/// Visit every [`ClientExpr`] reachable from a statement, including the
/// expressions nested inside browser effects, node-ref effects, and child
/// blocks. `visit` is called on each expression *and* on every sub-expression
/// (`walk_client_expr` recurses).
///
/// This is the single structural traversal of a client statement in the crate.
/// Dependency collection, the runtime-helper import gate, and the
/// representation-dispatch helper gate all build on it, so a new expression
/// position added here is seen by all of them at once.
pub fn walk_client_stmt(stmt: &ClientStmt, visit: &mut impl FnMut(&ClientExpr)) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => walk_client_expr(expr, visit),
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                walk_client_expr(value, visit);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => walk_client_expr(&closure.body, visit),
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                walk_client_expr(arg, visit);
            }
        }
        ClientStmt::BrowserEffect(effect) => {
            for expr in client_browser_effect_args(effect) {
                walk_client_expr(expr, visit);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => walk_client_expr(payload, visit),
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            walk_client_expr(condition, visit);
            walk_client_block(then_block, visit);
            if let Some(else_block) = else_block {
                walk_client_block(else_block, visit);
            }
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            for expr in client_node_ref_effect_args(effect) {
                walk_client_expr(expr, visit);
            }
        }
        ClientStmt::Block(block) => walk_client_block(block, visit),
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            walk_client_expr(start, visit);
            walk_client_expr(end, visit);
            walk_client_block(body, visit);
        }
    }
}

/// Visit every [`ClientExpr`] reachable from a block. See [`walk_client_stmt`].
pub fn walk_client_block(block: &ClientBlock, visit: &mut impl FnMut(&ClientExpr)) {
    for stmt in &block.stmts {
        walk_client_stmt(stmt, visit);
    }
}

fn client_browser_effect_args(effect: &ClientBrowserEffect) -> Vec<&ClientExpr> {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => {
            vec![x, y]
        }
        ClientBrowserEffect::WindowPrint => Vec::new(),
        ClientBrowserEffect::LocationSetHash { value } => vec![value],
        ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => Vec::new(),
        ClientBrowserEffect::LocationAssign { url }
        | ClientBrowserEffect::LocationReplace { url }
        | ClientBrowserEffect::HistoryPushState { url }
        | ClientBrowserEffect::HistoryReplaceState { url } => vec![url],
        ClientBrowserEffect::HistoryGo { delta } => vec![delta],
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            let mut args = vec![name];
            if let Some(detail) = detail {
                args.push(detail);
            }
            args
        }
        ClientBrowserEffect::DocumentSetTitle { title } => vec![title],
        ClientBrowserEffect::ClipboardWriteText { text } => vec![text],
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. } => {
            fallback.iter().collect::<Vec<_>>()
        }
        ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            fallback,
            ..
        }
        | ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            fallback,
            ..
        } => {
            let mut args = vec![options_json];
            if let Some(fallback) = fallback {
                args.push(fallback);
            }
            args
        }
        ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            fallback.iter().collect::<Vec<_>>()
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. } => vec![key, value],
        ClientBrowserEffect::StorageRemoveItem { key, .. } => vec![key],
        ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => vec![key, value],
        ClientBrowserEffect::UrlDeleteSearchParam { key, .. } => vec![key],
        ClientBrowserEffect::Declared { args, .. } => args.iter().collect(),
    }
}

fn client_node_ref_effect_args(effect: &ClientNodeRefEffect) -> Vec<&ClientExpr> {
    match effect {
        ClientNodeRefEffect::Focus
        | ClientNodeRefEffect::Blur
        | ClientNodeRefEffect::ScrollIntoView
        | ClientNodeRefEffect::Click
        | ClientNodeRefEffect::RequestSubmit
        | ClientNodeRefEffect::Reset
        | ClientNodeRefEffect::ReportValidity
        | ClientNodeRefEffect::ShowModal
        | ClientNodeRefEffect::Close
        | ClientNodeRefEffect::ShowPopover
        | ClientNodeRefEffect::HidePopover
        | ClientNodeRefEffect::TogglePopover
        | ClientNodeRefEffect::Select => Vec::new(),
        ClientNodeRefEffect::ScrollTo { x, y } | ClientNodeRefEffect::ScrollBy { x, y } => {
            vec![x, y]
        }
        ClientNodeRefEffect::SetPointerCapture(expr)
        | ClientNodeRefEffect::ReleasePointerCapture(expr) => vec![expr],
        ClientNodeRefEffect::SetValue(expr)
        | ClientNodeRefEffect::SetChecked(expr)
        | ClientNodeRefEffect::SetCustomValidity(expr)
        | ClientNodeRefEffect::SetTextContent(expr)
        | ClientNodeRefEffect::AddClass(expr)
        | ClientNodeRefEffect::RemoveClass(expr)
        | ClientNodeRefEffect::ToggleClass(expr) => vec![expr],
        ClientNodeRefEffect::SetAttribute { value, .. } => vec![value],
        ClientNodeRefEffect::RemoveAttribute { .. } => Vec::new(),
        ClientNodeRefEffect::SetStyleProperty { value, .. } => vec![value],
        ClientNodeRefEffect::SetSelectionRange(start, end) => vec![start, end],
    }
}

/// Visit an expression and every nested [`ClientExpr`] in stable structural
/// preorder.
///
/// This is the exhaustive expression traversal shared by dependency analysis,
/// emitters, and descriptor consumers that must validate contextual leaves
/// such as node-local payload reads.
pub fn walk_client_expr(expr: &ClientExpr, visit: &mut impl FnMut(&ClientExpr)) {
    visit(expr);
    match expr {
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
        | ClientExpr::NodePayload { .. } => {}
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
        | ClientExpr::Field { base: index, .. } => walk_client_expr(index, visit),
        ClientExpr::BrowserRead(read) => walk_client_browser_read(read, visit),
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_client_expr(condition, visit);
            walk_client_expr(then_expr, visit);
            walk_client_expr(else_expr, visit);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        } => {
            walk_client_expr(haystack, visit);
            walk_client_expr(needle, visit);
        }
        ClientExpr::Binary { left, right, .. } => {
            walk_client_expr(left, visit);
            walk_client_expr(right, visit);
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                walk_client_expr(&field.value, visit);
            }
        }
        ClientExpr::Index { base, index } => {
            walk_client_expr(base, visit);
            walk_client_expr(index, visit);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                walk_client_expr(part, visit);
            }
        }
        ClientExpr::Call { args, .. } => {
            for arg in args {
                walk_client_expr(arg, visit);
            }
        }
    }
}

fn walk_client_browser_read(
    read: &crate::client::ClientBrowserRead,
    visit: &mut impl FnMut(&ClientExpr),
) {
    match read {
        crate::client::ClientBrowserRead::Window(_)
        | crate::client::ClientBrowserRead::Location(_) => {}
        crate::client::ClientBrowserRead::Storage { key, fallback, .. }
        | crate::client::ClientBrowserRead::UrlSearchParam { key, fallback } => {
            walk_client_expr(key, visit);
            if let Some(fallback) = fallback {
                walk_client_expr(fallback, visit);
            }
        }
        crate::client::ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                walk_client_expr(arg, visit);
            }
        }
    }
}

fn record_client_reactive_read(expr: &ClientExpr, deps: &mut ReactiveDeps) {
    match expr {
        ClientExpr::SignalRead(id) if !deps.signals.contains(id) => {
            deps.signals.push(*id);
        }
        ClientExpr::QueryRead { id, .. } if !deps.queries.contains(id) => {
            deps.queries.push(*id);
        }
        ClientExpr::MutationRead { id, .. } if !deps.mutations.contains(id) => {
            deps.mutations.push(*id);
        }
        _ => {}
    }
}

/// Reactive dependency set for a binding: the signals it reads, the
/// queries whose state slots it projects through, and the mutations
/// whose pending/error state it observes.
///
/// Round-3 review #4: a binding that reads `query.data()` (or any
/// other [`crate::client::QueryAccessor`]) must subscribe to that
/// query's handle, not just to plain signals; otherwise the binding
/// applies once at hydration and never updates when the query's
/// data changes. Same goes for mutations.
///
/// The IR exposes the three sets separately so the JS emitter can
/// build the runtime's `bind_text(node, getter, signals,
/// extra_handles)` argument list with the right per-kind lookups.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReactiveDeps {
    pub signals: Vec<SignalId>,
    pub queries: Vec<QueryId>,
    pub mutations: Vec<MutationId>,
}

/// Walk an expression tree at `root` and collect every reactive
/// dependency: [`SignalId`]s read, [`QueryId`]s whose state is
/// projected through, and [`MutationId`]s whose pending/error state
/// is observed.
///
/// Returns ids in first-seen order with duplicates removed (same
/// determinism contract as [`collect_deps`]).
pub fn collect_reactive_deps(arena: &ExprArena, root: ExprId) -> ReactiveDeps {
    collect_client_expr_reactive_deps(arena.client_expr(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::IslandBuilder;
    use crate::client::{ClientBinaryOp, ClientSimpleClosure};
    use crate::client::{MutationAccessor, QueryAccessor};
    use crate::reactive::StorageArea;
    use crate::value::Value;

    #[test]
    fn collect_deps_dedupes_repeated_signal_reads() {
        let mut b = IslandBuilder::new();
        let s = b.signal(Value::Int(0));
        // count.get() + count.get()
        let e = b.expr(ClientExpr::Binary {
            op: ClientBinaryOp::Add,
            left: Box::new(ClientExpr::SignalRead(s)),
            right: Box::new(ClientExpr::SignalRead(s)),
        });
        let island = b.build();
        let deps = collect_deps(&island.exprs, e);
        assert_eq!(deps, vec![s]);
    }

    #[test]
    fn collect_deps_orders_by_first_encounter() {
        let mut b = IslandBuilder::new();
        let a = b.signal(Value::Int(0));
        let c = b.signal(Value::Int(0));
        // a.get() + c.get()
        let e = b.expr(ClientExpr::Binary {
            op: ClientBinaryOp::Add,
            left: Box::new(ClientExpr::SignalRead(a)),
            right: Box::new(ClientExpr::SignalRead(c)),
        });
        let island = b.build();
        let deps = collect_deps(&island.exprs, e);
        assert_eq!(deps, vec![a, c]);
    }

    #[test]
    fn collect_client_block_helper_edges_is_ordered_and_deduplicated() {
        let helper_call = |id| ClientExpr::Call {
            function: ClientFunctionRef::Helper {
                edge: HelperCallEdgeId::new(id),
                authored_path: format!("helpers::helper_{id}"),
                name: format!("helper_{id}"),
            },
            args: Vec::new(),
        };
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::Expr(helper_call(4)),
            ClientStmt::Block(ClientBlock::from_stmts(vec![
                ClientStmt::Expr(helper_call(2)),
                ClientStmt::Expr(helper_call(4)),
            ])),
        ]);

        assert_eq!(
            collect_client_block_helper_edges(&block),
            vec![HelperCallEdgeId::new(4), HelperCallEdgeId::new(2)]
        );
    }

    #[test]
    fn statement_walker_visits_each_expression_once_in_source_order() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::Let {
                local: crate::client::ClientLocalId(0),
                name: "value".to_string(),
                ty: crate::client::ClientType::Unknown,
                value: Some(ClientExpr::SignalRead(SignalId(0))),
            },
            ClientStmt::ForRange {
                local: crate::client::ClientLocalId(1),
                name: "index".to_string(),
                integer_domain: crate::client::IntegerDomain::U32,
                start: ClientExpr::SignalRead(SignalId(1)),
                end: ClientExpr::SignalRead(SignalId(2)),
                inclusive: false,
                body: ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
                    ClientExpr::SignalRead(SignalId(3)),
                ))]),
            },
        ]);

        let mut visits = Vec::new();
        walk_client_block(&block, &mut |expression| {
            if let ClientExpr::SignalRead(signal) = expression {
                visits.push(*signal);
            }
        });

        assert_eq!(
            visits,
            vec![SignalId(0), SignalId(1), SignalId(2), SignalId(3)]
        );
    }

    #[test]
    fn external_dependencies_recompute_every_transported_callback_reference() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::SignalSet {
                id: SignalId(2),
                value: ClientExpr::Call {
                    function: ClientFunctionRef::Import {
                        module: "labels".to_string(),
                        name: "format".to_string(),
                    },
                    args: vec![ClientExpr::SignalRead(SignalId(1))],
                },
            },
            ClientStmt::If {
                condition: ClientExpr::QueryRead {
                    id: QueryId(3),
                    accessor: QueryAccessor::Data,
                },
                then_block: ClientBlock::from_stmts(vec![
                    ClientStmt::RunMutation {
                        mutation: MutationId(4),
                        args: vec![ClientExpr::NodeRefRead {
                            id: NodeRefId(5),
                            accessor: crate::client::NodeRefAccessor::Value,
                        }],
                    },
                    ClientStmt::CallbackInvoke {
                        callback: CallbackSlotId(6),
                        payload: ClientExpr::Call {
                            function: ClientFunctionRef::Helper {
                                edge: HelperCallEdgeId::new(7),
                                authored_path: "helpers::notify".to_string(),
                                name: "notify".to_string(),
                            },
                            args: vec![ClientExpr::CallbackPayload],
                        },
                    },
                ]),
                else_block: None,
            },
        ]);

        assert_eq!(
            collect_client_block_external_dependencies(&block).unwrap(),
            ClientBlockExternalDependencies {
                signal_reads: vec![SignalId(1)],
                signal_writes: vec![SignalId(2)],
                queries: vec![QueryId(3)],
                mutations: vec![MutationId(4)],
                node_refs: vec![NodeRefId(5)],
                callback_slots: vec![CallbackSlotId(6)],
                helper_edges: vec![HelperCallEdgeId::new(7)],
                client_module_calls: vec![ClientModuleCall {
                    module: "labels".to_string(),
                    name: "format".to_string(),
                }],
                uses_callback_payload: true,
                uses_browser_read: false,
                uses_browser_write: false,
                uses_event_payload: false,
                uses_node_payload: false,
            }
        );
    }

    #[test]
    fn expression_external_dependencies_are_ordered_and_deduplicated() {
        let expression = ClientExpr::QueryKey(vec![
            ClientExpr::SignalRead(SignalId(2)),
            ClientExpr::Call {
                function: ClientFunctionRef::Import {
                    module: "labels".to_string(),
                    name: "format".to_string(),
                },
                args: vec![
                    ClientExpr::QueryRead {
                        id: QueryId(3),
                        accessor: QueryAccessor::Data,
                    },
                    ClientExpr::SignalRead(SignalId(2)),
                ],
            },
            ClientExpr::Conditional {
                condition: Box::new(ClientExpr::MutationRead {
                    id: MutationId(4),
                    accessor: MutationAccessor::IsPending,
                }),
                then_expr: Box::new(ClientExpr::NodeRefRead {
                    id: NodeRefId(5),
                    accessor: crate::client::NodeRefAccessor::Value,
                }),
                else_expr: Box::new(ClientExpr::Call {
                    function: ClientFunctionRef::Helper {
                        edge: HelperCallEdgeId::new(7),
                        authored_path: "helpers::notify".to_string(),
                        name: "notify".to_string(),
                    },
                    args: vec![ClientExpr::CallbackPayload],
                }),
            },
            ClientExpr::BrowserRead(crate::client::ClientBrowserRead::Declared {
                helper: "document.lookup".to_string(),
                args: vec![
                    ClientExpr::SignalRead(SignalId(1)),
                    ClientExpr::EventTargetWithinNodeRef {
                        id: NodeRefId(5),
                        inside: true,
                    },
                ],
            }),
            ClientExpr::EventPayload {
                attr: "value".to_string(),
            },
            ClientExpr::NodePayload {
                attr: "data-state".to_string(),
                tokens: "open closed".to_string(),
            },
            ClientExpr::Call {
                function: ClientFunctionRef::Import {
                    module: "labels".to_string(),
                    name: "format".to_string(),
                },
                args: vec![ClientExpr::QueryRead {
                    id: QueryId(3),
                    accessor: QueryAccessor::Error,
                }],
            },
            ClientExpr::Call {
                function: ClientFunctionRef::Helper {
                    edge: HelperCallEdgeId::new(7),
                    authored_path: "helpers::notify".to_string(),
                    name: "notify".to_string(),
                },
                args: Vec::new(),
            },
        ]);

        assert_eq!(
            collect_client_expr_external_dependencies(&expression).unwrap(),
            ClientBlockExternalDependencies {
                signal_reads: vec![SignalId(2), SignalId(1)],
                signal_writes: Vec::new(),
                queries: vec![QueryId(3)],
                mutations: vec![MutationId(4)],
                node_refs: vec![NodeRefId(5)],
                callback_slots: Vec::new(),
                helper_edges: vec![HelperCallEdgeId::new(7)],
                client_module_calls: vec![ClientModuleCall {
                    module: "labels".to_string(),
                    name: "format".to_string(),
                }],
                uses_callback_payload: true,
                uses_browser_read: true,
                uses_browser_write: false,
                uses_event_payload: true,
                uses_node_payload: true,
            }
        );
    }

    #[test]
    fn expression_external_dependencies_reject_unclosed_function_references() {
        let expression = ClientExpr::Call {
            function: ClientFunctionRef::Local(ClientFunctionId(9)),
            args: vec![ClientExpr::Call {
                function: ClientFunctionRef::Unresolved("unreachable::helper".to_string()),
                args: Vec::new(),
            }],
        };

        assert_eq!(
            collect_client_expr_external_dependencies(&expression),
            Err(ClientBlockDependencyError::LocalFunction(ClientFunctionId(
                9
            )))
        );
    }

    #[test]
    fn external_dependencies_classify_browser_reads_and_imperative_effects() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::Expr(ClientExpr::BrowserRead(
                crate::client::ClientBrowserRead::Declared {
                    helper: "window.prefers_reduced_motion".to_string(),
                    args: Vec::new(),
                },
            )),
            ClientStmt::BrowserEffect(ClientBrowserEffect::WindowPrint),
            ClientStmt::NodeRefEffect {
                id: NodeRefId(11),
                effect: ClientNodeRefEffect::Focus,
            },
        ]);

        assert_eq!(
            collect_client_block_external_dependencies(&block).unwrap(),
            ClientBlockExternalDependencies {
                node_refs: vec![NodeRefId(11)],
                uses_browser_read: true,
                uses_browser_write: true,
                ..ClientBlockExternalDependencies::default()
            }
        );
    }

    #[test]
    fn external_dependencies_reject_unclosed_function_references() {
        let local = ClientBlock::from_stmts(vec![ClientStmt::Expr(ClientExpr::Call {
            function: ClientFunctionRef::Local(ClientFunctionId(9)),
            args: Vec::new(),
        })]);
        assert_eq!(
            collect_client_block_external_dependencies(&local),
            Err(ClientBlockDependencyError::LocalFunction(ClientFunctionId(
                9
            )))
        );

        let unresolved = ClientBlock::from_stmts(vec![ClientStmt::Expr(ClientExpr::Call {
            function: ClientFunctionRef::Unresolved("labels::format".to_string()),
            args: Vec::new(),
        })]);
        assert_eq!(
            collect_client_block_external_dependencies(&unresolved),
            Err(ClientBlockDependencyError::UnresolvedFunction(
                "labels::format".to_string()
            ))
        );
    }

    #[test]
    fn collect_handler_deps_walks_closure_body_in_signal_update() {
        // Counter-style handler: count.update(|c| count.get() + 1).
        // The closure body reads `count`, so `count` must appear in
        // the dep list. The destination signal of the update (also
        // `count`) does NOT contribute a dep.
        let count = SignalId(0);
        let handler = EventHandler::from_ir(
            crate::types::EventHandlerId(0),
            ClientBlock::from_stmts(vec![ClientStmt::signal_update(
                count,
                ClientSimpleClosure::new(
                    "c",
                    ClientExpr::binary(
                        ClientBinaryOp::Add,
                        ClientExpr::signal_read(count),
                        ClientExpr::int(1),
                    ),
                ),
            )]),
        );
        assert_eq!(collect_handler_deps(&handler), vec![count]);
    }

    #[test]
    fn collect_reactive_deps_includes_queries_and_mutations() {
        // Round-3 review #4: a binding that reads `query.data() ===
        // null && mutation.is_pending()` must record the QueryId and
        // the MutationId so the JS emitter can hand them to the
        // runtime's `bind_text` extra-handles arg. The signals set
        // stays empty (no signal reads in the expression).
        let mut b = IslandBuilder::new();
        let q = crate::types::QueryId(0);
        let m = crate::types::MutationId(0);
        let expr = b.expr(ClientExpr::Binary {
            op: ClientBinaryOp::And,
            left: Box::new(ClientExpr::Eq {
                target: Box::new(ClientExpr::QueryRead {
                    id: q,
                    accessor: crate::client::QueryAccessor::Data,
                }),
                value: crate::value::Value::Null,
            }),
            right: Box::new(ClientExpr::MutationRead {
                id: m,
                accessor: crate::client::MutationAccessor::IsPending,
            }),
        });
        let island = b.build();
        let deps = collect_reactive_deps(&island.exprs, expr);
        assert!(deps.signals.is_empty());
        assert_eq!(deps.queries, vec![q]);
        assert_eq!(deps.mutations, vec![m]);
    }

    #[test]
    fn collect_client_block_reactive_deps_walks_v3_statements() {
        let first = SignalId(0);
        let write_target = SignalId(1);
        let second = SignalId(2);
        let q = QueryId(0);
        let m = MutationId(0);

        let block = ClientBlock::from_stmts(vec![
            ClientStmt::Let {
                local: crate::client::ClientLocalId(0),
                name: "current".to_string(),
                ty: crate::client::ClientType::Unknown,
                value: Some(ClientExpr::SignalRead(first)),
            },
            ClientStmt::SignalSet {
                id: write_target,
                value: ClientExpr::QueryRead {
                    id: q,
                    accessor: QueryAccessor::Data,
                },
            },
            ClientStmt::If {
                condition: ClientExpr::MutationRead {
                    id: m,
                    accessor: MutationAccessor::IsPending,
                },
                then_block: ClientBlock::from_stmts(vec![ClientStmt::BrowserEffect(
                    ClientBrowserEffect::StorageSetItem {
                        area: StorageArea::Local,
                        key: ClientExpr::SignalRead(first),
                        value: ClientExpr::string("cached"),
                    },
                )]),
                else_block: Some(ClientBlock::from_stmts(vec![ClientStmt::NodeRefEffect {
                    id: crate::types::NodeRefId(0),
                    effect: ClientNodeRefEffect::SetValue(ClientExpr::SignalRead(second)),
                }])),
            },
        ]);

        let deps = collect_client_block_reactive_deps(&block);

        assert_eq!(deps.signals, vec![first, second]);
        assert_eq!(deps.queries, vec![q]);
        assert_eq!(deps.mutations, vec![m]);
    }

    #[test]
    fn collect_handler_deps_skips_pure_signal_set_destination() {
        // count.set(other.get()) → deps = [other], NOT [count, other].
        let count = SignalId(0);
        let other = SignalId(1);
        let handler = EventHandler::from_ir(
            crate::types::EventHandlerId(0),
            ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                count,
                ClientExpr::signal_read(other),
            )]),
        );
        assert_eq!(collect_handler_deps(&handler), vec![other]);
    }
}
