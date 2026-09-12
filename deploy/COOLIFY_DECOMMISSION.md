# Decommissioning the legacy Proa marketing previews

Proa marketing production now runs on Railway at `https://proa.so`.
Railway PR environments replace the repository-specific Coolify preview
workflow. This runbook tracks the external resources that intentionally cannot
be removed merely by merging repository changes.

The Coolify installation and its Cloudflare gateway are shared with the
separately owned `Proa-Labs/proa-ui` repository. Do not delete the
`proa-coolify` GitHub App, the Coolify server, its HAProxy/Traefik routes, or the
Cloudflare Worker until `proa-ui` has been migrated or confirmed independent.

## Preconditions

Before changing external infrastructure:

1. Confirm the repository no longer contains or runs
   `.github/workflows/marketing-preview.yml`.
2. Confirm a push to `main` deploys successfully to Railway and
   `https://proa.so/readyz` reports the expected commit.
3. Enable Railway PR Environments and verify a test PR receives a distinct
   Railway-provided domain. Leave Focused PR Environments disabled because the
   marketing build consumes workspace-wide inputs.
4. Remove the fixed `SITE_URL` variable from Railway after the deployment that
   understands `RAILWAY_PUBLIC_DOMAIN` is live. Verify both `robots.txt` and
   `sitemap.xml` use the current environment's domain.

## GitHub cleanup

After the preconditions pass:

- Remove repository variables `COOLIFY_API_URL`,
  `COOLIFY_MARKETING_APPLICATION_UUID`, `MARKETING_PREVIEW_DOMAIN`, and
  `MARKETING_PREVIEW_MAINTAINERS` from `Proa-Labs/proa`.
- Remove obsolete repository secrets `PROA_CI_SSH_KEY`, `PROA_DEPLOY_HOST`,
  and `PROA_DEPLOY_KNOWN_HOSTS` after confirming no remaining workflow uses
  them. Keep `CI_RUNNER` and unrelated CI/performance settings.
- Delete the selected-repository `proa-preview` GitHub App installation and
  then the App itself after old deployment comments no longer need updates.
- Change the `proa-coolify` installation from **All repositories** to selected
  repositories and retain `proa-ui` only. Do not delete this App while that
  repository still deploys through Coolify.
- Keep the official Railway GitHub App installed for `proa`.

## Coolify and host cleanup

Delete only the Proa marketing application and its `pr-N.proa.cool` previews
from Coolify. Then stop and disable the Proa preview archive timers, monitor,
controller, and router services installed from `deploy/server/`. Preserve an
offline copy of `/var/lib/proa-preview-archive/hostname-claims/` and
`/var/lib/proa-preview-archive/retired/` for the rollback window; these ledgers
prevent an old immutable hostname from being reassigned to different code.

The `proa.cool` wildcard, `coolify.proa.cool`, Coolify containers, and shared
HAProxy/Traefik configuration remain until `proa-ui` ownership is resolved.
The retired pre-Railway `proa.so` site may be removed only after the Railway
apex has passed the smoke checks in [`PROA_SO_CUTOVER.md`](./PROA_SO_CUTOVER.md)
and the rollback window has closed.

## Cloudflare Worker cleanup

The `proa-coolify-control` Worker currently has two roles: it is the shared
browser/origin proxy used by Coolify, and it contains the retired Proa-specific
preview broker. Keep the proxy role while `proa-ui` depends on it. In a
follow-up change, remove the Proa-specific OIDC, reconcile, presentation, and
archive endpoints and their tests, deploy that reduced Worker, and only then
delete these Worker secrets:

- `ARCHIVE_CONTROL_SECRET`
- `COOLIFY_API_TOKEN`
- `COOLIFY_MANUAL_WEBHOOK_SECRET`
- `GATEWAY_SIGNING_SECRET`
- `PROA_PREVIEW_APP_ID`
- `PROA_PREVIEW_APP_PRIVATE_KEY`
- `PROA_PREVIEW_APP_SLUG`

Retain `ORIGIN_AUTH` and the GitHub webhook secret for as long as the shared
Coolify proxy still uses them. Remove DNS records and Worker routes only after
the remaining Coolify consumer has moved elsewhere.
