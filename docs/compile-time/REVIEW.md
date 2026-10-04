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
  from 4 to 7, and final wrapper policy from 6 to 8. Fresh target namespaces
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
CGUs and the default 64-unit setting, including unused retained witnesses for
libraries and binaries. Response/file-list
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

Retain the PR as a draft pending independent review. Review cache/descriptor changes, then runtime generations
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
compact measurements.

The subsequent Rust CI run caught the matching origin scaffold snapshots still
using the old normalizer. Both template files now match the example after crate
identity substitution; the complete 680-test CLI library recheck passes.

All six CI jobs passed on `390ff41` in
[run 37186827741](https://github.com/Proa-Labs/proa/actions/runs/37186827741).

## Second author review

Rechecked descriptor completeness, fresh cache authority, unit projection
dependencies, runtime identities, and hot-template publication. Two medium
severity defects were reproduced and fixed in the opt-in hot-static path:

| Finding | Fix and regression |
| --- | --- |
| A constant `text!` attribute merged compiler static runs that remained separate in the manifest, producing malformed HTML | Preserve the attribute boundary in hot emission. The failing two-versus-three slot regression now passes for five attribute forms; HTTP checks verify escaping and the complete surrounding markup |
| Emptying or moving text between dynamic children could remove/reposition runs without changing masked token shape, publishing stale or misplaced text | Include ordered static/child/attribute walk events in the compatibility shape. Equal-count movement and deletion regressions require Cargo and retain the active manifest until the build succeeds |

Ordinary macro emission and authored APIs retain their behavior. No additional
defect was found in descriptor/cache or unit invalidation logic. Large-app peak
RSS and unsupported native linker protocols remain the previously recorded
limits; this pass is still an author review.

Frozen-source validation: 681 CLI library tests and 583 core/DSL/macro tests
pass, as do all ten HTTP acceptance cases with the rebuilt release CLI. Both
new regressions failed before their fixes. Strict Clippy for CLI/DSL/macros,
formatting, source checks/lint (existing low-level warnings allowed), render
allocation lint, and acceptance partition validation pass. Pizza performance
was not remeasured; this correction affects only the opt-in prototype.

## Descriptor-size follow-up and combined fixes

Descriptor extraction now overrides only intermediate debug information. The
original final profile still participates in unit matching, and final rustc
arguments retain full debug information. Policy version 6 forces one fresh
namespace rather than admitting old debug-bearing carriers. Incremental library
and native-binary regressions check complete unused witnesses across three
generations while confirming descriptor objects contain no debug sections.

The link-manifest hasher skips an unused input-domain digest on link-only paths,
retaining dependency identities and the existing SHA-256 format. Its production
manifest matches the reference hasher. The proposed source memoization was
discarded after a randomized A/B trial failed to establish an end-to-end gain.

Reviewed the other agent's fixes together with the existing slot safeguards:
hot-static edits run `dev.before` and asset builds before reload; syntax scan
errors reach Cargo/rustc; native carriers replace stale files before exclusive
creation. The exact combined production build passes 683 CLI library tests,
24 wrapper/cache/projection integration tests, strict CLI-library Clippy,
formatting, diff checks and allocation lint over all five changed Rust files.
All 14 live HTTP checks pass, including class-edit asset regeneration, constant
text attributes, moved static runs, syntax-error retention and repair. Asset
validation uses a configured asset command, not actual Tailwind. An earlier
combined build also passed a startup syntax-error check. No new blocker was
found in these fixes; this remains an author review.

Performance follow-ups are separate local diagnostic-fork trials summarized in
[the measurements](README.md); no raw traces or profiler implementation ship.
The subsequent production benchmark includes the hot-static fixes and policy 7.
Generated profiles stay unchanged: ordinary incremental
compilation is already enabled, and the modest build-override gain roughly
doubled first startup. CI must pass on the new head before merging.

## Descriptor partitioning follow-up

Only incremental descriptor invocations without an explicit codegen-unit count
now use 64 instead of rustc's default 256. Explicit counts, nonincremental
defaults, single-object fallback, final profile identity and final compiler
arguments retain their behavior. Policy 7 and the normalized descriptor policy
separate old compiler artifacts; sidecars report the actual descriptor count.
Complete library/native witnesses survive three generations of partial edits
and removals with both explicit and default partition counts.

The frozen change passes 685 CLI library tests and 24 wrapper/cache/projection
tests, strict CLI-library Clippy, formatting, allocation lint and diff checks.
One initial run exported `CARGO_TARGET_DIR` into fixtures, making a deliberate
snapshot-corruption test share its snapshot with another fixture. The final
run selects Cargo output with `--target-dir`, retaining isolated fixture targets.
No production cache check was weakened.

The six-edit production trials have 2.552s template and 2.583s Rust-function
medians. Separate controlled
compiler-proxy trials establish roughly 140–190ms savings from the selected
partition policy. A 32-unit broad cap gains little over repeated 64-unit edits;
retain more incremental granularity. Large-app compile behavior remains
unmeasured, and explicit Cargo counts remain available for tuning. The PR stays
a draft pending independent review and passing CI on the final head.
