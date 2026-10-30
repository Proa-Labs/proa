# Managed head validation

October 30, 2026. Baseline: main `ee29048e42d032d80f232444c06c2d592cb06e83`.
Implementation branch: `codex/managed-document-head`.

## Correctness

- 967 tests passed across `proa_core`, `proa_stream`, `proa_framework_core`,
  `proa_framework_axum` and `proa_macros` (library and integration tests).
- 18 core doctests passed with HTML conversion enabled, including synchronous-hook
  and Send-hook compile-fail cases; five pre-existing examples remain ignored.
- 97 new managed-head regressions cover lifecycle/errors, publication, escaping,
  URL policy, typed slots, social fallbacks, templates, defaults, loader sharing,
  native-page transfer, actual Axum local/Send in-order/out-of-order drivers,
  deadlines, HTTP framing/bodyless statuses, client payloads and middleware defaults.
- The second review adds ten regressions: middleware restrictions constrain new
  policies across all five cache-header families, including `Surrogate-Control`;
  inherited-head Pages reject cancelled and unwound preparation in local, macro,
  Send and sync dispatch; and deferred error fallbacks recheck sticky lifecycle
  failures before committing artifacts. Public cache policies, successful Pages,
  valid fallbacks and HEAD suppression remain covered as passing controls.
- A real Page with an immediately ready body publishes Ready on an earlier poll,
  in local/Send and in-order/out-of-order modes, including Pages that inherit
  their head through an empty hook. Direct `render_child` dispatch yields
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
- Page inference adds four regressions covering all render surfaces and sync
  adapters, capability detection without hook execution, nested Page rejection,
  and Page fallback rejection before the protected child runs. Async and sync
  authoring examples also compile as doctests with warnings denied; only the
  async method's intentional `refining_impl_trait` lint is allowed.
- The preceding review passed the CI-equivalent workspace suites: 2,588 library
  tests and 1,001 integration tests. This revision reran the 967 tests across the
  five affected crates, workspace Clippy, Rust 1.93 checks and RSJS conformance.
- The workflow-lint action checks pass locally, including copyright headers and
  actionlint. Both RSJS compile-fail snapshots include the new dispatch-bound
  note; the full conformance suite passes without snapshot-update mode.

The strict Proa source check reports 14 warnings also reproduced against main:
existing response-tail writes/clones and ambiguous HTML/Markdown Suspense coverage.
The changed-file source check passes with `--allow-warnings`; no formatting errors
remain. These baseline warnings are not counted as a clean strict-lint run.

## Native-path performance

[Raw results](evidence/head-page-inference-performance.json) include every sample, compiler
identity and probe/library hashes. [Reproduction instructions](../../proa_bench/probes/README.md).
These are identical-source optimized probes against both checkouts, not theoretical
claims about what LLVM should eliminate.

WebContext remains **152 bytes**, SendWebContext **576 bytes**, WriteError **40 bytes**.
Returned outcome sizes, poll counts, allocation counts and output lengths match.
Native ErrorBoundary and local in-order Suspense allocations now match main at
**312 allocated bytes**; local static Suspense remains 144 bytes and Send Suspense
adds no allocation. Removing the separate Page role removes the ordinary future's
Page branch and its enum storage. This recovers the eight-byte increase measured
in the [preceding review](evidence/head-native-lifecycle-review-performance.json).
Ordinary components return their own render outcomes directly. Page hooks use a
separate strategy that owns lifecycle guards without adding a dispatch box.

Macro composition uses fewer allocated bytes:

| Case | Main, local / Send | Implementation, local / Send |
| --- | --- | --- |
| One child | 200 / 216 | 120 / 136 |
| Pending child | 584 / 600 | 376 / 392 |
| 32 children | 216 / 728 | 136 / 648 |

Local no-head latency: median of **19 alternating samples**, each containing
3,000,000 warm-buffer renders, with the same counting allocator in both builds.

| Case | Main | Implementation | Change |
| --- | --- | --- | --- |
| Direct leaf | 6.513 ns | 6.431 ns | -1.3% |
| One macro-composed child | 30.288 ns | 22.297 ns | -26.4% |
| Pending child | 46.805 ns | 45.141 ns | -3.6% |
| 32-child loop | 338.835 ns | 139.107 ns | -58.9% |

An earlier implementation had a repeatable small regression. Its
[results are retained](evidence/head-native-before-dispatch-optimization.json).
The fix emits a direct Done/Pending match at each macro child boundary: no-op
components avoid a redundant async adapter and completed children never enter a
`resolve()` future. Actual head hooks still run before their bodies.

The [initial implementation measurements](evidence/head-native-performance.json)
are retained separately; the tables above include inferred Page identity,
publication and cancellation handling. This does not establish universal zero overhead. Send macro timing, end-to-end
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
- Runtime branch selection and late mutation remain runtime checks. Page identity is inferred statically from hook capability.
- Cancellation discards preparation. Sampled memory limits and cooperative async deadlines are not hard peak-memory or CPU-preemption guarantees.
- Pages that only inherit defaults still provide an empty successful hook;
  ordinary components and layouts omit the hook.
- The `async fn` authoring form needs a scoped `#[allow(refining_impl_trait)]`
  because the trait also accepts the non-Future `NoHead` marker. Synchronous
  hooks use `impl SyncHeadOperation` with direct mutations.
- A wholly synchronous `WebRenderSync` / `html_sync!` render cannot yield at
  publication; it completes its current poll before the transport observes
  Ready. Expensive body work that needs early head delivery belongs in an async
  component after the outlet.
- Existing client JSON is supported; no new browser navigation/head-merging runtime is included.
- Manual HTML remains application-owned. Custom writers must opt into owned-page transfer.
