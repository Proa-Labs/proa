# Development rebuild implementation

Branch `perf/dev-rebuild-pipeline`, based on fetched `origin/main` at `ab6e5cd`
(2026-10-03), lives in a separate worktree. The original checkout and its
preexisting edits are preserved. This is an investigation branch with production
cache changes and explicitly scoped prototypes; it does not complete the
owner-local plan redesign or every platform/backend adapter.

## Six investigation outcomes

| Priority | Outcome | Remaining limit |
| --- | --- | --- |
| 1 | Safe identical cache publication; measured actual pizza example waterfall | Assets/browser paint are excluded |
| 2 | Immutable validated byte snapshots, paired input hashes, bounded parsed-record reuse | Every cycle still freshly validates bytes and inputs |
| 3 | Complete multi-object carriers with incremental codegen; partial-edit/removal witnesses retained | Native host cc and rlibs supported; other cases retain conservative single-object fallback |
| 4 | Independent final mapping-entry witnesses; Cargo reuse integration proof | Global plan still invalidates RSJS expansions; owner-local projections need new identities |
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
6.974s. Final branch: standalone 3.999s and pizza 4.854s. The earlier cache/snapshot
checkpoint alone was 3.903s on the standalone consumer. These three-sample
serial trials are separate from the historical isolated prototype.

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

Final wrapper policy 7 validates full map/plan authority but exposes one exact
mapping-entry hash per unit to Cargo. The proc-macro witness tracks that hash,
plus the existing global plan ID and file hash. The integration test changes
another unit's source/map and proves the unchanged mapped dependency remains
fresh. A real split RSJS-island crate still rebuilds in the final pass after an
SSR-page edit because asset/seed identities depend on the global plan. See
INVALIDATION.md for the observed dependency and required owner-local redesign.

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
- CLI library: 678 tests.
- Core/DSL/Proa-macro/RSJS-macro libraries: 268/15/37/463 tests.
- Descriptor wrapper integration: 11 tests; descriptor cache integration: 12.
- Final compilation integration: 4 tests, including mapping reuse.
- Earlier lifecycle dev integration: 1; linked helper acceptance: 4 (including
  generated JavaScript execution and cross-crate imports).
- HTTP hot-static acceptance and three-sample changed-response trials.
- Render allocation lint, Python syntax checks, Rust formatting and diff checks.

The whole-workspace test suite was not run. Raw portable measurements live in
`evidence/`; compiler configuration metadata and logs are retained by the probes.
