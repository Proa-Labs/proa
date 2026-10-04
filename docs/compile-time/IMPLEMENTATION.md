# Development rebuild implementation

The measured outcome, reproduction commands and waterfall are in
[README.md](README.md). This document describes the final implementation;
[REVIEW.md](REVIEW.md) records validation and compatibility changes.

## Cache publication and validated data

`atomic_write_replaceable` validates the plain parent directory and file before
comparing complete on-disk bytes, including the newline. Identical publications
keep their existing inode and require no durability flush. Changed pointers
retain atomic replacement and directory/file flushing. Hard-link tests
separate actual reuse from rewriting; symlink files/parents remain rejected.

The cache lease retains `Arc<Vec<u8>>` snapshots of validated descriptor carriers.
Descriptor and link hash domains are computed from the same freshly read bytes;
manifest assembly and extraction reuse them without rereading or copying.
Paired dependency hashes are reused within the admitted cycle. Parsed records
are cached by exact path/content generation, bounded to 2048 entries, and used
only after both current authority checks succeed. Retaining complete carriers
increases memory; large-app peak RSS and a byte-budgeted cache remain follow-ups.

The CLI uses sha2 0.11 runtime dispatch while preserving digest domains and
formats. Compatibility tests cover SHA-block and I/O boundaries. Independent
cached units validate observed inputs in parallel with deterministic error
ordering. Independent object/index publications complete under the cache lease
before the invocation snapshot commits; changed writes retain durability.

## Complete descriptor carriers

An rlib contains all its codegen objects. For supported native host cc/gcc/clang
links, an adapter captures explicit objects before rustc removes temporary files
and forwards original linker arguments unchanged. It writes an ordinary archive
for the existing transport extractor. It does not scan rustc-private incremental
directories.

The wrapper preserves Cargo's incremental flags and explicit codegen counts.
For incremental descriptor builds without an explicit count, it uses 64 units
instead of rustc's default 256. Final-app partitioning and debug settings retain
the original Cargo profile. Intermediate carriers omit debug information.
Metadata-only checks, unsupported targets/linker protocols,
multiple crate types and explicitly routed outputs use the complete single-object
carrier while retaining incremental queries. Indirect response/file-list inputs
are rejected rather than producing a partial archive; this can restrict large or
custom builds. Sidecar schema 11 and descriptor target policy 7 isolate old caches.

The library/executable acceptance probe covers explicit four-unit and default
64-unit settings over three source
generations. It retains unused witnesses across edits and removes only the deleted
module's witness. Native execution passed locally on macOS and in the initial
Linux PR pipeline shard.

## Final compilation

Final wrapper policy 8 validates full map/plan authority and exposes an exact
mapping-entry hash plus a complete canonical plan projection per unit to Cargo.
Stable runtime coordinates and asset generations replace dependencies on global
traversal order. Macros freshly verify consumed projections against the full plan;
root attestation and launch validation remain global. The complete contract and
actual Cargo/runtime acceptance are in [INVALIDATION.md](INVALIDATION.md).

A bounded process-local cache reuses only pure projection derivations after fresh
full-plan checks. Preparation queries Cargo metadata/tool versions concurrently,
without reusing old answers. Warm wrapper snapshot reuse validates current source
and destination bytes, permissions and the executable contract; misses retain
copy/atomic publication. Same-size corruption remains rejected.

## Watcher and optional static updates

The watcher requires fresh observed bytes to complete a 200ms quiet window;
changed bytes reset it. The pending deadline can shorten the normal poll.
Successful server exit is observed every 5ms while TERM still gets two seconds
before KILL. These CLI defaults apply to existing and newly scaffolded apps.

The [hot-static prototype](HOT_STATIC.md) masks only eligible template string
values in its compatibility fingerprint. Dynamic Rust and markup shape remain
exact. A compatible edit publishes one coherent template snapshot and reloads;
incompatible edits stage a normal linked build before replacing the active
manifest/server. Failed builds retain prior output. Normal macro emission
remains the consolidated literal path. This prototype stays explicitly opt-in.
