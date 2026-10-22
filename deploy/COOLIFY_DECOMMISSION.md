# Cleanup after the Coolify retirement

**Migration complete as of 22 October 2026:** all Proa applications, including
`Proa-Labs/proa-ui`, use Railway. Coolify, its GitHub Apps, the control-plane
gateway, and the VPS preview archive are no longer active deployment
dependencies. There is no remaining `proa-ui` migration prerequisite.

Proa marketing production runs at `https://proa.so`, and native Railway PR
environments replace the old Coolify preview workflow. Current operations are
documented in [the marketing deployment guide](../proa_marketing/README.md#deployment)
and [the release-image runbook](gcp-runners/README.md#marketing-release-images).

This runbook inventories legacy resources to verify and remove if they still
exist. Migration completion does not establish that every old host, route,
credential, or backup has already been deleted. The historical configuration
is in [COOLIFY.md](COOLIFY.md).

## Verify the Railway replacement

1. Confirm the old `.github/workflows/marketing-preview.yml` workflow is absent
   and no active workflow calls the Coolify gateway or archive controller.
2. Verify production health and the expected revision at
   `https://proa.so/readyz`, using the checks in
   [the domain cutover reference](PROA_SO_CUTOVER.md#5-verify-the-public-contract).
3. Verify that Railway PR environments receive distinct public domains and
   that their robots and sitemap output use those domains. Keep Focused PR
   Environments disabled while marketing consumes workspace-wide inputs.

## GitHub cleanup

- Remove obsolete variables `COOLIFY_API_URL`,
  `COOLIFY_MARKETING_APPLICATION_UUID`, `MARKETING_PREVIEW_DOMAIN`, and
  `MARKETING_PREVIEW_MAINTAINERS` from repositories that have migrated.
- Remove obsolete secrets `PROA_CI_SSH_KEY`, `PROA_DEPLOY_HOST`, and
  `PROA_DEPLOY_KNOWN_HOSTS` once no active workflow references them. Keep
  `CI_RUNNER` and current CI/performance settings.
- Remove the retired `proa-preview` and `proa-coolify` GitHub App installations
  and retire those Apps. The former `proa-ui` dependency no longer requires
  retaining the Coolify App.
- Keep the Railway integration and the current release registry credentials.

## Former VPS cleanup

Remove retired Coolify applications and their `pr-N.proa.cool` previews, then
stop and disable the preview archive timers, monitor, controller, and router
services installed from `deploy/server/`. Retain required backups and the
hostname ownership ledgers under the applicable retention policy; do not
reuse an old immutable preview hostname for different code.

Retire the Coolify dashboard, realtime and proxy containers, their dedicated
HAProxy/Traefik entries, and their obsolete public hostnames. Preserve any
unrelated services on the VPS. Remove old `proa.so` origin routes only after
the Railway apex checks pass. These resources no longer need to remain for
`proa-ui`.

Confirm that retired PHP and Node services are stopped or removed; switching
application traffic to Railway does not itself stop an abandoned container.

## Cloudflare gateway cleanup

Retire the entire `proa-coolify-control` Worker and its routes. Both its
browser/origin proxy and its OIDC, reconcile, presentation, and archive broker
roles are obsolete. No reduced proxy needs to remain for `proa-ui`.

Revoke or delete the retired Worker's secrets and associated credentials:

- `ARCHIVE_CONTROL_SECRET` and `COOLIFY_API_TOKEN`
- `COOLIFY_MANUAL_WEBHOOK_SECRET` and `GATEWAY_SIGNING_SECRET`
- `PROA_PREVIEW_APP_ID`, `PROA_PREVIEW_APP_PRIVATE_KEY`, and `PROA_PREVIEW_APP_SLUG`
- `ORIGIN_AUTH` and the old GitHub webhook secret

Remove obsolete Coolify DNS records and wildcard routes without changing the
current Railway production or preview domains. Verify cleanup separately from
the application migration; repository documentation does not execute it.
