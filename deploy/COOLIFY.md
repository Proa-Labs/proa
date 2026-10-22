# Archived: Coolify on the former Proa VPS

> **Retired as of 22 October 2026:** all Proa applications, including
> `proa-ui`, use Railway. Coolify is no longer an active deployment dependency.
> The configuration and commands below describe the former VPS installation;
> they are historical reference, not current operating instructions. See
> [the marketing deployment guide](../proa_marketing/README.md#deployment) for
> the current platform and [the cleanup runbook](COOLIFY_DECOMMISSION.md) for
> any remaining legacy resources.

Coolify previously provided the deployment control plane for GitHub-connected
applications and pull-request previews on a shared VPS. The former topology
was:

```text
Cloudflare
    -> HAProxy :80/:443
        -> Coolify dashboard 127.0.0.1:8000
        -> Coolify realtime  127.0.0.1:6001/6002
        -> Traefik router     127.0.0.1:8081
            -> Coolify-managed application containers
```

The dashboard is `https://coolify.proa.cool`. The root account is
`admin@proa.so`; its password is stored in the local macOS Keychain:

```bash
security find-generic-password \
  -a admin@proa.so \
  -s coolify.proa.cool \
  -w
```

Do not put that password, GitHub App keys, or Coolify's `.env` in this
repository.

## Pinned version

The installation is pinned to stable Coolify `4.1.2` with
`LATEST_IMAGE=4.1.2` in `/data/coolify/source/.env`. `AUTOUPDATE=false` is also
set. Do not remove the image pin: Docker's `latest` tag can point at a Coolify
prerelease even when unattended updates are disabled.

## Checked-in configuration

- `deploy/server/coolify-compose.custom.yml` keeps the dashboard and realtime
  ports on loopback and maps `host.docker.internal` to the management bridge.
- `deploy/server/coolify-proxy-compose.yml` is the proxy configuration saved in
  Coolify. Traefik listens only on loopback port 8081; HAProxy remains the TLS
  edge.
- `deploy/server/latency-render-haproxy` renders the exact dashboard routes and
  the final single-label `*.proa.cool` fallback to Traefik.

The live HAProxy registry entry is `/etc/haproxy/previews/coolify.env`. Exact
host rules are emitted before the wildcard router, so existing manually managed
preview hosts continue to win.

## Host access

Coolify connects to its local server as the unprivileged `deploy` user with its
own SSH key. Root SSH remains disabled. The Docker management subnet is allowed
to reach port 22, and Fail2ban ignores `10.200.0.0/16`; otherwise Coolify's own
container can ban itself after a failed connection.

The `deploy` user has execute-only ACL access to `/data/coolify` so Coolify can
enter its proxy/application directories. Do not replace that with a
world-readable directory mode.

## GitHub App and previews

The source record is named `proa-coolify` and belongs to the `Proa-Labs`
organization. Enable **Preview Deployments** during GitHub App registration so
the app receives `pull_request` events and can write deployment comments.

Use one-level preview domains:

```text
pr-{{pr_id}}.proa.cool
```

Enter Coolify-managed application domains as `http://...`. HAProxy supplies the
public HTTPS connection and forwards `X-Forwarded-Proto: https`; configuring a
second TLS redirect in Traefik can create a redirect loop and redundant ACME
issuance.

Cloudflare currently proxies the wildcard successfully to HAProxy. Before
changing the zone to Full (strict), install an origin certificate covering
`proa.cool` and `*.proa.cool` in `/etc/haproxy/certs/`.

## Verification

```bash
ssh deploy@5.78.233.107 \
  'sudo docker inspect --format "{{.State.Health.Status}}" coolify coolify-proxy'

curl -fsS https://coolify.proa.cool/api/health
curl -fsS http://127.0.0.1:8081/ping  # run on the VPS

ssh deploy@5.78.233.107 \
  'sudo ss -lntp | grep -E ":(6001|6002|8000|8081) "'
```

All four management ports must be bound to `127.0.0.1`. HAProxy must remain the
only process bound publicly to ports 80 and 443.

## Recovery

The pre-install rollback bundle is under `/var/backups/proa-pre-coolify-*`.
The clean stable rebuild also has `/var/backups/coolify-pre-stable-reset-*`.
Both directories are root-only.

To restore only the checked-in routing configuration, copy the three files
above to their matching live locations, validate Compose/HAProxy, and reload
HAProxy. Do not restore a database created by a newer Coolify release into an
older image.

Enable two-factor authentication interactively from `/profile`; Coolify must
generate and confirm the TOTP secret so the recovery codes can be stored
directly in the owner's password manager.
