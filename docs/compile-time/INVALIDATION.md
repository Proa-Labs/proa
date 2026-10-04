# Final-plan invalidation experiment

This branch makes final unit-map dependencies independent: each mapped unit
tracks `PROA_RSJS_FINAL_UNIT_INPUT_<descriptor-unit-id>`, containing a hash of
only its exact mapping entry. The final wrapper still validates the complete
map, exact plan bytes, coordinate, sources, generated inputs, and environment
before compiling that unit. Root artifact attestation is retained.

The mapping hash excludes unrelated entries and the map's global header.
Semantic-plan runners now also publish and track a complete content-addressed
projection for each unit. Expanded macros freshly validate the exact global
plan and compare their projection with its complete consumed selections.
Legacy transport-only runners retain the global plan ID/file-hash dependencies.

## Why the mapping witness alone is insufficient

Stable runtime coordinates and content-addressed asset generations now remove
global PlanId and dense owner ordinals from emitted identities. Cross-unit child
relationships, selected exports, and transitive helper implementations still
influence the Rust/HTML and JavaScript that a unit emits. Its mapping-entry hash
alone does not describe these selections.

A final crate may remain fresh only when its complete consumed selections remain
compatible with the new plan. The unit projection below supplies that dependency;
the mapping witness alone cannot authorize reuse.

## Architecture and verification

1. Give each owner and member site a stable semantic identity based on its
   definition/site coordinate rather than global traversal order. Treat
   reordered or duplicated sites explicitly; do not use a file-offset shortcut.
   Runtime site coordinates and member seed keys are now implemented below.
2. Compute an owner content hash over the actual generated payload and its
   transitive child/helper/protocol dependencies. Use it for asset and seed
   identities, consistently in server macros, JavaScript, and manifests. Asset
   generation identities are now implemented below.
3. Publish content-addressed unit projections containing exactly the final
   selections that unit's macros consume. Track each projection's hash in
   Cargo dep-info. Keep root publication/attestation tied to the complete plan.
   This is now implemented for semantic-plan runners.
4. For a unit shared by multiple owners, include the union of its required
   selections; do not assume a component belongs to only one owner. Projections
   include every local definition and all of its occurrences.
5. Prove that an edit to owner A leaves unrelated owner B's final crate fresh,
   while a shared helper/child/protocol change invalidates every dependent
   owner. Check emitted HTML asset IDs and run the generated JavaScript, not
   just Cargo's `fresh` flag. The Cargo acceptance checkpoint below covers
   unrelated owner reuse and shared helper/child generations.

The compiler matrix uses a real RSJS island in a separate unchanged consumer
crate (`--split-islands`) to expose global invalidation in the earlier checkpoint. The
`another_units_mapping_change_keeps_a_tracked_dependency_fresh` integration
test separately isolates the new mapping dependency with a proc-macro fixture.
It tests Cargo's actual dep-info/environment handling, without pretending that
the fixture proves owner-local RSJS plan reuse.

## Shared validation foundation

The pure final-plan parser, topology validation, and owner/helper selection now
live in `rsjs_ir::final_plan`. Macros retain filesystem/environment validation
and Cargo witnesses in `rsjs_macros/src/final_plan.rs`. This gives the CLI a
shared implementation for preparing future unit projections; a second parser
must not independently decide what final expansions consume. That initial move
left global plan witnesses and emitted identities unchanged. The next checkpoint
below replaces runtime site coordinates while retaining global Cargo witnesses.

The move preserves transport's recursively sorted JSON helper hashes, including
dependency graphs that enable `serde_json/preserve_order`. Tests check fixed
canonical bytes, scalar escaping, nested objects, and significant array order.

The remaining identity work extends beyond asset filenames. Hidden linked
render traits exchange `(plan_id, candidate_id)` tokens across crates; live
props, callbacks, and lexical SharedState providers also carry candidate IDs.
Owner JavaScript and NodeRef aliases must use the same replacement identities.
Keeping global dense candidate numbers in any of those outputs would make a
projection depend on unrelated graph traversal order.

A runtime occurrence can use the stable root semantic coordinate and its
validated structural edge path, with the component semantic coordinate included.
Repeated occurrences of the same definition therefore remain distinct. Dense
planner IDs can stay internal. If a compact numeric runtime ID is used, detect
collisions before publication and retain a full hash for seed/content identities.
The lexical scope identity should describe the logical root, rather than hash
the entire root forest's content: otherwise editing one separate owner would
still invalidate another owner under that same SSR root.

Owner content hashes must include normalized generated JavaScript and every
imported fragment generation. Fragment generations must cover the component
body, its required export union, transitive helper implementations, and compiler
protocol changes. Hashing owner JavaScript while retaining an unchanged fragment
URL after a helper-body change would publish a stale browser payload. A unit
projection must include every occurrence of each definition in that Cargo unit,
including occurrences shared by multiple owners, with global ordinals excluded.
Root publication and exact full-plan attestation remain global.

## Stable runtime coordinates checkpoint

The shared validator now derives a root scope hash from the canonical root
semantic key. Each occurrence hashes that scope, its component semantic key,
and its validated structural edge path, using explicit domain and length
framing. Cargo-unit generations, raw record IDs, global PlanId, and dense
candidate ordinals are excluded. A compact 64-bit coordinate is used by the
hidden server ABI and DOM marker; both compact collisions and duplicate full
coordinates fail closed before selection. Fused member seed keys use the full
256-bit occurrence hash.

Final macros use these coordinates for standalone entry, child calls, live
props, callbacks, owner/root metadata, and lexical SharedState providers.
Lookups verify the expected logical root scope against the selected static.
Owner JavaScript uses identical string coordinates for mounts, signals,
callbacks, lifecycle groups, and NodeRef aliases, avoiding JavaScript number
precision loss. Decimal SSR encoding uses a 20-byte stack buffer.

Tests cover unrelated root insertion, descriptor generation changes, repeated
definitions under distinct roots/paths, collisions, and the full u64 decimal
range. End-to-end final builds cover sync/async rendering, live props and
callbacks, query/mutation/conditional owners, and exact shared-state providers.
Every rendered marker in the live/query/mutation/conditional trial must match
an exact lookup in its generated owner JavaScript.
The NodeRef acceptance test executes generated JavaScript with Node 26.8.1,
checking exact input focus and disposal through a participant chain.
Workspace checking, focused library tests, and render performance lint pass.

This checkpoint retained global asset names and Cargo plan witnesses. The asset
generation checkpoint below replaces the names; complete per-unit projections
are still required before unrelated final crates can remain fresh.

## Asset generation checkpoint

Each component fragment now has a separate 256-bit generation ID. It covers
the exact descriptor unit and record, canonical component codegen, required
signal/lifecycle/NodeRef export and import unions, helper bindings, and complete
potential transitive helper closure. The closure deliberately includes helpers
before dead-code elimination. Descriptor-unit generations cover compiler and
dependency inputs, so a change in the component's own Cargo unit conservatively
refreshes its fragment even if that particular component body is unchanged.
The descriptor's source identity stays intact.

Each owner ID hashes its stable full site identity and actual generated owner
JavaScript, including its fragment imports and NodeRef aliases. Only the
owner's own registration name and root seed name are replaced with a fixed
placeholder while hashing, avoiding a self-reference cycle. Final emission
uses the resulting ID in those positions. Server seed tags, fragment exports,
owner imports, and manifest registrations all use the same generation IDs.

Tests prove that adding an unrelated root preserves owner IDs and exact owner
JavaScript; changing a child refreshes all importing owners; changing a
transitive shared helper refreshes its consumers; and adding a required export
changes the fragment generation without changing source identity. The 308 IR
and 435 macro unit tests pass with both default JSON features and
`serde_json/preserve_order`. Eleven end-to-end cases cover helper linking,
NodeRef focus/disposal in generated JavaScript (Node 24.20.0), sync/async sites,
live props/callbacks, explicit root cuts, and SharedState consumers/providers.
This checkpoint retained global Cargo plan witnesses until the unit projections
below were implemented.
No additional Pizza latency result is claimed for either identity checkpoint.

## Complete unit projection checkpoint

`rsjs_ir::final_plan::unit_projection_bytes_for_units` uses the shared validator
to derive canonical schema-1 projections. A projection covers every local
component record, every concrete occurrence, standalone admission, child calls,
live props/callbacks, lexical SharedState providers/requirements, NodeRef aliases,
fragment generations, and exact emitted owner modules. Helper-only units retain
their validated helper descriptors and codegen. Fragment generations commit the
required export unions and potential transitive helper bodies/signatures/bindings.
Global PlanId and dense owner/candidate ordinals are excluded. Serialized site
references carry full stable scope/site hashes and the matching compact runtime
coordinate; producer-local fields/values/controls remain unchanged.

The CLI publishes exact projection bytes under their SHA-256, retaining identical
plain files and rejecting conflicting content or redirected paths. It injects
`PROA_RSJS_FINAL_UNIT_PLAN_<descriptor-unit-id>` before Cargo starts, so Cargo can
compare the previous expansion's dep-info against current projection hashes.
The wrapper validates the mapped unit's file and hash, and exposes only its own
projection input to rustc. Macros verify that the immutable projection exactly
matches the selections freshly derived from the current full plan. Their retained
artifact witness now carries the unit dependency hash. Final wrapper policy 8
separates earlier final targets from this dependency policy.

Global map/plan validation, descriptor source/generated/environment freshness,
root artifact coordinates, launch dependency validation, and immutable root
publication remain intact. Fresh units rely on the runner's recomputed complete
projections; expanded units independently compare them with the exact plan bytes.
Cargo's ordinary Rust dependency invalidation still applies, so shared crates
remain a coarser boundary than individual owners. A fragment generation also
conservatively covers its own complete descriptor-unit generation.

`rsjs_unit_projection` runs the real cached descriptor/link/final pipeline through
six successful builds. Changing A's template and adding an unused root in its
crate changes the global plan while B remains fresh with identical owner asset
IDs and JavaScript. A shared helper edit refreshes both consumers. After both
owners consume one child, a child template edit refreshes both, and an unchanged
build keeps all four dependency crates fresh. Every build checks HTML owner IDs,
manifest fragment imports/exports, dynamic SSR values, and executes the actual
generated fragments with Node 26.8.1. The current B dep-info contains its unit
projection dependency and no global plan ID/file hash. Corrupting that current
projection is rejected before accepting even a warm Cargo artifact.

The fixture's changed islands pass Proa format verification, lint with warnings
denied, and check. A new Pizza rebenchmark is still required before attributing
any latency change to this architecture.

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
