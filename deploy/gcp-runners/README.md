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
