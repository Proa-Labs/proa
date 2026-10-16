# GCP CI runners (ARC on GKE Standard)

Ephemeral, scale-to-zero GitHub Actions runners on spot pods. Scale-set names
are the `runs-on` labels: `CI_RUNNER` selects the trusted-main pool and the
workflow's literal `gcp-pr-ci` label selects the unprivileged pull-request pool.

| Piece | Where |
|---|---|
| Cluster | GKE Standard `proa-ci`, `us-west1-a`, project `airy-cycle-504821-k9` |
| Runner image | `us-west1-docker.pkg.dev/airy-cycle-504821-k9/ci-images/proa-runner` (build: `runner.Dockerfile`, rebuild on toolchain bump; v5 includes GitHub CLI plus ssh, rsync, and zstd for preview/deploy control) |
| Trusted scale set | helm release `gcp` / label `gcp-ci`, values: `runner-scale-set-values.yaml` |
| PR scale set | helm release `gcp-pr` / label `gcp-pr-ci`, values: `pr-runner-scale-set-values.yaml` |
| Controller | helm release `arc` in ns `arc-systems` (gha-runner-scale-set-controller 0.14.2) |
| GitHub auth | k8s secret `proa-github-token` in `arc-runners` (rotate: recreate secret, restart listener) |
| sccache | bucket `gs://proa-ci-sccache` (30-day lifecycle), Workload Identity: KSA `arc-runners/runner` -> GSA `proa-ci-sccache@...` |

The `gcp-ci` runner and its writable cache identity are trusted-main
infrastructure. Pull requests run on the separate `gcp-pr-ci` scale set,
because Rust build scripts execute repository code and must not be able to
poison cache objects later consumed by the `main` deployment job. The PR pool
uses the namespace's unannotated default Kubernetes service account; never bind
that account to the sccache GCP service account.

The trusted scale set registers at **organization scope** in the
`proa-trusted-main` runner group. GitHub restricts that group to repository
`Proa-Labs/proa` and this exact workflow reference:

```text
Proa-Labs/proa/.github/workflows/ci.yml@refs/heads/main
```

Keep `restricted_to_workflows` enabled and public repository access disabled.
Runner labels in a pull request are editable; the GitHub runner-group policy
is what prevents a PR from selecting the cache-writing runner. Do not register
`gcp-ci` at repository scope or move it to the default runner group. The PR
pool stays repository-scoped and has no trusted cache identity. Releases use
the unprivileged PR pool as well. Dispatch trusted CI only from `main`.

Before provisioning, confirm the runner group's repository and workflow
restrictions in organization Settings > Actions > Runner groups. Helm values
select an existing group; they do not create or enforce its access policy.
Changing a scale set's GitHub registration scope requires draining its jobs
and reinstalling that scale set. Leave the other scale sets running.

Rebuild image (`--tag` cannot select a non-root Dockerfile, hence the config):
  cd deploy/gcp-runners && gcloud builds submit --config cloudbuild.yaml --substitutions _TAG=<toolchain>-vN .
Then bump the tag in runner-scale-set-values.yaml and roll the fleet
(release `gcp` per `helm list -n arc-runners`; chart version matches `helm list`):
  helm upgrade gcp oci://ghcr.io/actions/actions-runner-controller-charts/gha-runner-scale-set --version 0.14.2 -n arc-runners -f runner-scale-set-values.yaml

Roll the isolated PR pool with the same chart version:
  helm upgrade --install gcp-pr oci://ghcr.io/actions/actions-runner-controller-charts/gha-runner-scale-set --version 0.14.2 -n arc-runners -f pr-runner-scale-set-values.yaml

The trusted repository runner variable is `CI_RUNNER=gcp-ci`. Keep the PR
label literal in `ci.yml`: this deliberately prevents a missing variable from
falling through to the trusted runner pool.

Any replacement trusted pool must use the same repository and workflow
restrictions before changing `CI_RUNNER`. Private fork PR workflows remain
disabled. If fork contributions are enabled later, require approval and keep
write tokens and secrets unavailable to fork workflows. Runners are single-use
JIT pods, but approved PRs still execute code inside this GCP project.

## Quota notes (learned the hard way, 2026-08-07)

- `pd-balanced` (standard-rwo) counts against `SSD_TOTAL_GB`, same as
  pd-ssd. Every concurrent runner costs ~150GB of that quota: ~100GB
  Autopilot node boot disk + 50GB work volume.
- Granted us-west1 quotas: PREEMPTIBLE_CPUS=96, IN_USE_ADDRESSES=32
  (both auto-approved in minutes). SSD_TOTAL_GB request to 2000 is
  pending review; until granted, effective concurrency is ~1 runner.
- If a runner sits Pending through several autoscaler backoffs, ARC
  marks it Failed after 5 pod starts and stops retrying. Recycle with:
    kubectl delete ephemeralrunner --all -n arc-runners
  (the scale set immediately mints a fresh runner with a valid JIT token)

## 2026-08-08: rebuilt as GKE Standard (Autopilot retired)

Autopilot's fixed 100GB pd-ssd boot disk per node consumed the entire
500GB SSD_TOTAL_GB quota (system nodes included), stalling every
scale-up behind a quota review. Standard mode removes the wall:

- Cluster `proa-ci`, **zonal us-west1-a**, image streaming on, WI pool
  unchanged. Default pool: 1x e2-medium (system pods, pd-standard).
- Runner pool `runners`: n2d-standard-8 **spot**, autoscale 0-8,
  75GB pd-standard boot, **1x local NVMe** via
  `--ephemeral-storage-local-ssd`; labeled+tainted `ci-runner-pool=true`.
- Runner pods: no PVC at all. `_work` is an emptyDir landing on the
  NVMe (80Gi ephemeral-storage request), so storage draws zero
  PD quota and is faster than pd-ssd.
- Quota per concurrent runner is now just 8 spot CPUs (96 granted =
  12 concurrent) and 75GB of the 4TB pd-standard pool.

Expected latency: first job of a session ~2-3 min (spot VM boot);
warm-node jobs start in seconds. To pin latency near zero, set
minRunners: 1 (~$50/mo for one always-on spot runner).

## 2026-08-08 postmortem: why jobs were never delivered

Three stacked faults, in causal order:
1. **Stale runner binary (root cause).** The runner image pinned
   `actions-runner:2.321.0` (2024). It connected and registered but could
   not complete the current broker handshake: REST showed the runner
   `offline`, it received no work, and exited cleanly. Symptom at every
   other layer looked like "runner up, job queued forever". Keep the
   image's base current; a runner more than a few releases behind is
   silently refused work.
2. **ARC 0.9.3 chart** predated the ghalistener protocol; upgrade in
   place poisoned the controller cache (CRD schema change on
   EphemeralRunnerStatus.failures). helm does not upgrade CRDs: a chart
   major bump needs uninstall + CRD delete (strip finalizers) + fresh
   install.
3. **Scale-set name reuse** after uninstall-while-jobs-queued leaves
   orphaned assignments; the fleet label is now `gcp-ci` and CI_RUNNER
   points at it.


## Marketing release images

`ci.yml` builds the marketing release image in parallel with validation, then
passes the immutable image digest to Railway only after the CI gate succeeds.
The `release-image` and `deploy-release` jobs run only on trusted `main` events.
PR jobs cannot access the release builder, its cache, or the Railway credential.

The release entry point is `deploy/marketing/release.py`: CI calls it with
`build` to submit and await Cloud Build, or `deploy` to cut over Railway. The
compilation and packaging commands live directly in `cloudbuild.yaml`.
Experimental profiling tools live separately in `scripts/perf/release_images/`;
the release workflow does not invoke them.

The runner has no Docker daemon. It submits `deploy/marketing/cloudbuild.yaml`
to native x86-64 Cloud Build workers. The build retains the existing linked Proa
pipeline and produces the same binary, static assets, and Node/Bun companions.
`BUILD_ARTIFACTS=prebuilt` skips the Rust Docker stage during packaging. Image
checks exercise HTTP routes, asset delivery, companions, and browser hydration
before the image is published.

Run `bash deploy/marketing/setup-gcp.sh` once with project administrator access.
It creates a private release repository and separates compiler-cache writes,
image writes, and image pulls across scoped identities. The existing trusted
runner identity may submit builds as the release builder and mint short-lived
registry read tokens as the scoped pull identity for validation. Those expiring
tokens must not be persisted as Railway production pull credentials. Configure a
**production-scoped** Railway project token in the repository secret
`RAILWAY_PRODUCTION_TOKEN`.
Private registry credentials require Railway Pro; the workspace was upgraded
with the owner's consent during validation.
The first CI cutover changes production to the image source and removes only
its GitHub build trigger. The deployment must then pass Railway's health check
before the job succeeds. Preview environments retain their source configuration.

**Merge/cutover blocker:** the former Artifact Registry pull credential expires
after one hour. A later restart, host migration, dashboard rollback, or redeploy
can require a fresh pull and fail. Railway documents this limitation for expiring
registry tokens in its [private registry guide](https://docs.railway.com/guides/private-container-registry).
Do not cut production over until it has durable, private-registry read credentials.
This GCP organization blocks service-account keys; moving the runtime image to
private GHCR with a package-read credential is the proposed replacement. The
deployment script checks for registry credentials in Railway before staging a
source. Configure durable credentials directly on the production Railway service
after choosing the replacement registry. The release workflow never writes or
rotates them: Railway masks stored passwords, so the script cannot save the old
password for rollback. This also keeps a failed credential rotation from breaking
pulls of the previous image. Credential rotation is a separate operator action.
The presence check cannot establish whether masked credentials are valid or
durable; do not configure expiring credentials such as `oauth2accesstoken`.
Compiler caches and build reports can remain in private GCS.

Manual `workflow_dispatch` currently repeats the complete native compilation,
image packaging, HTTP and Playwright checks, and normal CI gate before deploying.
It is **not** a credential-only refresh or a fast rollback. Reverting on `main`
and dispatching runs that same full pipeline. There
is currently no supported production image deployment path until durable pull
credentials are configured; dispatch cannot bypass this blocker.

The image packages the full Git commit in `PROA_BUILD_REVISION`; image smoke
checks and the final public `/readyz` probe must return that exact revision in
`x-proa-revision`, in addition to Railway's digest check.

Main CI runs serialize so a newer push cannot cancel a source cutover between
staging and deployment. The current-main check rejects superseded queued runs.
If a staging API call fails or the job receives SIGTERM/SIGINT before attempting
deployment, the script restores the previous source, config-file selection,
healthcheck and restart settings while leaving a source changed by another
operator alone. Registry credentials remain unchanged throughout.

If the deployment request loses its response, the script polls the service
repeatedly for a new deployment with the requested image digest. A match
continues through Git trigger removal, deployment status and public revision
verification without submitting another deployment. If the outcome remains
unknown, or the process receives a signal during submission, the source stays
untouched and the job fails. Inspect Railway's deployments and production Git
triggers before retrying: absence from a timed-out query does not prove the
request was rejected. Never restore the old source while a new deployment might
still be running. Trigger deletion happens only after a deployment ID is known.
SIGKILL, runner loss, and an unavailable Railway API can also prevent cleanup;
inspect the same state before any dashboard deployment or CI retry.

The cache archive includes the content-stable source tree, Cargo registry, CLI,
descriptor artifacts, and final artifacts. It is restored into the same paths
on fresh workers; this preserves Cargo's timestamp and Proa's identity checks.
A missing archive starts a cold build. Never share this bucket or identity with
PR runners. To invalidate incompatible caches, change `_CACHE` in the build
configuration. The cloud build is cancelled when its CI job receives SIGTERM or
polling exits without a terminal result; transient polling errors get three
attempts before cancellation.

To reproduce compilation and runtime measurements:

```sh
gcloud builds submit --config scripts/perf/release_images/benchmark.yaml --async .
```

This runs cold, unchanged, and one-link-change builds for each codegen/LTO
configuration, with the application always at optimization level 3. Reports
include elapsed time, peak RSS and CPU, Cargo timing HTML, fingerprint logs,
binary sizes/checksums, and 1,000 SSR samples per showcase page. The descriptor
pass emits compiler transport at optimization level 0 while preserving the
original profile's assertion configuration; final compilation keeps
the selected application profile. The CLI itself uses `release-fast` because
its own executable is a build tool rather than a published render benchmark.
Reports are stored under the build ID in the private marketing cache bucket.
CPU time and peak RSS describe each timed build stage; descriptor and final
elapsed times come from Cargo's completion lines.
Successful benchmark builds also retain their binaries and packaged assets for
controlled runtime comparisons. Record worker CPU models alongside timings:
native Cloud Build workers of the same machine class can still vary.

Download reports and compare complete results with:

```sh
python3 scripts/perf/release_images/report.py path/to/cgu1-thin path/to/cgu16-thin
```

Compare retained binaries on the same native Linux worker before selecting a
runtime profile. The comparison alternates execution order over three rounds,
with 100 warmups and 1,000 timed SSR samples per page in each round:

```sh
chmod +x artifacts/cgu1-thin/proa_marketing artifacts/cgu16-thin/proa_marketing
python3 scripts/perf/release_images/runtime_compare.py \
  artifacts/cgu1-thin/proa_marketing artifacts/cgu16-thin/proa_marketing \
  artifacts/cgu16-thin runtime-comparison
```

### Native measurements, 2026-10-15

The initial profile sweep used native `E2_HIGHCPU_8` workers, Rust 1.95.0,
application optimization level 3, and descriptor policy 5. Times below include
CSS generation, source reconciliation, CLI compilation, and the linked build;
they exclude worker setup, cache transfer, image packaging, and rollout.

| Codegen units | LTO | Cold | No-op | One href edit |
|---|---|---:|---:|---:|
| 1 | thin | 14m38s | 14.6s | 11m14s |
| 8 | thin | 14m57s | 29.1s | 8m46s |
| 16 | thin | 10m35s | 17.2s | 6m26s |
| 16 | false (local ThinLTO) | 13m00s | 24.2s | 7m15s |
| 16 | off | 7m22s | 9.3s | 3m33s |

Workers varied substantially. This sweep identifies candidates, and does not
establish render-performance equivalence across profiles. Build IDs, in the
table's row order: `346c5683-aa8e-4737-9811-8ae35be865ab`,
`8e00c1e1-a61d-407f-b2c4-b6e9ebf9d05e`,
`59d7e8eb-56c3-4482-aac1-c044c1f2ad96`,
`7e54010e-8179-4d2d-ac6c-e7b942c4fe54`, and
`314232a6-b974-44db-9fad-a086d410b27b`.

The reviewed descriptor policy 6 preserves the original assertion cfg. Its
CGU16/ThinLTO cold build took **9m50s** on an AMD EPYC 7B12 worker. A new worker
restored the full archive and rebuilt one authored href in **5m22s**, reusing
309/310 descriptor artifacts and 306/310 final artifacts; the CLI took 1.3s.
Those four final rebuilds remain the existing global RSJS invalidation behavior.
Cold build: `851e3fda-909f-45d5-9535-17bcc1ff7801`; fresh-worker build:
`f42e19aa-2a3c-4a0b-b308-d429282a6dde`.
The descriptor phase fell from 8m41s to 1m06s; final optimized compilation was
6m32s before and 6m39s after. The earlier whole-build baseline took 17m16s
(`e4c190b5-70fe-403d-adb0-59fc84173243`).

That CGU16 image was deployed by its exact digest to an isolated Railway
environment in **15.4s**. Public HTTP checks covered health, four routes,
16 browser assets, and Proa/Node/Bun rendering. Browser checks exercised homepage
mouse/keyboard tab navigation and ESPN panel hydration. Production switches to
this release path only after the PR is merged and main's validation gate passes.

The final policy-6 runtime comparison put the exact CGU16 binary from build
`851e3fda-909f-45d5-9535-17bcc1ff7801` and a freshly compiled CGU1 binary on
one Intel Xeon 2.20GHz worker. Three rounds alternated execution order, with 1,000 measured samples
per page per binary per round. Median render times across rounds were:

| Workload | CGU1, microseconds | CGU16, microseconds |
|---|---:|---:|
| eBay SRP | 339.027 | 323.402 |
| Nike catalog | 56.105 | 55.183 |
| Nike PDP | 10.392 | 10.314 |
| ESPN gamecast | 23.494 | 22.815 |
| GitHub PR diff | 70.493 | 70.357 |

CGU16 matched or improved median rendering on all five workloads in this
comparison. CI therefore uses **optimization level 3, CGU16, and full ThinLTO**;
the workspace's release defaults for other commands remain unchanged. Separate
workers' SSR deltas did not reproduce in this controlled comparison. Comparison
build: `fe700146-5b34-429e-8084-b116b2c9491e`; raw rounds and binary checksums are
under its `report/control` directory in the private cache bucket.

The five-minute warm compilation target remains unmet: the measured fresh-worker
edit took 5m22s, and overall commit-to-production latency also includes the
existing CI validation gate. Narrowing global RSJS invalidation is separate
work; this change deliberately preserves the existing unit-map/plan witnesses.
