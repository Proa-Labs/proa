# Pre-PR review of the rebuild branch

Reviewed `perf/dev-rebuild-pipeline` against `origin/main` (`ab6e5cd`), fetched
again during review. This is an author review, not an independent approval.
The change includes compiler coordination and emitted runtime identities as
well as cache optimizations. It should receive careful review before merging.

## Findings and disposition

| Finding | Disposition |
| --- | --- |
| New `rsjs_unit_projection` integration target missing from the acceptance partition; `--validate` failed, preventing CI from running | Fixed: assigned to main shard 4 and included in PR acceptance |
| Existing PR acceptance covered only linked helpers, leaving the new invalidation proof and much of the modified pipeline out of the PR gate | Expanded PR coverage to descriptor wrapper/cache, final mapping, linked-site runtime, unit projection, pipeline and dev lifecycle targets |
| Unit projections need direct regression coverage for runtime protocol changes beyond template/helper edits | Added validated-plan comparisons for activation, conditional branch, NodeRef alias and lexical provider changes; every consumer invalidates and an unrelated owner stays unchanged |
| Hidden linked-render API and public `DevOptions` source compatibility change | Deliberate, disclosed to the user; user prefers the cleaner long-term design before external adoption. No legacy linked-render compatibility shim added |
| Four stricter Clippy findings in new CLI code | Cleanup before PR; no hash, ownership or protocol behavior change |

The initial concurrent verification run is not passing evidence: source edits
during its fixture compilation correctly triggered `RSJS0001`. The final
integration run uses frozen Rust sources. The new regression test initially
exposed an incomplete ordinal-shifting test helper (its consumer coordinate
was not shifted); that helper is corrected. This was not a production
invalidation failure.

## Compatibility and rollout

Ordinary `html_sync!`, `html!`, component authoring, and the render traits retain
their authored signatures. There are deliberate internal/source changes:

- `rsjs::__private` linked-site records and generated trait methods use `u64`
  occurrence coordinates instead of `u32`, and stable `scope_id` instead of
  global `plan_id`. `RsjsLinkedProviderOccurrenceKey::scope_id()` replaces its
  `plan_id()` accessor. Handwritten users of these hidden APIs must migrate.
- `proa_cli::lifecycle::DevOptions` gains `hot_static`; Rust struct literals
  need `hot_static: false` to retain ordinary behavior.
- Owner/fragment filenames, seed identities and DOM candidate values change.
  Rebuild HTML, generated JavaScript and manifests together. Do not mix CLI,
  macro, runtime or generated artifacts from different revisions.
- Descriptor sidecars advance from schema 10 to 11, descriptor target policy
  from 4 to 5, and final wrapper policy from 6 to 8. Fresh target namespaces
  intentionally prevent reuse of old-policy compiler artifacts; the first
  build after upgrading can be cold. No user source or database migration is
  required for ordinary templates.
- Watch debounce changes from 300ms to 200ms. `--debounce-ms 300` retains the
  previous setting. Changed bytes still reset the full quiet window.
- Native carrier collection rejects response/file-list linker inputs. Large
  or custom cc/clang builds can encounter this newly enforced restriction;
  rejecting the build preserves descriptor completeness. Parsing those
  protocols or selecting a complete fallback needs separate validation.
- `html_hot_sync!` and `proa dev --hot-static` are additive, opt-in prototype
  surfaces. They do not change ordinary macro emission or scaffold defaults.

Package release versions have not been advanced in this investigation branch.
A release must version the affected crates and CLI together and describe these
breaks. New sha2/digest dependencies require Rust 1.85, below the workspace's
existing 1.93 floor; the pinned tested compiler remains 1.95.

## Correctness review

**Cache authority and concurrency.** Identical replaceable records compare all
bytes including the newline after plain-file/parent checks. Changed writes
retain atomic publication and durability. Independent publications complete
under the existing lease before committing an invocation snapshot. Indexed
parallel results preserve canonical error ordering. Artifact coordinates are
unique before publishing indexes. Interrupted generations retain recovery
journals and isolated recovery attempts.

**Retained bytes and parsing.** Current carrier bytes are freshly read and
hashed in both descriptor and link domains. The same owned `Arc<Vec<u8>>`
snapshot supplies manifest and transport extraction. Parsed-record cache hits
follow both authority checks; paired extern hashes come from current validated
inputs, not timestamps. Hash compatibility covers block/I/O boundaries. Retained
whole carriers increase memory relative to streaming validation; large-app peak
RSS remains an unmeasured risk. The parsed-record cache has an entry limit,
not a byte budget.

**Complete descriptor collection.** Rlibs contain all codegen members. The
native adapter captures explicit object arguments before rustc removes them
and forwards the real link unchanged. Partial-edit/removal tests exercise four
CGUs and unused retained witnesses for libraries and binaries. Response/file-list
protocols fail closed. Unsupported target/linker/output forms use the
single-object fallback while retaining incremental queries. Native adapter
execution was validated on macOS and in the initial Linux PR pipeline shard.

**Plan selection and reuse.** The moved parser/validator was compared against
its old macro implementation, rather than treating all 6,000 lines as new logic.
Filesystem/environment authority stays in the macro loader. Typed selections,
all local definitions/occurrences, helper-only units, complete generated owner
JavaScript, fragment generations and protocol relationships contribute to
canonical unit projections. Cargo tracks each unit's mapping/projection hashes;
expanded macros also freshly compare the projection against the full plan.
Cached pure derivations are bounded and keyed by fresh full-plan digest, plan
and unit. Wrong-unit, matching-hash changed-content and corrupted warm
publication cases remain rejected.

**Runtime identity.** Full site hashes and collision-checked compact `u64`
coordinates are shared by SSR and generated JS. JS coordinates are strings,
avoiding numeric precision loss. Owner IDs commit normalized actual emitted
JS and fragment imports. Fragment generations cover export unions, NodeRefs,
compiler/unit generation and potential transitive helper codegen. Root artifact
attestation and launch validation remain global. Tests check actual JS execution,
HTML/manifest agreement, unrelated-crate reuse and shared dependency invalidation.

**Watcher and prototype.** Debounce requires a fresh observed snapshot; TERM
still gets two seconds before KILL. Hot updates mask eligible literals while
retaining dynamic Rust/markup shape, escape through the shared DSL, and publish
one template generation atomically. Fallback builds stage after tasks/assets;
failed builds retain the previous server and manifest. Source discovery is
limited to regular app `src/` files. The opt-in renderer reads JSON and allocates;
new sessions can rebuild cold. These are documented prototype limitations,
not grounds for enabling it by default.

## Validation and merge recommendation

The frozen full CLI/pipeline sweep passed **717 tests**; IR/macro library tests
with `serde_json/preserve_order` passed **312/437**. The four CLI style cleanups
then passed a **695-test** library/cache/pipeline recheck. Workspace all-target
checking, CI's workspace Clippy flags, strict `-D warnings` Clippy for every
changed crate, Rust formatting, render allocation lint, source checks (allowing
existing low-level warnings), shell syntax and CI partition validation passed.
[Exact commands and outcomes](evidence/pre-pr-review-validation.json) preserve
this distinction. Stricter *whole-workspace* Clippy encounters an unchanged
`manual_async_fn` warning in the Axum adapter's tests; no unrelated code was
changed to silence it. Performance was not remeasured after these test/CI/style
review changes; the existing numerical evidence remains tied to `26e6529`.

The existing plain-Cargo link guard means `cargo test --workspace` is not a valid all-app
acceptance command; framework library checks and linked CLI application checks
are reported separately. Proa source checking allows existing raw-HTML warnings
in low-level renderer helpers; the render allocation lint is clean.

Retain the PR as a draft until the complete expanded CI rerun passes. Review cache/descriptor changes, then runtime generations
and projection completeness, then the independent hot-static prototype. The
last category can be split out if it slows review. The measurements establish
approximately 3s typical Pizza rebuilds on one Mac, not a portable latency
promise or a strict bound. No independent reviewer or non-macOS performance
validation is claimed.

## Initial PR CI follow-up

The first Linux run passed the expanded pipeline shard and Pizza acceptance,
plus workspace Rust checks. Atlas acceptance failed because its golden fixture
pinned the old dense runtime IDs. Its normalization now assigns distinct owner
and occurrence labels while preserving repeated references; the fixture changes
only those tokens. The linked Atlas suite passes all six tests locally, including
a regression that checks identity equality/distinctness survives normalization.
Four retained Python probes now have the required license headers. Repository
boundary checks pass with checkout paths represented by placeholders in the
compact measurements. These fixes await the complete CI rerun.
