// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `handler_dedup` — merge structurally-identical event handlers.
//!
//! Two `<button on_click={...}>` bindings whose handlers are
//! byte-equivalent (same statement sequence, same expressions, same
//! captured signals) ought to share one handler in the emitted JS.
//! This pass identifies such duplicates in
//! [`crate::island::Island::handlers`], picks a canonical handler
//! per equivalence class (the lowest-id one), rewrites every
//! [`crate::bindings::Bindings::event_bindings`] entry that pointed
//! at a duplicate to point at the canonical id, and drops the
//! orphaned [`crate::reactive::EventHandler`] entries.
//!
//! # Equivalence
//!
//! Two handlers are equivalent iff their stored Client IR v3 blocks are
//! equal (structural `PartialEq` over [`crate::client::ClientBlock`]).
//!
//! Equivalence is over signal *ids*, not signal *values*. Two
//! handlers that both write `count.set(0)` are merged; two handlers
//! that write `count.set(0)` and `other.set(0)` (different signals)
//! are not.
//!
//! # Identity preservation
//!
//! [`crate::reactive::EventHandler::id`] is treated as part of the
//! handler's identity for the purposes of *external references*: the
//! pass leaves the canonical handler's id unchanged. The id is the
//! handler's slot in the `handlers` vector before the pass runs;
//! after the pass, the orphaned slots are *removed*, but the
//! surviving handlers retain their original ids. This means the
//! `handlers` vector is no longer dense (there will be gaps), but
//! existing dep collectors and emitters key off the ids stored in
//! [`crate::reactive::EventHandler::id`], not the vector position.
//! The M2 emitter sorts handlers in iteration order (by their
//! position in the vector) and uses `id.0` for the emitted symbol
//! name; both of those continue to work unchanged.
//!
//! # Idempotence
//!
//! Running the pass twice in a row is a no-op on the second run: the
//! first run leaves no two structurally-equal handlers in the IR, so
//! the equivalence-class walk on the second run finds nothing to
//! merge.

use std::collections::HashMap;

use crate::island::Island;
use crate::reactive::EventHandler;
use crate::types::EventHandlerId;

use super::{IslandPass, PassResult, PassStats};

/// The `handler_dedup` pass. See the module-level docs.
#[derive(Debug, Default, Clone, Copy)]
pub struct HandlerDedup;

impl HandlerDedup {
    /// Construct a new pass instance. The pass is stateless; the
    /// constructor exists for symmetry with stateful passes that may
    /// land later.
    pub fn new() -> Self {
        Self
    }
}

impl IslandPass for HandlerDedup {
    fn name(&self) -> &'static str {
        "handler_dedup"
    }

    fn run(&self, island: &mut Island) -> PassResult {
        let visited = island.handlers.len();
        if visited < 2 {
            return PassResult {
                changed: false,
                stats: PassStats {
                    items_visited: visited,
                    ..PassStats::default()
                },
            };
        }

        // Build the canonical-id map: for each handler, find the
        // first earlier handler in the `handlers` vector that is
        // structurally equal, and pick that handler's id as the
        // canonical id. Round-3 review #17: under the dense-id
        // invariant (vector index == id) this is equivalent to
        // "lowest id"; the previous comment claimed the latter
        // explicitly, but the code is anchored to vector iteration
        // order. If a future change loosens the dense-id invariant
        // (e.g. ids carrying generation counters), this would still
        // pick "first by source order" rather than literal min(id);
        // that is the intended semantics.
        //
        // This is an O(n^2) walk; for v0.1 islands (handful of
        // handlers) this is fine, and even at M7+ scale it stays
        // well below the noise floor of macro expansion. If it ever
        // becomes a hotspot we can hash handler bodies and bucket
        // them; the surface stays the same.
        // Equivalence keys on the Client IR v3 body AND the debounce
        // window: two handlers with identical bodies but different
        // `debounce(..., delay_ms)` wrappers (or one wrapped, one not) must
        // stay distinct because the wrapper is part of the emitted
        // declaration.
        let keys = island
            .handlers
            .iter()
            .map(|handler| (handler.body_ir(), handler.debounce_ms()))
            .collect::<Vec<_>>();
        let mut canonical: HashMap<EventHandlerId, EventHandlerId> = HashMap::new();
        for i in 0..island.handlers.len() {
            let h_i = &island.handlers[i];
            // Look for a prior handler this one duplicates.
            let mut canon = h_i.id;
            for (prev_idx, h_prev) in island.handlers.iter().take(i).enumerate() {
                if keys[prev_idx] == keys[i] {
                    canon = h_prev.id;
                    break;
                }
            }
            canonical.insert(h_i.id, canon);
        }

        let merged_count = canonical
            .iter()
            .filter(|(orig, canon)| orig != canon)
            .count();

        if merged_count == 0 {
            return PassResult {
                changed: false,
                stats: PassStats {
                    items_visited: visited,
                    ..PassStats::default()
                },
            };
        }

        // Rewrite event_bindings to point at the canonical handler.
        for binding_handler in island.bindings.event_bindings.values_mut() {
            if let Some(canon) = canonical.get(binding_handler) {
                *binding_handler = *canon;
            }
        }

        // Drop the orphaned handlers, keeping only the canonical
        // entries. A handler is canonical iff its id maps to itself
        // in the canonical table.
        let before = island.handlers.len();
        island
            .handlers
            .retain(|h: &EventHandler| canonical.get(&h.id).copied() == Some(h.id));
        let removed = before - island.handlers.len();

        PassResult {
            changed: true,
            stats: PassStats {
                items_removed: removed,
                items_merged: merged_count,
                items_visited: visited,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::EventName;
    use crate::builder::IslandBuilder;
    use crate::client::{ClientBinaryOp, ClientBlock, ClientExpr, ClientSimpleClosure, ClientStmt};
    use crate::types::NodeId;
    use crate::value::Value;

    /// Build a `count.update(|c| c + 1)`-style handler block.
    fn increment_block(count: crate::types::SignalId) -> ClientBlock {
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
        )])
    }

    /// Build a `count.update(|c| c - 1)` handler block. Distinct from
    /// `increment_block`: different operator.
    fn decrement_block(count: crate::types::SignalId) -> ClientBlock {
        ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            count,
            ClientSimpleClosure::new(
                "c",
                ClientExpr::binary(
                    ClientBinaryOp::Sub,
                    ClientExpr::signal_read(count),
                    ClientExpr::int(1),
                ),
            ),
        )])
    }

    #[test]
    fn merges_two_structurally_identical_handlers() {
        // Two buttons, both `count.update(|c| c + 1)`.
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let h1 = b.handler_from_ir(increment_block(count));
        let h2 = b.handler_from_ir(increment_block(count));

        let root = b.element("div", None);
        let btn_a = b.element("button", Some(root));
        let btn_b = b.element("button", Some(root));
        b.bind_event(btn_a, EventName::Click, h1);
        b.bind_event(btn_b, EventName::Click, h2);
        b.set_root(root);
        let mut island = b.build();

        let pass = HandlerDedup::new();
        let result = pass.run(&mut island);

        assert!(result.changed, "pass must report changed=true");
        assert_eq!(result.stats.items_merged, 1);
        assert_eq!(result.stats.items_removed, 1);
        assert_eq!(
            island.handlers.len(),
            1,
            "one canonical handler should remain"
        );
        assert_eq!(island.handlers[0].id, h1, "canonical id is the first one");

        // Both bindings now point at the canonical handler.
        for ((_, _), bound) in &island.bindings.event_bindings {
            assert_eq!(*bound, h1);
        }
    }

    #[test]
    fn leaves_distinct_handlers_alone() {
        // Counter fixture pattern: two handlers with opposite
        // operators (`c + 1` vs `c - 1`). Should not merge.
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let inc = b.handler_from_ir(increment_block(count));
        let dec = b.handler_from_ir(decrement_block(count));

        let root = b.element("div", None);
        let btn_a = b.element("button", Some(root));
        let btn_b = b.element("button", Some(root));
        b.bind_event(btn_a, EventName::Click, inc);
        b.bind_event(btn_b, EventName::Click, dec);
        b.set_root(root);
        let mut island = b.build();

        let before = island.clone();
        let pass = HandlerDedup::new();
        let result = pass.run(&mut island);

        assert!(!result.changed, "distinct handlers must not be merged");
        assert_eq!(result.stats.items_merged, 0);
        assert_eq!(result.stats.items_removed, 0);
        assert_eq!(island, before, "IR must be unchanged");
    }

    #[test]
    fn idempotent_when_run_twice() {
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let h1 = b.handler_from_ir(increment_block(count));
        let h2 = b.handler_from_ir(increment_block(count));
        let h3 = b.handler_from_ir(increment_block(count));

        let root = b.element("div", None);
        let n1 = b.element("button", Some(root));
        let n2 = b.element("button", Some(root));
        let n3 = b.element("button", Some(root));
        b.bind_event(n1, EventName::Click, h1);
        b.bind_event(n2, EventName::Click, h2);
        b.bind_event(n3, EventName::Click, h3);
        b.set_root(root);
        let mut island = b.build();

        let pass = HandlerDedup::new();
        let r1 = pass.run(&mut island);
        assert!(r1.changed);
        assert_eq!(island.handlers.len(), 1);

        let snapshot = island.clone();
        let r2 = pass.run(&mut island);
        assert!(!r2.changed);
        assert_eq!(island, snapshot, "second run must not mutate the IR");
    }

    #[test]
    fn merges_three_into_one_with_correct_canonical() {
        // Three identical handlers: all should collapse to the first.
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        let h1 = b.handler_from_ir(increment_block(count));
        let _h2 = b.handler_from_ir(increment_block(count));
        let _h3 = b.handler_from_ir(increment_block(count));

        let root = b.element("div", None);
        b.set_root(root);

        // Wire each handler to its own pseudo node so the binding
        // table has three distinct entries.
        let n_a = b.element("button", Some(root));
        let n_b = b.element("button", Some(root));
        let n_c = b.element("button", Some(root));
        b.bind_event(n_a, EventName::Click, h1);
        b.bind_event(n_b, EventName::Click, EventHandlerId(1));
        b.bind_event(n_c, EventName::Click, EventHandlerId(2));
        let mut island = b.build();

        let pass = HandlerDedup::new();
        let r = pass.run(&mut island);
        assert_eq!(r.stats.items_merged, 2);
        assert_eq!(r.stats.items_removed, 2);
        assert_eq!(island.handlers.len(), 1);
        assert_eq!(island.handlers[0].id, h1);

        // All three bindings collapse to the canonical id.
        let bound_ids: Vec<EventHandlerId> = (0..3)
            .filter_map(|n| {
                island
                    .bindings
                    .event_bindings
                    .get(&(NodeId(1 + n as u32), EventName::Click))
                    .copied()
            })
            .collect();
        assert_eq!(bound_ids.len(), 3);
        for id in bound_ids {
            assert_eq!(id, h1);
        }
    }

    #[test]
    fn empty_or_single_handler_is_a_no_op() {
        // Empty.
        let mut empty = Island::new();
        let r = HandlerDedup::new().run(&mut empty);
        assert!(!r.changed);

        // Single handler.
        let mut b = IslandBuilder::new();
        let count = b.signal(Value::Int(0));
        b.handler_from_ir(increment_block(count));
        let mut island = b.build();
        let r = HandlerDedup::new().run(&mut island);
        assert!(!r.changed);
        assert_eq!(island.handlers.len(), 1);
    }
}
