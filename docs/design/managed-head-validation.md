# Managed head validation

October 30, 2026. Baseline: main `ee29048e42d032d80f232444c06c2d592cb06e83`.
Implementation branch: `codex/managed-document-head`.

## Correctness

- 963 tests passed across `proa_core`, `proa_stream`, `proa_framework_core`,
  `proa_framework_axum` and `proa_macros` (library and integration tests).
- 16 core doctests passed with HTML conversion enabled, including synchronous-hook
  and Send-hook compile-fail cases; five pre-existing examples remain ignored.
- 93 new managed-head regressions cover lifecycle/errors, publication, escaping,
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
  their head without implementing a hook. Direct `render_child` dispatch yields
  on publication in streaming modes and completes without that yield in buffered
  modes. An outlet inside the Page
  likewise yields before the next body child. Missing and duplicate Ready
  events fail at the transport boundary. A separate worker may resume body
  work after Ready is queued, before the handler returns; the guarantee is
  publication order, not a cross-thread execution barrier.
- The actual out-of-order response test verifies RSJS seed publication precedes
  its base64-encoded boundary swap, after initial shell closure.
- `cargo check --workspace --all-targets` passes.
- Rust **1.93.0** checks pass for all targets of core, stream, macros and Axum.
- `python3 scripts/proa_perf_lint.py` passes.
- The preceding review passed the CI-equivalent workspace suites: 2,588 library
  tests and 1,001 integration tests. The second-review fixes reran the 963 tests
  across the five affected crates, plus workspace Clippy and Rust 1.93 checks.
- The workflow-lint action checks pass locally, including copyright headers and
  actionlint. Both RSJS compile-fail snapshots include the new dispatch-bound
  note; the full conformance suite passes without snapshot-update mode.

The strict Proa source check reports 14 warnings also reproduced against main:
existing response-tail writes/clones and ambiguous HTML/Markdown Suspense coverage.
The changed-file source check passes with `--allow-warnings`; no formatting errors
remain. These baseline warnings are not counted as a clean strict-lint run.

## Native-path performance

[Raw results](evidence/head-native-lifecycle-review-performance.json) include every sample, compiler
identity and probe/library hashes. [Reproduction instructions](../../proa_bench/probes/README.md).
These are identical-source optimized probes against both checkouts, not theoretical
claims about what LLVM should eliminate.

WebContext remains **152 bytes**, SendWebContext **576 bytes**, WriteError **40 bytes**.
Returned outcome sizes, poll counts, allocation counts and output lengths match.
The publication fix adds **8 allocated bytes** to the existing native
ErrorBoundary allocation (320 versus 312 bytes total), and to local in-order
Suspense (320 versus 312). Local static Suspense and Send Suspense storage remain
unchanged. No native path gains an allocation. The extra enum storage keeps an
ordinary pending future inline while boxing the managed Page continuation;
keeping both continuations inline caused recursive future growth and is
deliberately avoided. The cancellation fix retains a completion guard in that
continuation even before publication. Allocation counts, allocated-byte totals,
outcome sizes, polls and output on native paths are unchanged from the
[preceding review](evidence/head-native-review-performance.json).

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
| Direct leaf | 6.695 ns | 6.593 ns | -1.5% |
| One macro-composed child | 30.747 ns | 23.743 ns | -22.8% |
| Pending child | 48.518 ns | 43.297 ns | -10.8% |
| 32-child loop | 346.925 ns | 176.097 ns | -49.2% |

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
- Runtime branch selection and late mutation remain runtime checks. Role and hook capability are static.
- Cancellation discards preparation. Sampled memory limits and cooperative async deadlines are not hard peak-memory or CPU-preemption guarantees.
- Managed async Pages with inherited metadata use a boxed continuation to own
  their completion guard, including buffered modes and before an outlet exists.
  Ordinary components retain their existing inline future and allocation behavior.
- A wholly synchronous `WebRenderSync` / `html_sync!` render cannot yield at
  publication; it completes its current poll before the transport observes
  Ready. Expensive body work that needs early head delivery belongs in an async
  component after the outlet.
- Existing client JSON is supported; no new browser navigation/head-merging runtime is included.
- Manual HTML remains application-owned. Custom writers must opt into owned-page transfer.
