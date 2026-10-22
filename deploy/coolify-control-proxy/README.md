# Retired Coolify control-plane gateway

> **Retired as of 22 October 2026:** all Proa applications, including
> `proa-ui`, use Railway. This gateway is no longer an active deployment
> dependency. The implementation and design below are retained as historical
> reference; do not redeploy the gateway or reconnect the retired preview
> workflow. Follow [the cleanup runbook](../COOLIFY_DECOMMISSION.md) to verify
> removal of any remaining Worker, routes, credentials, and origin resources.

This Cloudflare Worker gave Coolify a browser origin outside the untrusted
`*.proa.cool` cookie boundary. It forwarded `https://coolify.proa.so` to the
legacy Cloudflare origin and added a credential required by HAProxy. HAProxy
redirected unauthenticated requests on `coolify.proa.cool` and stripped the
origin credential before forwarding to Coolify.

The following sections describe the retired Worker's behavior:

- redirects plaintext requests to HTTPS and adds HSTS;
- verifies GitHub App webhook signatures and the configured installation-target
  ID, then acknowledges push and `pull_request` events for the configured
  repository without forwarding them; signed data events for other installed
  repositories continue to Coolify unchanged;
- exchanges an exact-workflow GitHub OIDC identity for a purpose-scoped session
  bound to one application UUID, one PR number, and one exact full candidate
  SHA; and
- exposes `POST /_proa/github-actions/reconcile`, which creates or reuses only
  the session-bound Coolify preview revision; and
- exposes `POST /_proa/github-actions/presentation-token` to a five-minute
  `presentation` session. It mints a Proa Preview GitHub App installation
  token for exactly `Proa-Labs/proa`, with only Deployments write, Issues
  write, and Pull requests write; and
- exposes `POST /_proa/github-actions/archive` to a five-minute `archive`
  session, accepting only an exact deployment UUID plus `promote` or `revoke`.
  The Worker derives application, PR, revision, and workflow-run identity from
  the signed session before sending a separately authenticated command to the
  host-local archive controller.

The trusted `pull_request_target` workflow is the sole preview controller. It
uses one GitHub Actions concurrency group per PR, validates the live PR against
the owner-only allowlist, reconciles the exact revision, and waits for a
terminal result before releasing that group. Cleanup runs under the same
concurrency boundary and waits for confirmed preview absence. External webhook
delivery order, retries, and replays cannot race those operations. A manual
`workflow_dispatch` uses the same path and can create or recover a preview;
title-only and body-only PR edits do not trigger builds.

Deploy sessions last at most 75 minutes and expose the projected reads,
reconcile operation, and revision-guarded DELETE used by a deployment run. A
deploy DELETE is re-attested inside the Worker against the newest projected
history row for the session's PR; a different commit returns HTTP 412 without
reaching Coolify. Teardown sessions last exactly five minutes and expose only
DELETE for their bound application and PR. They intentionally ignore revision
history after the workflow has revalidated that the PR is closed or otherwise
ineligible, because its current head need not be the revision still deployed.
All purposes retain the exact candidate SHA as audit context.

Coolify v4.1.2 includes unbounded build logs in application-deployment history.
The Worker therefore reads each five-row history page under a dedicated 8 MiB
ceiling, immediately projects it to the small identity/status fields Actions
needs, and still caps a scan at 100 rows. Other Coolify responses retain the
tighter 1 MiB ceiling. An oversized history page fails closed with the fixed
`history_payload_too_large` reason; hiding logs upstream remains the durable
way to remove this compatibility allowance.

Presentation sessions also last exactly five minutes and cannot reach any
Coolify API, reconciliation, archive, or teardown path. Their one empty-body
POST asks the Worker to sign a GitHub App JWT, resolve the installation on the
fixed repository, and request a repository-selected installation token. The
Worker verifies the App and installation identity, exact repository list,
permission projection, and roughly one-hour expiry before returning only the
token, expiry, and App bot login. The composite publisher explicitly revokes
that token when it finishes. A hard workflow cancellation can interrupt the
final revocation step, in which case GitHub expires the token normally after
about one hour. The signed presentation session is a five-minute bearer
capability, not a one-use nonce: replay during that window can mint another
token, but every minted token remains fixed to the same repository and narrow
permission set and expires after roughly one hour. Treat the session as a
secret; moving token use into the broker is the path to one-use semantics if
that residual replay window later becomes unacceptable.

An active exact-commit deployment is reusable only for 45 minutes from its
strictly parsed Coolify creation timestamp. Older or malformed active rows are
treated as stuck and reconciliation queues a fresh synthetic deployment. This
prevents a timed-out, subsequently retired UUID from trapping every same-commit
rerun on the same `queued` or `in_progress` row.

Archive sessions also last exactly five minutes. They cannot read or mutate a
Coolify API path and cannot reconcile or delete the mutable PR preview. The
archive endpoint accepts exactly:

```json
{"operation":"promote","deployment_uuid":"dy2vxce7udvt9q2otsiwb6o1"}
```

All authority fields in the host command come from the OIDC-bound session, not
from that body. Before contacting the host, the Worker queries the exact
Coolify deployment detail and requires its UUID to belong to the session-bound
application, PR, and commit. This avoids making an exact transition depend on
unrelated history pages. Promotion additionally requires a recognized
successful Coolify status; revocation can retire the matching failed row. A
missing or mismatched detail returns an error without reaching the host. The
Worker then HMAC-signs the exact command bytes with the dedicated
`ARCHIVE_CONTROL_SECRET` and sends them to the fixed
`/_proa/origin/preview-archive` path. The host controller is loopback-only;
HAProxy requires the independent origin credential before selecting it. The
Worker explicitly returns 404 for every public `/_proa/origin/*` request so its
normal proxy path can never inject that credential on a caller's behalf.

The reconcile client retries only a bounded fixed response proving that
history failed before any mutation was attempted. A lost response, webhook
failure, or other unknown outcome is never retried: the workflow waits through
the queued-job retry grace, then selects the newest exact-SHA row within a
bounded 30-minute observation. The Worker also coalesces duplicate requests in
one isolate. A known mutation returns the exact deployment UUIDs that existed
before the request, so only a causally new row can grant cleanup ownership. If
no row becomes observable, the workflow fails but preserves the route rather
than deleting state while a queued result is unproven. A manual reconciliation
can then converge it. Per-PR Actions concurrency prevents ordinary
cross-isolate calls. Add durable cross-request coordination before exposing
this endpoint to multiple independent controllers.

Coolify does not expose an atomic compare-and-delete operation. Deploy sessions
therefore check history immediately before DELETE. Teardown authority instead
depends on the live-PR revalidation immediately before exchange, its five-minute
lifetime, and the same per-PR concurrency boundary. Independent operators must
not mutate the same preview concurrently; strict multi-writer operation would
require a conditional Coolify API or durable coordination used by every writer.

The current deployment is an owner-only pilot. It applies runtime limits and
keeps application secrets out of preview builds, but still shares Coolify's
host Docker daemon. Marketing builds retain per-preview source and Cargo target
caches on that daemon; cache IDs serialize trusted builds but are not security
capabilities. Move preview builds to a separate host, daemon, network, and
build-cache root with restricted egress before allowing broader collaborators
or fork-authored code.

Each preview FQDN gets a paired source and Cargo target cache. Closed-preview
caches are intentionally left unused for BuildKit GC rather than deleted by a
workflow with daemon-wide prune authority. Keep Docker's builder GC enabled and
verify `docker buildx inspect --bootstrap` reports an `exec.cachemount` age
policy plus a host free-space floor; the current host uses the default 48-hour
unused-cache policy. Cache eviction only makes the next build cold.

The current preview application emits one generated hostname per preview. If it
later serves multiple FQDNs, pass one sanitized cache-scope build argument
instead of using Coolify's potentially multi-value `COOLIFY_FQDN` directly.

Required Worker secrets:

```text
ORIGIN_AUTH
GITHUB_WEBHOOK_SECRET
COOLIFY_MANUAL_WEBHOOK_SECRET
COOLIFY_API_TOKEN
GATEWAY_SIGNING_SECRET
ARCHIVE_CONTROL_SECRET
PROA_PREVIEW_APP_ID
PROA_PREVIEW_APP_SLUG
PROA_PREVIEW_APP_PRIVATE_KEY
```

`COOLIFY_MANUAL_WEBHOOK_SECRET` must exactly match, and be unique to, the
target application's `manual_webhook_secret_github`. The Worker signs Coolify's
application-scoped manual webhook with it; `GITHUB_WEBHOOK_SECRET` is used only
to verify external delivery. `COOLIFY_API_TOKEN` must be root-capable or have
`read:sensitive`, because the Worker verifies the raw application binding and
never returns it to Actions.

`ARCHIVE_CONTROL_SECRET` must be 64 lowercase hexadecimal characters and must
exactly match the host controller's root-only configuration. It authenticates
only the narrow archive transition protocol; do not reuse
`GATEWAY_SIGNING_SECRET`, `ORIGIN_AUTH`, a GitHub credential, or a Coolify token
for it. Host requests use:

```text
X-Proa-Archive-Timestamp: <Unix seconds>
X-Proa-Archive-Signature: sha256=<64 lowercase hex>
```

The signed bytes are exactly
`v1\nPOST\n/_proa/origin/preview-archive\n<timestamp>\n<body>`.

The three `PROA_PREVIEW_APP_*` values belong only in Worker secret storage,
never in repository or environment Actions secrets and variables. The private
key must be unencrypted PKCS#8 PEM (`BEGIN PRIVATE KEY`), not GitHub's usual
PKCS#1 `BEGIN RSA PRIVATE KEY` download. The Worker accepts no caller-selected
owner, repository, installation, or permissions.

The non-secret `GITHUB_APP_ID` binding must match the separate source App
configured in Coolify; it is not `PROA_PREVIEW_APP_ID`. `PREVIEW_DOMAIN` must
remain the isolated `proa.cool` suffix. The non-secret
`ARCHIVE_APPLICATION_NAME` must match the host archive install. A
finished Coolify deployment is reusable only while both its stable PR route
and its exact archive route attest the session-bound revision; a missing or
retired archive therefore causes same-commit reconciliation to build a new
deployment rather than wedging future reruns. The remaining policy bindings
are versioned in `wrangler.toml`.

Never commit their values. Install the versioned `latency-render-haproxy`
helper and copy `origin.env.example` to the origin's preview configuration.
The origin credential file itself remains host-only. Keep
`ARCHIVE_CONTROL_PATH` and `ARCHIVE_CONTROL_PORT`: the renderer validates the
one exact internal path and emits its authenticated loopback backend before the
generic Coolify backend. `ARCHIVE_ROUTER_APPLICATION`,
`ARCHIVE_ROUTER_DOMAIN`, `ARCHIVE_ROUTER_HOST`, `ARCHIVE_ROUTER_PORT`, and
`ARCHIVE_ROUTER_HEALTH_PORT` define a second backend for only
`proa-marketing-[a-z0-9]{12}.proa.cool`. That exact ACL is emitted before the
broad wildcard and sends traffic to the file-provider-only archive router on
the canonical internal IPv4 address in `ARCHIVE_ROUTER_HOST` (currently
`172.31.252.2`) and port `8082`, while checks use its health-only port `8083`.
Missing, non-canonical, or out-of-range IPv4 addresses fail closed before any
HAProxy configuration is rendered. Duplicate archive namespaces or router
ports fail generation. Keep `ROUTER_SUFFIX` and
`ROUTER_PORT` too: they route every remaining dynamic `pr-N.proa.cool`
hostname through HAProxy to Coolify's loopback Traefik listener, so previews do
not need per-PR HAProxy entries or legacy systemd services.

Before the first build, prepare the application directory for both the host
operator and Coolify service group:

```bash
sudo ./prepare-application-directory <application-uuid>
```

The helper is idempotent, rejects symlink and nested-mount traversal, grants
execute-only traversal on the shared applications root, replaces stale ACLs
inside the target subtree, preserves intentional file execute bits, and
installs inherited cross-writer ACLs for future Coolify metadata files.

Validate and deploy:

```bash
node --test worker.test.js
./test-latency-render-haproxy
pnpm dlx wrangler secret put COOLIFY_MANUAL_WEBHOOK_SECRET
pnpm dlx wrangler secret put ARCHIVE_CONTROL_SECRET
pnpm dlx wrangler secret put PROA_PREVIEW_APP_ID
pnpm dlx wrangler secret put PROA_PREVIEW_APP_SLUG
pnpm dlx wrangler secret put PROA_PREVIEW_APP_PRIVATE_KEY
pnpm dlx wrangler deploy
pnpm dlx wrangler secret list
```

`COOLIFY_API_URL` must be exactly `https://coolify.proa.so`.
