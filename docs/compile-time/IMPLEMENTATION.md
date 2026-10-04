# Development rebuild implementation

Branch `perf/dev-rebuild-pipeline`, based on fetched `origin/main` at `ab6e5cd`
(2026-10-03), lives in a separate worktree. The original checkout and its
preexisting edits are preserved. This is an investigation branch with production
cache changes, complete unit-plan dependencies, and explicitly scoped prototypes.
Cargo crate boundaries and platform/backend adapter limits remain conservative.

## Six investigation outcomes

| Priority | Outcome | Remaining limit |
| --- | --- | --- |
| 1 | Safe identical cache publication; measured actual pizza example waterfall | Assets/browser paint are excluded |
| 2 | Immutable validated byte snapshots, paired input hashes, bounded parsed-record reuse | Every cycle still freshly validates bytes and inputs |
| 3 | Complete multi-object carriers with incremental codegen; partial-edit/removal witnesses retained | Native host cc and rlibs supported; other cases retain conservative single-object fallback |
| 4 | Independent mapping and complete unit-plan witnesses; real unrelated-owner Cargo reuse; stable runtime and asset identities | Ordinary Cargo crate dependencies and complete own-unit fragment generations remain conservative |
| 5 | Repository/current stable and isolated nightly feature trials, retaining failures | See COMPILERS.md for actual compatibility and performance results |
| 6 | Explicit hot-static implementation and HTTP correctness/latency proof | Development-only surface, render overhead, new-session cold builds |

## Cache publication and validated data

`atomic_write_replaceable` first validates the plain parent directory and file,
then compares the complete on-disk representation, including its newline.
Identical publications keep their existing inode and require no new durability
flush. Changed pointers retain atomic replacement and directory/file flushing.
Hard-link tests distinguish true reuse from rewriting identical bytes; symlink
files and redirected parents remain rejected.

The cache lease now retains `Arc<[u8]>` snapshots of every validated descriptor
carrier. Both descriptor and link hash domains are computed from the same bytes;
manifest assembly and extraction consume them without rereading the object.
Paired dependency hashes are reused within the admitted cycle. Parsed transport
records are cached by exact path/content generation, bounded to 2048 entries,
and consulted only after both current authority checks succeed. A prior parsed
record never authorizes stale or corrupted filesystem content.

Latest-main dev-profile/300ms watcher medians: standalone 6.914s and copied pizza
6.974s. Initial branch checkpoint: standalone 3.999s and pizza 4.854s. The earlier cache/snapshot
checkpoint alone was 3.903s on the standalone consumer. These three-sample
serial trials are separate from the historical isolated prototype.

## Pizza target follow-up

The finer waterfall identified bulk SHA-256 validation as the largest remaining
setup cost. The CLI now uses sha2 0.11 runtime dispatch for artifact/cache/wrapper
hashing, preserving the existing formats and digest domains. Independent cached
units validate observed inputs in parallel with deterministic error ordering.
Graceful shutdown still allows two seconds, but successful exit is observed on a
5ms poll. The watcher defaults to a 200ms quiet window and bounds its next poll
by the pending quiet-window deadline. `--debounce-ms` still overrides it.

Pizza normal-dev medians are now 2.985s at the original 300ms, 2.895s with actual
CLI defaults, and 2.908s for a Rust helper-body edit with those defaults. Each is
three serialized warm changed HTTP responses, assets skipped. Complete owner-local
plan reuse remains unfinished. See [PIZZA_TARGET.md](PIZZA_TARGET.md) for the
controls, detailed waterfalls, limitations, reproduction, and current checks.

## Complete descriptor carriers

An rlib is an authoritative bundle of all its codegen objects. For supported
native host cc/gcc/clang links, an adapter captures the exact explicit objects
before rustc removes temporary files and forwards the original linker arguments
unchanged. It writes an ordinary archive for the existing transport extractor;
no rustc-private incremental directories are scanned.

The wrapper preserves Cargo's incremental flags and codegen partitioning for
these carriers. Metadata-only checks, unsupported targets/linker protocols,
multiple crate types, and explicitly routed link outputs retain the single-object
carrier (one CGU), while preserving incremental queries. Indirect response and
file-list protocols are rejected instead of publishing an incomplete archive.
Sidecar schema 11 and descriptor target policy 5 exclude old-policy reuse.

The library/executable integration probe uses four CGUs and three source
generations. It retains unused witnesses across edits and removes only the
removed module's witness. Archive/object packaging can differ after a complete
recovery, so recovery checks the currently published exact generation rather
than requiring the old archive bytes to reappear.

The incremental checkpoint's warm dev-profile build was 3.459s, versus 3.693s
for cache/snapshot reuse alone; descriptor Cargo dropped from about 1.23s to
0.82s. The corresponding watcher medians (3.913s versus 3.903s) show no additional
end-to-end gain. Larger archive validation cost matters.

## Final invalidation and static edits

Final wrapper policy 8 validates full map/plan authority and exposes one exact
mapping-entry hash plus a complete canonical selection hash per unit to Cargo.
Expanded macros independently verify their projection against the current full
plan. Legacy transport-only runners retain global plan witnesses. Root artifact
attestation, launch validation, and publication still use the complete plan.

The next checkpoint shares plan validation and owner/helper selection in
`rsjs_ir::final_plan` and replaces emitted dense candidate IDs/global-plan scope
tokens with logical root/site identities. Server child calls, SharedState
provider occurrences, and owner JavaScript now agree on those stable coordinates;
collisions fail closed. Fused member seed keys use full occurrence hashes.
Fragment generations now cover descriptor/compiler inputs, required exports,
and potential transitive helpers. Owner IDs cover actual emitted JavaScript
with its own registration and seed names normalized, plus its stable site
identity. Server seed tags and manifest imports use the same generations.
Unrelated roots preserve owner IDs and exact JavaScript, while changed children,
helpers, or required exports refresh their consumers. These identity checkpoints
retained global Cargo witnesses until the projection work below. They are
correctness and architecture work, with no new latency claim.

Complete unit projections now cover all local definitions and occurrences,
including components shared by multiple owners and helper-only units. The real
Cargo acceptance fixture preserves an unrelated B crate after an A edit plus
unused root insertion, refreshes both consumers after shared helper/child changes,
and checks unchanged reuse, generated JavaScript execution, HTML/manifest asset
coherence, dep-info dependencies, and corrupted projection rejection. See
[INVALIDATION.md](INVALIDATION.md) for the exact dependency and validation contract.
Pizza must be rebenchmarked after this architecture change.

The explicit hot-static prototype uses the shared checked DSL and consolidation
walk. Its file fingerprint masks only eligible template string values; dynamic
Rust and markup structure remain exact. It publishes a coherent template
snapshot atomically, reloads without Cargo/restart for compatible edits, and
stages incompatible candidates without changing the active server's manifest.
The compile environment stays stable within a watcher session. Failed builds
retain prior output; normal macro emission remains the literal fast path.

The release-CLI static-edit trial measured 0.129s median at 100ms debounce.
HTTP acceptance covers escaping, static attributes, dynamic Rust fallback,
compile-failure retention, and repair. Its debug-CLI timings overlapped another
compilation and are correctness evidence only. See HOT_STATIC.md for usage and
limits, especially runtime JSON reads and cold builds across watcher sessions.
The final debug-CLI HTTP regression additionally verifies generated Rust from
pre-build hooks at startup and after a fallback rebuild; all seven cases pass.

## Validation

Passed checks:

- `cargo check --workspace --all-targets`.
- CLI/IR/RSJS-macro libraries: 680/311/437 tests for the projection work;
  IR/macro checks also pass with `serde_json/preserve_order`.
- Earlier unchanged core/DSL/Proa-macro libraries: 268/15/37 tests.
- Descriptor wrapper integration: 11 tests; descriptor cache integration: 12.
- Final compilation integration: 4 tests, including mapping reuse.
- Earlier lifecycle dev integration: 1; linked helper acceptance: 4 (including
  generated JavaScript execution and cross-crate imports).
- Projection acceptance: one real fixture across six successful builds plus
  a corrupted-publication rejection; changed islands pass Proa fmt/lint/check.
- Current final/linked-site/helper/SharedState/static-slot regressions: 15 cases,
  including generated JavaScript execution with Node 26.8.1.
- HTTP hot-static acceptance and three-sample changed-response trials.
- Render allocation lint, Python syntax checks, Rust formatting and diff checks.

The whole-workspace test suite was not run. Raw portable measurements live in
`evidence/`; compiler configuration metadata and logs are retained by the probes.
