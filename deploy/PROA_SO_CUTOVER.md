# `proa.so` Railway cutover

This runbook moves the Proa marketing application from its temporary
`preview.proa.so` hostname to the `proa.so` apex without making the apex depend
on an unverified Railway certificate or an untested origin.

## Current ownership

- Railway project `Proa`, production environment, service `proa` runs the
  marketing application.
- `preview.proa.so` remains attached as the rollback hostname.
- `proa.so` must be attached to the same Railway service before its Cloudflare
  traffic record changes.
- Production sets `SITE_URL=https://proa.so` without triggering a deployment.
  The next production deployment applies that canonical origin.

## 1. Retrieve the current Railway DNS requirements

From a checkout linked to the production service:

```bash
railway domain status proa.so \
  --service proa \
  --environment production \
  --json
```

Copy `verification.dnsHost`, `verification.token`, and the traffic record's
`name` / `requiredValue` from that response. Do not copy values from an old
runbook or screenshot; Railway may replace them when a domain is recreated.

## 2. Validate ownership without moving traffic

In the Cloudflare `proa.so` DNS zone, add the Railway verification TXT record
first. Leave the existing apex traffic record in place. Poll the Railway domain
status until `verification.verified` is true and certificate issuance is no
longer waiting on ownership.

This step is non-disruptive: only the `_railway-verify` TXT record changes.

## 3. Move the apex

Replace the existing apex traffic record with the Railway record reported in
step 1. Cloudflare supports a CNAME at the zone apex through CNAME flattening.
Keep the record proxied if the existing `preview.proa.so` Railway record is
proxied; do not create parallel apex A/AAAA records that can split traffic
between the old and new origins.

Do not remove `preview.proa.so`. It is the rollback path until the apex and
installer checks pass.

## 4. Deploy the canonical-origin change

Deploy the commit that changes Proa's fallback origin to `https://proa.so` and
keeps Railway PR environments on their own `RAILWAY_PUBLIC_DOMAIN`. The
production `SITE_URL=https://proa.so` variable is already prepared, so the
deployment's sitemap and robots output use the apex immediately.

## 5. Verify the public contract

```bash
REVISION=$(git rev-parse origin/main)

python3 deploy/marketing_smoke.py \
  --url https://proa.so \
  --expected-revision "$REVISION"

curl -fsSI https://proa.so/readyz
curl -fsS https://proa.so/robots.txt
curl -fsS https://proa.so/sitemap.xml
curl -fsS https://proa.so/install.sh >/dev/null
curl -fsS https://proa.so/install.ps1 >/dev/null
curl -fsS https://proa.so/.well-known/proa-site.json >/dev/null
curl -fsS https://proa.so/openapi.json >/dev/null
```

Confirm that `robots.txt` and every sitemap `<loc>` use `https://proa.so`, and
that `/readyz` reports the deployed commit in `x-proa-revision`.

## 6. Redirect the temporary hostname

After the apex passes, add a Cloudflare 308 redirect from
`https://preview.proa.so/*` to `https://proa.so/$1` while preserving the query
string. Test a nested path and query before treating the migration as complete.

Keep the Railway `preview.proa.so` custom domain through the rollback window;
the Cloudflare redirect can be disabled immediately if the apex must roll back.
After the window closes, the custom domain and its verification TXT record may
be removed.

## Rollback

Restore the previous Cloudflare apex traffic record, disable the preview-host
redirect, and redeploy with `SITE_URL=https://preview.proa.so` if canonical
metadata must also roll back. Do not delete either Railway custom domain while
rollback is still plausible.
