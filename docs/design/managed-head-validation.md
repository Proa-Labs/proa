# Managed head validation

October 30, 2026. Baseline: main `ee29048e42d032d80f232444c06c2d592cb06e83`.
Implementation branch: `codex/managed-document-head`.

## Hookless layout role parity

Sync and Send-sync dispatch now select the guarded provider lifecycle from
`ROLE`, just like native async and adapted rendering. `LAYOUT` and `PAGE` require
managed mode even with default hooks; layouts are rejected after sealing and in
excluded body regions. The synchronous absence marker remains a compiler
assertion input, and ordinary `COMPONENT` values retain their body-only fast path.
`to_send_static` also rejects a hookless layout rather than erasing its role.

The six-surface regression matrix covers manual, open, sealed and excluded
contexts, plus passing ordinary components and layout-to-Page composition.
Actual capture and out-of-order Suspense fallback tests reject hookless layouts
before body code runs. Compile fixtures include hookless layouts on every surface.

[Storage evidence](evidence/head-role-parity-storage.json) compares identical
native probes against `0243ee9b`: ordinary local/Send components, ErrorBoundary
and Suspense retain the same allocation counts/bytes, future sizes, poll counts
and output bytes. This check does not time managed hookless provider preparation.

## Correctness

- 1,322 tests passed across `proa_core`, `proa_stream`, `proa_framework_core`,
  `proa_framework_axum`, `proa_macros` and `proa_source_tools` (library and
  integration tests, including the final source-lint and cancellation additions).
- 17 core doctests passed with HTML conversion enabled, including synchronous-hook
  and Send-hook compile-fail cases; five pre-existing examples remain ignored.
- Managed-head regressions cover lifecycle/errors, publication, escaping,
  URL policy, typed slots, social fallbacks, templates, defaults, loader sharing,
  native-page transfer, actual Axum local/Send in-order/out-of-order drivers,
  deadlines, HTTP framing/bodyless statuses, client payloads and middleware defaults.
- The second review adds ten regressions: middleware restrictions constrain new
  policies across all five cache-header families, including `Surrogate-Control`;
  inherited-head Pages reject cancelled and unwound preparation in local, macro,
  Send and sync dispatch; and deferred error fallbacks recheck sticky lifecycle
  failures before committing artifacts. Public cache policies, successful Pages,
  valid fallbacks and HEAD suppression remain covered as passing controls.
- Eight authoring regressions distinguish head contribution from the explicit
  Page role. They cover direct async/sync authoring, adapter identity, nested
  Page rejection, fallback validation, two ancestor layouts followed by a Page,
  and rejection of a layout hook below the Page before its code runs. The
  ancestor chain runs through local, Send, sync and adapted rendering. Dropping
  unpolled async Page/Layout boundaries cancels preparation before hook code runs.
  Ordinary body work through `render_child` remains lazy until polled; metadata
  boundaries claim permission eagerly so an unpolled boundary cancels preparation.
- Head providers declare `LAYOUT` or `PAGE`. Native async hooks return
  `Result<(), WriteError>` without user attributes or async blocks; both async
  surfaces compile with warnings denied on Rust 1.93. Role-directed dispatch
  skips ordinary components entirely. The source error `render/head-role-required`
  rejects missing role declarations and explicit `COMPONENT` roles; forwarded
  constants are accepted without evaluation. Sync hooks retain code-generation
  assertions, including when lifted through `to_async` / `to_send`. The compiler
  fixture covers 32 valid/invalid cases across native async, sync and adapted sync
  surfaces, with warnings denied. Source tests cover missing/explicit-invalid
  roles, valid providers, forwarded constants, ordinary components and test scope.
- The corrected PR example calls `shop_head` inside `shop_router`; its exact
  Rust blocks live in `proa_framework_axum/tests/managed_head_example.rs` and
  pass an actual Axum router test for metadata, stylesheet replacement, body
  output and one product lookup per request.
- Dropping an unpolled dispatch outcome for a Page with a real hook is rejected
  like an inherited Page: the hook guard is claimed before the outcome is
  returned, in local, `render_child` and Send dispatch.
- A CSP nonce installed after preparation began fails with
  `HeadError::NonceAfterInitialization` instead of a generic state-replacement
  error.
- A real Page with an immediately ready body publishes Ready on an earlier poll,
  in local/Send and in-order/out-of-order modes, including Pages that inherit
  their head without implementing a hook. Direct `render_child` dispatch yields
  on publication in streaming modes and completes without that yield in buffered
  modes. An outlet inside the Page
  likewise yields before the next body child. Missing and duplicate Ready
  events fail at the transport boundary. A separate worker may resume body
  work after Ready is queued, before the handler returns; the guarantee is
  publication order, not a cross-thread execution barrier.
- The actual out-of-order response test verifies RSJS seed publication precedes
  its base64-encoded boundary swap, after initial shell closure.
- Workspace Clippy passes with all targets and the CI correctness lint policy.
- Rust **1.93.0** checks pass for all targets of core, stream, macros and Axum.
- `python3 scripts/proa_perf_lint.py` passes.
- The preceding review passed the CI-equivalent workspace suites: 2,588 library
  tests and 1,001 integration tests. This layout/Page correction reran the 970
  tests across the five affected crates, workspace Clippy, Rust 1.93 checks,
  core doctests and RSJS conformance.
- The workflow-lint action checks pass locally, including copyright headers and
  actionlint. Both RSJS compile-fail snapshots include the new dispatch-bound
  note; the full conformance suite passes without snapshot-update mode.

The workspace all-target check passes. Strict workspace Clippy is blocked by
existing findings also reproduced on main: sliced strings converted to bytes,
a needless borrow, `first().is_some()`, and `StageTimer` lacking `Default`.
The all-target pass also reaches an existing default-field reassignment in the
source-lint content tests and a manual async loader in the representation tests.
Changed-package verification excludes those six lint categories using command-line
flags; no source suppressions were added.

Changed production Rust adds no Proa source-lint findings at any severity. The
`perf/clone` and `perf/raw-write-html` warnings that `proa lint` still reports in
`proa_framework_core/src/response.rs` all blame to main. Linting the source-linter's
own `src/lint/tests.rs` separately reports its intentionally invalid suppression
fixture; this pre-existing fixture is excluded from the production-path check.
The `rust-ci` action now runs `proa lint` over the production crate sources, the
marketing site, the scaffold templates, and the atlas and pizza examples, so
`render/head-role-required` is enforced on every push rather than only on the
quickstart scaffold.

## Native-path performance

[Raw results](evidence/head-layout-page-performance.json) include every sample, compiler
identity and probe/library hashes. [Reproduction instructions](../../proa_bench/probes/README.md).
These are identical-source optimized probes against both checkouts, not theoretical
claims about what LLVM should eliminate.

The native async-hook change has fresh
[storage-only evidence](evidence/head-native-async-role-storage.json), compared
with main using identical optimized probes. Relative to the
[previous PR implementation](evidence/head-role-assertion-storage.json), ordinary
component and ErrorBoundary outputs, polls, allocations, allocated bytes and
outcome sizes match. Local static Suspense uses 152 allocated bytes rather than
144: an 8-byte increase in its existing boxed future to preserve eager provider
cancellation and lazy ordinary bodies. Its allocation count, outcome size and
poll count match; all other Suspense metrics match. No native path gains an
allocation.
Explicit async Layout/Page boundaries now box their head/body continuation;
managed-boundary latency and allocation totals were not measured by these probes.
The latency table below belongs to the preceding implementation; no new timing
campaign was run for the native async-hook change.

WebContext remains **152 bytes**, SendWebContext **576 bytes**, WriteError **40 bytes**.
Ordinary component outcome sizes, poll counts, allocation counts and output
lengths match the previous PR. Compared with main, ErrorBoundary retains its
prior 8-byte increase (320 versus 312 bytes total), as does local in-order
Suspense (320 versus 312). Send Suspense storage remains unchanged; local static
Suspense uses 152 versus 144 allocated bytes in the latest probe. No native path gains an
allocation. A boxed metadata-boundary continuation keeps ordinary pending
futures inline without recursively enlarging them. The
[earlier explicit-role review](evidence/head-native-lifecycle-review-performance.json)
is retained for comparison.
The Page-only inference experiment is retained as
[historical evidence](evidence/head-page-inference-performance.json); that model
is superseded because layouts also contribute metadata before the Page.

Macro composition uses fewer allocated bytes:

| Case | Main, local / Send | Implementation, local / Send |
| --- | --- | --- |
| One child | 200 / 216 | 120 / 136 |
| Pending child | 584 / 600 | 384 / 400 |
| 32 children | 216 / 728 | 136 / 648 |

Local no-head latency: median of **19 alternating samples**, each containing
3,000,000 warm-buffer renders, with the same counting allocator in both builds.

| Case | Main | Implementation | Change |
| --- | --- | --- | --- |
| Direct leaf | 6.545 ns | 6.513 ns | -0.5% |
| One macro-composed child | 30.396 ns | 23.293 ns | -23.4% |
| Pending child | 47.269 ns | 43.731 ns | -7.5% |
| 32-child loop | 341.302 ns | 173.808 ns | -49.1% |

An earlier implementation had a repeatable small regression. Its
[results are retained](evidence/head-native-before-dispatch-optimization.json).
The fix emits a direct Done/Pending match at each macro child boundary: no-op
components avoid a redundant async adapter and completed children never enter a
`resolve()` future. Actual head hooks still run before their bodies.

The [initial implementation measurements](evidence/head-native-performance.json)
are retained separately; the tables above include inherited-head publication and
cancellation fixes. This does not establish universal zero overhead. Send macro timing, end-to-end
router/middleware throughput, network latency, request-context construction and
managed-head serialization are not measured by these probes. Managed mode pays
for state, validation and publication; metadata-free routes retain their native
mode. Streaming insertion transfers existing buffer pages. Buffered finalizers
copy each retained page directly into their existing contiguous output, without
first rebuilding a second chained buffer.

## Remaining deliberate limits

- Raw `child.render(cx)` calls remain body-only; macros and framework dispatch own hooks.
- One Page and one outlet per managed document; bodies/Suspense/fallbacks cannot change metadata.
- Invalid fields fail atomically; lifecycle failures poison the document and preserve the first cause.
- Runtime branch selection and late mutation remain runtime checks. Async dispatch uses the constant role and cannot detect default-method overrides; source lint is required to catch missing/explicit `COMPONENT` roles. Forwarded constants require author validation. Sync hooks keep their compiler assertion.
- Cancellation discards preparation. Sampled memory limits and cooperative async deadlines are not hard peak-memory or CPU-preemption guarantees.
- Explicit async Layout/Page boundaries use a boxed head/body continuation to own
  their completion guard, including buffered modes and before an outlet exists.
  Ordinary components retain their existing inline future and allocation behavior.
- A wholly synchronous `WebRenderSync` / `html_sync!` render cannot yield at
  publication; it completes its current poll before the transport observes
  Ready. Expensive body work that needs early head delivery belongs in an async
  component after the outlet.
- Existing client JSON is supported; no new browser navigation/head-merging runtime is included.
- Manual HTML remains application-owned. Custom writers must opt into owned-page transfer.
