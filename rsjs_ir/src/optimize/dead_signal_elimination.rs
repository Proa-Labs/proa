// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `dead_signal_elimination` — drop signals with no readers and no
//! writers.
//!
//! This pass is deliberately conservative. A signal is "dead" iff it has
//! neither readers nor writers anywhere
//! in the IR:
//!
//! - **Readers**: any Client IR v3 signal read referencing the signal id,
//!   walked across every arena expression and every Client IR v3
//!   handler block.
//! - **Writers**: any [`crate::client::ClientStmt::SignalSet`] /
//!   [`crate::client::ClientStmt::SignalUpdate`] destination id in
//!   handlers, and browser-effect target signal writes in handlers.
//!   Arena expressions cannot write signals: writes are statements
//!   in Client IR v3.
//! - **Component fragment mounts**: a parent signal aliased into a
//!   mounted child fragment counts as both, and parent-side callback
//!   adapter bodies are walked like handler bodies. This traversal is
//!   shared with [`crate::analysis::strategy`] so the pass and
//!   reachability analysis cannot disagree about which signals a mount
//!   keeps alive.
//!
//! A signal that is *only ever written* (no reader anywhere) is NOT
//! dead by this pass: writes may be observed by other handlers
//! through dependent computations not yet present in the v0.1
//! surface, and removing the write changes the user's mental model.
//! A more aggressive write-only-is-dead variant lands in M9+, gated
//! on a use case that demonstrates the trade-off is worth it.
//!
//! # Identity preservation
//!
//! Like [`super::handler_dedup`], this pass leaves the surviving
//! signals' [`crate::reactive::SignalDecl::id`] values unchanged. The
//! `signals` vector is no longer dense after a removal (gaps appear
//! where dead signals used to live), but every consumer of the IR
//! keys off the stored id, not the vector position.

use std::collections::HashSet;

use crate::analysis::dep_collector::collect_client_expr_reactive_deps;
use crate::analysis::strategy::{mount_reactive_roots, parent_runtime_mounts, MountReactiveRoot};
use crate::client::{
    ClientBlock, ClientBrowserEffect, ClientExpr, ClientNodeRefEffect, ClientStmt,
};
use crate::island::Island;
use crate::lists::ListSource;
use crate::types::SignalId;

use super::{IslandPass, PassResult, PassStats};

/// The `dead_signal_elimination` pass. See the module-level docs.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeadSignalElimination;

impl DeadSignalElimination {
    /// Construct a new pass instance.
    pub fn new() -> Self {
        Self
    }
}

impl IslandPass for DeadSignalElimination {
    fn name(&self) -> &'static str {
        "dead_signal_elimination"
    }

    fn run(&self, island: &mut Island) -> PassResult {
        let visited = island.signals.len();
        if visited == 0 {
            return PassResult::default();
        }

        let mut readers: HashSet<SignalId> = HashSet::new();
        let mut writers: HashSet<SignalId> = HashSet::new();

        // 1. Walk every arena expression. Arena expressions are pure
        //    reads (signal writes are statements in Client IR v3), so
        //    they only ever contribute readers. The arena entries do
        //    not include handler statement payloads; those are walked
        //    separately below.
        for expr in island.exprs.iter_client_exprs() {
            record_client_expr_reads(expr, &mut readers);
        }

        // 2. Walk every handler through its Client IR v3 block.
        for handler in &island.handlers {
            record_client_block_refs(handler.body_ir(), &mut readers, &mut writers);
        }

        // 2b. Interval timer bodies read and write signals exactly like
        //     handlers; without this walk a signal touched only by an
        //     `rsjs::interval(...)` tick would be classified as dead.
        for interval in &island.intervals {
            record_client_block_refs(interval.body_ir(), &mut readers, &mut writers);
        }

        // 2c. Keyed lists read their source through the `bind_list`
        //     getter and emitted dep arrays; without this walk a
        //     Vec-valued signal used only by a keyed list would be
        //     classified as dead. Expression-backed lists may also
        //     contain signal reads nested under query/mutation accessors.
        for list in &island.lists {
            match list.source {
                ListSource::Signal(signal) => {
                    readers.insert(signal);
                }
                ListSource::Expr(expr) => {
                    record_client_expr_reads(island.exprs.client_expr(expr), &mut readers);
                }
            }
        }

        // 3. Round-3 review #5: input bindings (`bind_value`,
        //    `bind_checked`, `bind_selected`) reference a signal
        //    two-way: the runtime mirrors the DOM property to the
        //    signal AND writes signal updates back to the DOM. The
        //    signal is therefore both a reader and a writer of its
        //    bound element. Without this step, a signal used only
        //    by an input binding would be classified as dead and
        //    dropped, even though the emitted JS still references
        //    `__sig_<id>`.
        for binding in island.bindings.input_bindings.values() {
            readers.insert(binding.signal);
            writers.insert(binding.signal);
        }

        // 3b. Reusable component fragment mounts keep parent signals
        //     live in two ways that live on the mount rather than in
        //     the expression arena or `island.handlers`:
        //
        //     - `mount.signal_aliases[].parent` is a parent signal cell
        //       handed to the child fragment at mount time. The child
        //       both reads it (its bindings subscribe to the cell) and
        //       can write it back through the same object, so it counts
        //       as reader and writer exactly like an input binding.
        //     - `mount.callbacks[].body_ir()` is a parent-side callback
        //       adapter whose block is stored on the mount, so the
        //       handler walk above never sees its signal writes.
        //
        //     Without this walk the pass deleted declarations that
        //     `emit_component_fragment_mounts` still emits by name, and
        //     the island JS referenced an undeclared `__sig_<id>`: the
        //     mount call threw `ReferenceError` at hydration (swallowed
        //     into `report_hydration_error`, so the child silently never
        //     mounted) and every callback invocation threw as well.
        //
        //     The traversal is shared with
        //     `crate::analysis::strategy`'s reachability walk so the
        //     pass and the analysis agree by construction.
        for mount in parent_runtime_mounts(island) {
            for root in mount_reactive_roots(mount) {
                match root {
                    MountReactiveRoot::Block(block) => {
                        record_client_block_refs(block, &mut readers, &mut writers);
                    }
                    MountReactiveRoot::ParentSignal(signal) => {
                        readers.insert(signal);
                        writers.insert(signal);
                    }
                }
            }
        }

        // 4. A signal is dead if it has neither readers nor writers.
        let before = island.signals.len();
        island
            .signals
            .retain(|s| readers.contains(&s.id) || writers.contains(&s.id));
        let removed = before - island.signals.len();

        if removed == 0 {
            return PassResult {
                changed: false,
                stats: PassStats {
                    items_visited: visited,
                    ..PassStats::default()
                },
            };
        }

        PassResult {
            changed: true,
            stats: PassStats {
                items_removed: removed,
                items_merged: 0,
                items_visited: visited,
            },
        }
    }
}

fn record_client_block_refs(
    block: &ClientBlock,
    readers: &mut HashSet<SignalId>,
    writers: &mut HashSet<SignalId>,
) {
    for stmt in &block.stmts {
        record_client_statement_refs(stmt, readers, writers);
    }
}

fn record_client_statement_refs(
    stmt: &ClientStmt,
    readers: &mut HashSet<SignalId>,
    writers: &mut HashSet<SignalId>,
) {
    match stmt {
        ClientStmt::SignalSet { id, value } => {
            writers.insert(*id);
            record_client_expr_reads(value, readers);
        }
        ClientStmt::SignalUpdate { id, closure } => {
            writers.insert(*id);
            record_client_expr_reads(&closure.body, readers);
        }
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. } => {
            record_client_expr_reads(expr, readers);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                record_client_expr_reads(value, readers);
            }
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
        ClientStmt::Console { args, .. } => {
            for arg in args {
                record_client_expr_reads(arg, readers);
            }
        }
        ClientStmt::BrowserEffect(effect) => {
            if let ClientBrowserEffect::ClipboardReadTextInto { target, .. }
            | ClientBrowserEffect::WebAuthnCreateJsonInto { target, .. }
            | ClientBrowserEffect::WebAuthnGetJsonInto { target, .. }
            | ClientBrowserEffect::EventFileTextInto { target, .. } = effect
            {
                writers.insert(*target);
            }
            for expr in client_browser_effect_args(effect) {
                record_client_expr_reads(expr, readers);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            record_client_expr_reads(payload, readers);
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            record_client_expr_reads(condition, readers);
            record_client_block_refs(then_block, readers, writers);
            if let Some(else_block) = else_block {
                record_client_block_refs(else_block, readers, writers);
            }
        }
        ClientStmt::RunMutation { args, .. } => {
            // Argument expressions read signals the same way other reactive
            // expressions do; the mutation id itself is a separate side-table
            // the pass does not GC.
            for arg in args {
                record_client_expr_reads(arg, readers);
            }
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            for expr in client_node_ref_effect_args(effect) {
                record_client_expr_reads(expr, readers);
            }
        }
        ClientStmt::Block(block) => record_client_block_refs(block, readers, writers),
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            record_client_expr_reads(start, readers);
            record_client_expr_reads(end, readers);
            record_client_block_refs(body, readers, writers);
        }
    }
}

fn record_client_expr_reads(expr: &ClientExpr, readers: &mut HashSet<SignalId>) {
    for signal in collect_client_expr_reactive_deps(expr).signals {
        readers.insert(signal);
    }
}

fn client_browser_effect_args(effect: &ClientBrowserEffect) -> Vec<&ClientExpr> {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => vec![x, y],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::IslandBuilder;
    use crate::client::{ClientBinaryOp, ClientSimpleClosure};

    use crate::value::Value;

    #[test]
    fn drops_signal_with_no_readers_or_writers() {
        // _unused has no readers and no writers; count is read +
        // written. Pass should drop _unused, keep count.
        let mut b = IslandBuilder::new();
        let _unused = b.signal(Value::Int(0));
        let count = b.signal(Value::Int(0));

        // count.get() expression in arena (a reader).
        b.expr(ClientExpr::SignalRead(count));

        // Handler that mutates count via update (a writer).
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            count,
            ClientSimpleClosure::new(
                "c",
                ClientExpr::binary(
                    ClientBinaryOp::Add,
                    ClientExpr::signal_read(count),
                    ClientExpr::int(1),
                ),
            ),
        )]));

        let mut island = b.build();
        let r = DeadSignalElimination::new().run(&mut island);

        assert!(r.changed);
        assert_eq!(r.stats.items_removed, 1);
        assert_eq!(island.signals.len(), 1);
        assert_eq!(island.signals[0].id, count);
    }

    #[test]
    fn write_only_signal_is_kept() {
        // The conservative variant: only no-reader-AND-no-writer
        // signals are dead. A signal that is only written must
        // survive (writes can be observed through other paths once
        // queries / memos land).
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));

        // Handler that *writes* count but never reads it.
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            count,
            ClientExpr::int(42),
        )]));

        let mut island = b.build();
        let r = DeadSignalElimination::new().run(&mut island);
        assert!(!r.changed, "write-only signals must be preserved");
        assert_eq!(island.signals.len(), 1);
    }

    #[test]
    fn read_only_signal_is_kept() {
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        b.expr(ClientExpr::SignalRead(count));
        let mut island = b.build();
        let r = DeadSignalElimination::new().run(&mut island);
        assert!(!r.changed);
        assert_eq!(island.signals.len(), 1);
    }

    #[test]
    fn idempotent_when_run_twice() {
        let mut b = IslandBuilder::new();
        let _a = b.signal(Value::Int(0));
        let _b = b.signal(Value::Int(0));
        let c = b.signal(Value::Int(0));
        b.expr(ClientExpr::SignalRead(c));
        let mut island = b.build();

        let pass = DeadSignalElimination::new();
        let r1 = pass.run(&mut island);
        assert!(r1.changed);
        assert_eq!(r1.stats.items_removed, 2);

        let snapshot = island.clone();
        let r2 = pass.run(&mut island);
        assert!(!r2.changed);
        assert_eq!(island, snapshot);
    }

    #[test]
    fn input_binding_signal_is_kept() {
        // Round-3 review #5 regression. A signal used only via
        // `bind_value` (or any `bind_*`) was incorrectly classified
        // as dead before the fix; the input binding contributes
        // both a read (DOM -> signal) and a write (signal -> DOM)
        // to the live set so the signal must survive.
        use crate::bindings::InputKind;
        let mut b = IslandBuilder::new();
        let username = b.signal(Value::String(String::new()));
        let input_node = b.element("input", None);
        b.bind_input(input_node, InputKind::Value, username);
        b.set_root(input_node);

        let mut island = b.build();
        let r = DeadSignalElimination::new().run(&mut island);
        assert!(!r.changed, "signals used by input bindings must survive");
        assert_eq!(island.signals.len(), 1);
        assert_eq!(island.signals[0].id, username);
    }

    #[test]
    fn handler_browser_effect_target_counts_as_writer() {
        let mut b = IslandBuilder::new();
        let clipboard_value = b.signal(Value::String(String::new()));
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::BrowserEffect(
            ClientBrowserEffect::ClipboardReadTextInto {
                target: clipboard_value,
                fallback: None,
            },
        )]));

        let mut island = b.build();
        let r = DeadSignalElimination::new().run(&mut island);

        assert!(
            !r.changed,
            "browser-effect target signals must be preserved"
        );
        assert_eq!(island.signals.len(), 1);
        assert_eq!(island.signals[0].id, clipboard_value);
    }

    #[test]
    fn empty_island_is_a_no_op() {
        let mut island = Island::new();
        let r = DeadSignalElimination::new().run(&mut island);
        assert!(!r.changed);
        assert_eq!(r.stats.items_removed, 0);
    }

    #[test]
    fn handler_only_signal_set_destination_counts_as_writer() {
        // A SignalSet statement destination (with no reader anywhere)
        // must still register as a writer so the signal survives.
        // Expression-level signal writes no longer exist: the arena
        // stores Client IR v3, where writes are statements only.
        let mut b = IslandBuilder::new();
        let target = b.signal(Value::Int(0));
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::SignalSet {
            id: target,
            value: ClientExpr::Literal(Value::Int(1)),
        }]));
        let mut island = b.build();
        let r = DeadSignalElimination::new().run(&mut island);
        assert!(!r.changed, "set-only signal is still a writer");
        assert_eq!(island.signals.len(), 1);
    }
}
