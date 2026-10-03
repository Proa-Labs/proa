# Final-plan invalidation experiment

This branch makes final unit-map dependencies independent: each mapped unit
tracks `PROA_RSJS_FINAL_UNIT_INPUT_<descriptor-unit-id>`, containing a hash of
only its exact mapping entry. The final wrapper still validates the complete
map, exact plan bytes, coordinate, sources, generated inputs, and environment
before compiling that unit. Root artifact attestation is retained.

The mapping hash excludes unrelated entries and the map's global header. The
plan ID and exact plan-file hash remain separately tracked by RSJS expansions.
This distinction is necessary: a source-only change in another unit can change
the global map without changing this unit's lookup, but a changed plan can
still change this unit's emitted Rust/HTML.

## Why removing the global plan witness is incorrect

In `rsjs_macros/src/final_plan.rs`, `owner_asset_id` uses the global PlanId and
an owner ordinal; `member_seed_key` uses the global PlanId and a candidate
ordinal. Dense owner, candidate, occurrence, and control IDs can change when
an unrelated definition is inserted. Cross-unit child relationships and fused
helper implementations also influence the selected owner's JavaScript.

An old final crate can therefore refer to an asset or member seed that the new
global plan no longer emits. A cache hit is not sufficient evidence that its
output remains compatible. Retaining the global witnesses avoids that bug.
The branch does **not** claim complete owner-local invalidation.

## Next architecture experiment

1. Give each owner and member site a stable semantic identity based on its
   definition/site coordinate rather than global traversal order. Treat
   reordered or duplicated sites explicitly; do not use a file-offset shortcut.
2. Compute an owner content hash over the actual generated payload and its
   transitive child/helper/protocol dependencies. Use it for asset and seed
   identities, consistently in server macros, JavaScript, and manifests.
3. Publish content-addressed unit projections containing exactly the final
   selections that unit's macros consume. Track each projection's hash in
   Cargo dep-info. Keep root publication/attestation tied to the complete plan.
4. For a unit shared by multiple owners, include the union of its required
   selections; do not assume a component belongs to only one owner.
5. Prove that an edit to owner A leaves unrelated owner B's final crate fresh,
   while a shared helper/child/protocol change invalidates every dependent
   owner. Check emitted HTML asset IDs and run the generated JavaScript, not
   just Cargo's `fresh` flag.

The compiler matrix uses a real RSJS island in a separate unchanged consumer
crate (`--split-islands`) to expose the remaining global invalidation. The
`another_units_mapping_change_keeps_a_tracked_dependency_fresh` integration
test separately isolates the new mapping dependency with a proc-macro fixture.
It tests Cargo's actual dep-info/environment handling, without pretending that
the fixture proves owner-local RSJS plan reuse.

## A separate upstream Cargo compatibility finding

The 2026-08-26 nightly Cargo supplies both an `.rlib` and a separate `.rmeta`
for a single build-dependency extern name, with `-Z embed-metadata=no`. The
current normalized invocation and sidecar dependency map assume one exact
artifact per extern name and reject this as conflicting artifacts. The failure
occurs even without next-solver/frontend/backend flags. Stable 1.99 passes.

Keep the rejection until the dependency schema represents multiple exact roles
per extern. Extend unit hashing, sidecars, current-input validation, Cargo
artifact joins, and manifest edges together; neither silently dropping `.rmeta`
nor picking the last argument preserves exact input freshness. The compiler
comparison retains this failure and uses an earlier pinned nightly to isolate
compiler features from that experimental Cargo layout.

The 2026-08-01 nightly exposes another existing assumption: locating build-script
`rustc-env` declarations beside `OUT_DIR`. That lookup fails for its layout.
Future support should use Cargo's authoritative `build-script-executed` data
with a correctly ordered wrapper handoff, preserving exact declarations and
generated-input attestation. The direct Cargo SSR trials isolate compiler
features without bypassing either wrapper freshness check.
