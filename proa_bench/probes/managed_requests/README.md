# Local render and request comparison

This harness links the **actual canonical fixture binaries** from a local
`proa-benchmarks` checkout to two local Proa checkouts using Cargo patches.
It does not edit that checkout or refresh its published scoreboard. The isolated
benchmark copy uses the retained `suite.lock` and `request.lock` resolutions
for both builds, with `--locked`. Optional main uses its recorded
`main-suite.patch` graph adjustment because its Proa manifests predate managed-head
dependencies. Registry versions remain fixed. The suite resolution pins bumpalo to OXC's
exact `3.19.0` requirement.

Prepare a baseline checkout containing the managed-head API, then run from Proa:

```sh
# Baseline before the latest routing/metadata tweaks.
git worktree add --detach /tmp/proa-before-routing 1bde343a
python3 proa_bench/probes/managed_requests/compare.py \
  --baseline /tmp/proa-before-routing \
  --candidate "$PWD" \
  --benchmarks "$HOME/Desktop/proa-benchmarks" \
  --main "$HOME/Desktop/proa" \
  --work-dir /tmp/proa-routing-perf-run \
  --http
```

The output directory must not already exist. The optional `--main` is an extra
renderer control, not the request baseline: older main has no managed-head API.
The optional HTTP run uses Node and the benchmark repository's existing load
driver and byte validator. The scripts retain build logs, lockfile/binary/source
hashes, raw process samples, output hashes, and HTTP CPU/RSS/generator diagnostics.
The detached benchmark copy and build outputs remain in the output directory.
Remove the task-created worktrees with `git worktree remove` when finished.

Compiler jobs are idle during measurement. Keep other compilers and captures idle.
Request measurements alternate 15 process pairs, each running 100,000 warmed
complete `Router::oneshot` requests. Allocation counts include constructing the
request and consuming the full response, with identical instrumentation in all
builds. Bulk metadata is a separate candidate feature; baseline setters and bulk
produce byte-identical output.

Renderer measurements use unmodified `representative_suite_bench` binaries:
nine alternating pairs/triplets, 500 warmups and 3,000 samples per process, for
the card page, 96-product grid and 1,000-row table. Canonical dumps must match
byte for byte. Native component/future probes use the existing `compare_head.py`
protocol. HTTP uses one Tokio worker, three alternating pairs, fixed rates of
2,000/500/50 requests per second, a one-second pre-roll and five-second window.
Every response is validated, and generator-limited phases fail. This checks
latency/CPU at equal load, not maximum capacity or cross-framework rankings.

See the [measured review](../../../docs/design/local-head-performance-review.md)
for results, uncertainty, the initial regressions, and remaining small costs.
