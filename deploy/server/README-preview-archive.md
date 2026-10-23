# Retired immutable preview archives

> **Retired as of 22 October 2026:** Railway PR environments replaced this
> Coolify archive pipeline. All Proa applications, including `proa-ui`, use
> Railway. The design and host commands below are historical reference; do not
> provision or restart these services for current deployments. Verify cleanup
> with [the decommissioning runbook](../COOLIFY_DECOMMISSION.md).

Coolify's native preview owns the mutable `pr-N.proa.cool` route. A Coolify
`{{random}}` preview template does not create a retained deployment: Coolify
regenerates it during each deployment, updates the one preview row, and
replaces the fixed PR container.

`proa-preview-archive-sync` adds a bounded deployment-permalink layer without
modifying Coolify. Every 30 seconds it:

1. discovers healthy native containers named `<application-uuid>-pr-N`;
2. verifies Coolify ownership, the exact revision, exact image ID, and the
   deployment UUID in Coolify's compose artifact label;
3. starts one provisional archive container from that exact image ID on the
   dedicated internal `proa-preview-archive` network; and
4. atomically publishes a root-generated Traefik file-provider route for
   `proa-marketing-<deployment-prefix>.proa.cool`.

The retained container is forced to the marketing image's unprivileged UID,
has a read-only root filesystem, a small writable `/tmp`, no Linux
capabilities, `no-new-privileges`, and the same CPU/memory
limits as the native preview. Docker's local log driver is capped at three
10 MB files per retained container. Only an explicit application environment
allowlist crosses the boundary. The marketing image itself must declare
exactly `PATH`, `HOST`, `PORT`, `RUST_LOG`, `STATIC_DIR`,
`SHOWCASE_PUBLIC_DIR`, and `PROA_CONSOLE_LOAD_TESTS`; the final archive may add
only `SITE_URL`, `SOURCE_COMMIT`, and `PROA_BUILD_REVISION`. Duplicate,
malformed, unknown, and secret-bearing entries reject the image or container.
In particular, Coolify variables and unknown or secret-looking variables are
not copied. Archive containers use only
`proa.preview.archive.*` ownership labels; they intentionally omit Coolify
application and PR labels so they cannot perturb Coolify status or cleanup.
Before creating another container, the reconciler requires every physically
present archive-labeled container to have passed validation and requires the
existing population to be within both caps. One serialized candidate may then
use one temporary admission slot while it is attested: at most 4 containers
for its PR and 31 globally with the default limits. Only after it is healthy is
one incumbent evicted, returning the population to 3 per PR and 30 globally.
A failed candidate is removed without evicting an incumbent.

The archive router is a separate, pinned Traefik process. It mounts the exact
installer-owned static configuration and
`/var/lib/proa-preview-archive/routes` read-only. The launcher validates the
static file's path, ownership, mode, and checked-in SHA-256 digest before
starting Traefik. That configuration enables only Traefik's file provider: it
has no Docker socket, dashboard, API, or Coolify dynamic configuration.
HAProxy terminates wildcard TLS and sends only the exact
`proa-marketing-[a-z0-9]{12}.proa.cool` shape to `172.31.252.2:8082` on the
dedicated internal bridge before the broad Coolify wildcard route. A separate
entrypoint on `172.31.252.2:8083` serves router health and is never public.
The bridge subnet is pinned to `172.31.252.0/24`; the launcher requests and
attests the router's exact `.2` address and publishes no host ports.

Route publication uses a same-directory temporary file, file `fsync`, mode
`0644`, atomic rename, and parent-directory `fsync`. Removal also fsyncs the
parent directory. The directory is root-owned `0755`: the reconciler is its
only writer, while the nonroot router can read the bind-mounted definitions.
Each route is canonical JSON serialized into a `.yml` file: JSON remains valid
YAML, and the supported suffix is required for Traefik's file provider to
discover the definition. Legacy `.json` route artifacts are never accepted and
are removed during reconciliation.
CI pins this contract with `deploy/check_preview_archive_traefik_discovery.py`,
which runs a checksum-pinned native Traefik 3.6.25 binary, loads a canonical
route through the real file provider, and probes both the private ping
entrypoint and a Host-routed in-process backend. Local runs use the same native
path when `--traefik-binary` is supplied, otherwise they retain the pinned
Docker integration when Docker is available and cleanly skip when it is not.
Routes are derived again from the set of fully validated archive containers on
every tick. Desired routes are atomically published first and only then are
stale definitions durably removed, so a candidate or route-write failure does
not take an unrelated valid permalink offline. Reconciliation re-attests the
labeled internal archive bridge and every member and alias both before work
and immediately before publication. Only the exact router identity and exact
archive identities may join; a foreign member or backend-shaped alias clears
all routes and fails closed. Every native source must remain on the separate
`proa-preview` network.

## Publication and retention

Discovery does not make an archive durable. A new archive is provisional for
15 minutes, which gives the trusted preview workflow enough time to smoke-test
both the mutable and exact routes. Only that workflow can promote the exact
deployment after all publication checks succeed. It uses a five-minute,
OIDC-bound archive session whose application, PR, commit, workflow run, and
attempt are supplied by the signed session rather than caller input.

Before forwarding a promotion or revocation, the gateway verifies that the
deployment UUID belongs to the session-bound application, PR, and commit in
Coolify's history. For promotion, or revocation after a snapshot exists, the
host then independently verifies the archive's container, image, labels,
metadata, revision, and health. A rejection can arrive before the snapshotter;
in that one case the host relies on the signed, history-attested identity to
reserve the shortened hostname and tombstone the UUID before any container is
routed. Commands are sent to a loopback-only controller over one fixed HAProxy
path and authenticated with a dedicated HMAC secret.

A failed or rejected publication is explicitly revoked when possible. Any
unpromoted archive is retired automatically when its provisional window
expires. Retired deployment UUIDs and shortened hostnames are tombstoned and
can never be recreated or reassigned.

Promoted archives use these bounds:

- 7-day maximum age
- newest 3 deployments per PR
- newest 30 deployments globally for this application

Closing or merging a PR removes Coolify's stable `pr-N` preview but does not
remove promoted exact deployment links early. With no native source container,
the sync simply stops producing new snapshots; the normal TTL and caps collect
the existing ones. Running archive containers pin their exact image IDs until
GC. After an archive expires its URL may return 404, but that hostname can
never be reassigned to a different deployment.

## Install

Create a dedicated 32-byte secret before running the installer. The same exact
64-character lowercase hexadecimal value must be installed as the Cloudflare
Worker secret `ARCHIVE_CONTROL_SECRET`; do not reuse any GitHub, Coolify,
gateway-session, webhook, or origin credential.

On the Coolify host, create the file at its final mode instead of writing a
temporarily readable destination:

```bash
sudo install -d -m 755 /etc/proa
secret_tmp=$(mktemp)
trap 'rm -f "$secret_tmp"' EXIT
chmod 600 "$secret_tmp"
openssl rand -hex 32 > "$secret_tmp"
sudo install -m 600 -o root -g root \
  "$secret_tmp" /etc/proa/preview-archive-control.secret
rm -f "$secret_tmp"
trap - EXIT
```

Transfer that value to the Worker's secret store through a protected channel,
then erase any temporary copy. From this directory on the Coolify host:

```bash
sudo ./install-proa-preview-archive \
  --application-uuid irx97itir84sonabt5d0g5c5 \
  --application-name proa-marketing \
  --preview-domain proa.cool \
  --native-docker-network proa-preview \
  --archive-docker-network proa-preview-archive \
  --defer-sync
```

The installer writes non-secret configuration to
`/etc/proa/preview-archive.env`, validates the root-owned `0600` control secret,
pins the image ID of the running Coolify Traefik image for the isolated router,
creates or verifies the labeled internal archive network, and verifies both
the controller and router health checks. File, unit, configuration, network,
and prior service-state changes are transactional until both services are
healthy. A failed rollback retains its recovery directory and reports its
path. The transaction never restores or overwrites deployment metadata,
hostname claims, retired tombstones, approvals, or generated route state.
`--defer-sync` deliberately leaves the snapshot timer disabled while the
matching Worker secret/code and HAProxy route are installed and tested. After
an authenticated end-to-end probe succeeds, enable reconciliation explicitly:

```bash
sudo systemctl start proa-preview-archive-sync.service
sudo systemctl enable --now \
  proa-preview-archive-sync.timer \
  proa-preview-archive-monitor.timer
```

Omitting `--defer-sync` runs one successful reconciliation and then enables
both timers. It is intended only for already-coordinated upgrades. Inspect
structured journal events with:

```bash
journalctl -u proa-preview-archive-control.service --no-pager
journalctl -u proa-preview-archive-router.service --no-pager
journalctl -u proa-preview-archive-sync.service --no-pager
journalctl -u proa-preview-archive-monitor.service --no-pager
```

The reconciler persists a candidate's first provisional epoch before its first
health attempt. Failed retries therefore age out from the original attempt,
and expired provisional metadata is rejected before a container can be
recreated. Malformed exact-owned archives are durably de-routed and removed,
with fixed identity evidence in the journal. An ambiguous or foreign identity
is never deleted; it fails closed and blocks physical admission instead.

## Failure and expiry monitoring

A successful reconciliation atomically records a root-only heartbeat at
`/var/lib/proa-preview-archive/last-success`. The monitor checks that heartbeat,
the controller, and the router's private ping endpoint every minute. If at
least one exact route exists, it structurally validates every root-owned route
definition, loads each matching deployment metadata record, and requests
`/readyz` through the real `8082` router path with the exact Host header and
expected revision. More than 30 route files is itself invalid. It emits a
`PRIORITY=1` journal event when the last successful reconciliation is more than
20 minutes old, when state is malformed, or when either service or any exact
route is unhealthy. The threshold is intentionally longer than the sync
service's 15-minute timeout; a failed service also triggers an immediate
systemd `OnFailure` alert rather than waiting for heartbeat expiry.

These are host-local alerts. Production installation must forward priority-one
journal events to the operator's pager or log alerting service. Verify the
entire path after installation:

```bash
systemctl status proa-preview-archive-sync.timer
systemctl status proa-preview-archive-monitor.timer
journalctl -p alert \
  -u 'proa-preview-archive-alert@*.service' \
  -u proa-preview-archive-monitor.service \
  --since today --no-pager
```

If reconciliation is intentionally disabled for maintenance, disable the
monitor timer in the same change and re-enable it only after one successful
manual sync has refreshed the heartbeat. Lock contention exits nonzero, so an
overlapping run cannot execute the service's success-heartbeat hook. Preserve
`hostname-claims/` and `retired/` when an exact-owned invalid container is
removed. A foreign or ambiguous identity is retained for investigation but
blocks admission and emits a fixed-field error; never use broad Docker pruning
as archive remediation.

## GitHub Actions URL derivation

Actions and the host use the same helper, avoiding hostname drift:

```bash
deploy/server/proa-preview-archive-url \
  --deployment-uuid "$DEPLOYMENT_UUID" \
  --domain proa.cool \
  --format github-output
```

With `GITHUB_OUTPUT` set, this appends both `url=` and `hostname=`. The first
12 characters of the validated 24-character Coolify CUID form the public
deployment key.
