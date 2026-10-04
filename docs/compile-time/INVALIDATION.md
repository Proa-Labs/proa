# Final-plan invalidation contract

A final crate can remain fresh only if its complete consumed selections remain
compatible with the new plan. The mapping-entry hash alone cannot represent
cross-unit children, exports, helpers or runtime protocols.

Each unit tracks `PROA_RSJS_FINAL_UNIT_INPUT_<descriptor-unit-id>` for its exact
mapping entry, excluding unrelated entries/global headers. Semantic-plan runners
also publish a complete content-addressed unit projection. The wrapper validates
full map/plan authority before compiling; root artifact attestation remains global.
Legacy transport-only runners retain global plan ID/file-hash dependencies.

## Shared validation

The pure final-plan parser, topology validation and owner/helper selection live
in `rsjs_ir::final_plan`, shared by the CLI and macros. Filesystem/environment
validation and Cargo witnesses stay in `rsjs_macros/src/final_plan.rs`.
Most of the large new IR file is moved validator code, not new implementation.
Recursively sorted JSON hashes remain canonical with `serde_json/preserve_order`;
arrays retain their significant ordering.

## Stable runtime coordinates

The shared validator hashes the canonical root semantic key into a scope.
Each occurrence hashes that scope, its component semantic key and validated
structural edge path with domain/length framing. Cargo-unit generations, raw
record IDs, global PlanId and dense candidate ordinals are excluded. Repeated
occurrences remain distinct. Duplicate full hashes and compact 64-bit collisions
fail closed. Fused member seed keys retain the full 256-bit hash.

Server standalone/child calls, live props, callbacks, root metadata and lexical
SharedState providers use the same coordinates. Lookups verify logical root
scope. Owner JavaScript uses matching string coordinates for mounts, signals,
callbacks, lifecycle groups and NodeRef aliases, avoiding numeric precision loss.
Decimal SSR encoding uses a 20-byte stack buffer.

## Asset generations

Each fragment's 256-bit generation covers its exact descriptor unit/record,
canonical component codegen, required signal/lifecycle/NodeRef export/import
unions, helper bindings and the potential transitive helper closure before
elimination. Descriptor-unit generations include compiler/dependency inputs.
A change within a component's own Cargo unit therefore conservatively refreshes
its fragment even when that particular body is unchanged.

Each owner ID hashes its full stable site identity and actual emitted JavaScript,
including fragment imports and aliases. Its own registration/root seed names
are normalized while hashing to avoid a self-reference cycle, then filled with
the resulting ID. Server seeds, exports, imports and manifest registrations use
these same generations. Unrelated root insertion preserves identities; a changed
child/helper/export refreshes every affected importing owner.

## Complete unit projections

`unit_projection_bytes_for_units` derives canonical schema-1 projections from
the shared validator. A projection includes every local component definition and
occurrence, standalone admission, child calls, live props/callbacks, lexical
providers/requirements, aliases, fragment generations and exact owner modules.
Shared components include all required selections across owners. Helper-only
units retain their validated descriptors/codegen. Global plan and dense traversal
IDs are excluded; site references carry stable full hashes and runtime coordinates.

The CLI publishes exact projection bytes under SHA-256, retains identical plain
files and rejects conflicting bytes or redirected paths. It injects
`PROA_RSJS_FINAL_UNIT_PLAN_<descriptor-unit-id>` before Cargo starts so dep-info
can compare the previous expansion against current projection hashes. The
wrapper validates the mapped file/hash and exposes only its unit's input.
Macros compare immutable projections with complete selections from current plan
bytes. Pure derivation reuse is bounded and keyed by fresh full-plan SHA-256,
plan ID and unit ID; a hit cannot authorize changed files or substitute units.
Final wrapper policy 8 separates this dependency policy from older targets.

Source/generated/environment freshness, complete map/plan checks, root
coordinates, launch validation and immutable publication remain enforced.
Fresh units rely on recomputed complete projections; expanded units independently
compare against the full plan. Normal Rust dependency invalidation still applies.

## Acceptance

`rsjs_unit_projection` exercises the actual cached descriptor/link/final pipeline
through six builds. An A template edit plus unused root insertion changes the
global plan while unrelated B remains fresh with identical asset IDs/JavaScript.
Shared helper and child edits invalidate both owners. An unchanged build keeps
all four dependency crates fresh. Each build checks HTML, manifest exports/imports,
dynamic SSR and generated JavaScript execution. B's dep-info contains its unit
projection rather than global plan witnesses; corrupting that projection rejects
warm artifact acceptance.

Validated-plan regressions cover activation, conditional branch, NodeRef alias
and lexical provider changes: parent/child projections invalidate while an
unrelated owner stays unchanged. Other acceptance covers sync/async sites,
live props/callbacks and NodeRef focus/disposal. See [REVIEW.md](REVIEW.md) for
exact current checks and remaining independent-review/CI gates.

## Upstream compatibility follow-ups

Experimental Cargo layouts exposed existing assumptions about one artifact per
extern and build-script environment lookup beside `OUT_DIR`. Future support must
represent exact artifact roles across hashing, sidecars and manifests, and use
an authoritative build-script observation handoff. Dropping metadata inputs or
skipping environment validation would weaken freshness. Neither assumption is
silently relaxed by this branch.
