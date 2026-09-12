// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! IR rewriter pass infrastructure.
//!
//! Passes consume an [`crate::island::Island`] mutably and rewrite it
//! in place. Each pass is idempotent by construction: running it
//! twice on the same IR produces the same result on the second run.
//! The [`PassRunner`] applies a list of passes in order until a fixed
//! point, so passes that expose new opportunities for earlier passes
//! (e.g. dedup that removes references, allowing dead-code to fire)
//! converge automatically.
//!
//! v0.1 ships two concrete passes:
//!
//! - [`handler_dedup::HandlerDedup`] — merge structurally-identical
//!   event handlers and rewrite the binding side-table to point at
//!   the canonical id.
//! - [`dead_signal_elimination::DeadSignalElimination`] — drop
//!   signals with no readers and no writers anywhere in the IR.
//!
//! Static consolidation is intentionally NOT a
//! starting pass: rsjs has no static-text emissions of its own to
//! merge (proa's `html!` lowering already consolidates static text in
//! the parse tree).
//!
//! # Pure-IR contract
//!
//! Passes here are pure IR-to-IR rewrites. The JS emitter
//! (`crate::emit_js`) does not run them implicitly: callers that want
//! the optimised JS output are expected to construct a [`PassRunner`]
//! (or use [`default_passes`]) and run it before calling
//! `emit_js::emit`. This keeps `emit_js` deterministic with respect
//! to its input and avoids silently changing the M2 golden output.

use crate::island::Island;

pub mod dead_signal_elimination;
pub mod handler_dedup;

pub use dead_signal_elimination::DeadSignalElimination;
pub use handler_dedup::HandlerDedup;

/// A single IR-rewriter pass.
///
/// Passes are stateless beyond their constructor parameters: the
/// [`PassRunner`] owns the pass instances and may call [`Self::run`]
/// multiple times on the same instance against the same or different
/// islands. Implementations MUST be idempotent: running [`Self::run`]
/// on an island twice in a row, the second invocation MUST report
/// `changed: false` (no further work to do) and leave the island
/// byte-identical to its post-first-run state.
pub trait IslandPass {
    /// A short, stable identifier for the pass. Used for diagnostic
    /// output and for the result list returned by
    /// [`PassRunner::run_all`]. Convention: snake_case, matches the
    /// containing module name (e.g. `"handler_dedup"`).
    fn name(&self) -> &'static str;

    /// Run the pass against `island`. Returns whether the IR changed
    /// and per-pass statistics.
    fn run(&self, island: &mut Island) -> PassResult;
}

/// Result of running an [`IslandPass`] once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PassResult {
    /// Whether the pass mutated the IR. The runner reaches a fixed
    /// point when every pass in a round reports `changed: false`.
    pub changed: bool,
    /// Counters describing what the pass did this run.
    pub stats: PassStats,
}

/// Counters for a single pass invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStats {
    /// Items removed from the IR (e.g. dead signals dropped, handlers
    /// merged away).
    pub items_removed: usize,
    /// Items merged into a canonical entry (e.g. duplicate handlers
    /// folded into a single handler).
    pub items_merged: usize,
    /// Items the pass inspected. A diagnostic counter; not all passes
    /// populate it.
    pub items_visited: usize,
}

/// Maximum number of fixed-point rounds the runner will execute
/// before bailing. The runner stops earlier as soon as a full round
/// reports no changes; the cap exists only as a safety net against a
/// pass that violates the idempotence contract and oscillates. Ten
/// rounds is generous: in practice every pair of passes in v0.1
/// converges in one or two rounds on realistic islands.
const MAX_ROUNDS: usize = 10;

/// Applies a list of passes to an island until a fixed point.
///
/// The runner is constructed empty, populated via
/// [`PassRunner::add`], and consumed by [`PassRunner::run_all`]. Each
/// round iterates the registered passes in declared order. The runner
/// terminates when either:
///
/// - a complete round reports `changed: false` for every pass, or
/// - ten rounds have elapsed (the internal `MAX_ROUNDS` guard against a
///   bugged pass).
///
/// Pass ordering matters. See [`default_passes`] for the canonical
/// v0.1 order and the rationale.
#[derive(Default)]
pub struct PassRunner {
    passes: Vec<Box<dyn IslandPass>>,
}

impl PassRunner {
    /// Construct an empty runner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pass at the end of the pass list. Returns `self`
    /// so callers can chain builder-style.
    // The name `add` is intentional: this is a builder-style method,
    // not the `std::ops::Add::add` arithmetic operator. The clippy
    // lint flags any inherent method named `add` that does not match
    // the trait shape; for a builder this is a false positive.
    #[allow(clippy::should_implement_trait)]
    pub fn add<P: IslandPass + 'static>(mut self, pass: P) -> Self {
        self.passes.push(Box::new(pass));
        self
    }

    /// Run every registered pass against `island` until a fixed
    /// point. Returns the per-pass results from the *final* round
    /// (the one that reported no changes), so callers can verify
    /// idempotence and inspect the final stat counters.
    ///
    /// The returned `Vec` has one entry per registered pass, in the
    /// order they were added. Each tuple is `(pass_name, result)`.
    pub fn run_all(&self, island: &mut Island) -> Vec<(String, PassResult)> {
        let mut last: Vec<(String, PassResult)> = self
            .passes
            .iter()
            .map(|p| (p.name().to_string(), PassResult::default()))
            .collect();

        for _round in 0..MAX_ROUNDS {
            let mut any_changed = false;
            for (i, pass) in self.passes.iter().enumerate() {
                let result = pass.run(island);
                if result.changed {
                    any_changed = true;
                }
                last[i] = (pass.name().to_string(), result);
            }
            if !any_changed {
                return last;
            }
        }
        last
    }
}

impl std::fmt::Debug for PassRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassRunner")
            .field(
                "passes",
                &self.passes.iter().map(|p| p.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// A [`PassRunner`] preloaded with the v0.1 default pass list:
/// handler dedup followed by dead signal elimination.
///
/// Ordering rationale: `handler_dedup` may make signals dead by
/// removing references inside handlers it merges away (in v0.1 the
/// merged-away handlers' references were structurally identical to
/// the canonical ones, so this rarely matters; the ordering
/// nonetheless leaves the most savings exposed once richer passes
/// land in M9+). Running `dead_signal_elimination` after dedup means
/// the second pass sees the smallest possible IR.
pub fn default_passes() -> PassRunner {
    PassRunner::new()
        .add(HandlerDedup::new())
        .add(DeadSignalElimination::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-op pass for testing the runner.
    struct NoopPass;
    impl IslandPass for NoopPass {
        fn name(&self) -> &'static str {
            "noop"
        }
        fn run(&self, _island: &mut Island) -> PassResult {
            PassResult::default()
        }
    }

    /// A pass that always reports `changed: true` for the first N
    /// invocations, then `changed: false`. Used to exercise the
    /// fixed-point loop and the iteration cap.
    struct CountdownPass {
        counter: std::cell::Cell<usize>,
    }
    impl CountdownPass {
        fn new(rounds: usize) -> Self {
            Self {
                counter: std::cell::Cell::new(rounds),
            }
        }
    }
    impl IslandPass for CountdownPass {
        fn name(&self) -> &'static str {
            "countdown"
        }
        fn run(&self, _island: &mut Island) -> PassResult {
            let n = self.counter.get();
            if n == 0 {
                return PassResult::default();
            }
            self.counter.set(n - 1);
            PassResult {
                changed: true,
                stats: PassStats {
                    items_visited: 1,
                    ..PassStats::default()
                },
            }
        }
    }

    #[test]
    fn empty_runner_is_a_no_op() {
        let mut island = Island::new();
        let runner = PassRunner::new();
        let results = runner.run_all(&mut island);
        assert!(results.is_empty());
    }

    #[test]
    fn runner_stops_at_fixed_point() {
        let mut island = Island::new();
        let runner = PassRunner::new().add(NoopPass);
        let results = runner.run_all(&mut island);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "noop");
        assert!(!results[0].1.changed);
    }

    #[test]
    fn runner_iterates_until_pass_settles() {
        // Pass reports changed for 3 rounds, then settles. The
        // runner should report the final (settled) result.
        let mut island = Island::new();
        let runner = PassRunner::new().add(CountdownPass::new(3));
        let results = runner.run_all(&mut island);
        assert_eq!(results.len(), 1);
        assert!(!results[0].1.changed, "final round must be the settled one");
    }

    #[test]
    fn runner_caps_at_max_rounds() {
        // A pass that never settles. The runner must terminate at
        // MAX_ROUNDS rather than spin forever.
        let mut island = Island::new();
        // A high countdown value beyond MAX_ROUNDS: 100 > 10.
        let runner = PassRunner::new().add(CountdownPass::new(100));
        let results = runner.run_all(&mut island);
        // The final reported state will still show changed=true
        // because the pass never settled within the cap.
        assert_eq!(results.len(), 1);
        assert!(results[0].1.changed);
    }
}
