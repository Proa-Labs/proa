# Proa Marketing

Rust SSR marketing site for Proa.

## Proa UI homepage rail

The homepage includes the complete `home-rail-section` from Proa UI's
`marketing/src/pages/home.rs`: ten inert component previews and the catalog
tile. Its rendered HTML and scoped CSS are checked in, so normal builds do not
need the downstream component library or another checkout. Caption links open
the live catalog at `ui.proa.so`.

To refresh it, run the Proa UI marketing application locally, then run this
from the Proa workspace root with that application's URL:

```bash
node scripts/import-proa-ui-rail.mjs http://127.0.0.1:3333
```

The importer preserves the source rail's order, responsive styling, themes,
and keyboard navigation. It removes hydration metadata from the inert
previews and scopes their styles to this section. Review both themes and the
mobile layout after refreshing the snapshots.

## Docs search

The Cmd+K search runs entirely in the browser. The docs build emits
`/docs/_search/documents.json` (title, description, section, headings,
frontmatter `keywords`, and body text per page); `public/search.mjs` fetches it
on the first open and indexes it with a vendored [Orama](https://orama.com)
bundle. Ranking lives in `public/search-index.mjs`, which the Node battery
imports too, so the score you measure is the score that ships:

```bash
npm run search:battery                  # local server on :5555
node scripts/search_battery.mjs --source prod
node scripts/search_battery.mjs --source path/to/documents.json --verbose
node scripts/search_battery.mjs --config tweak.json   # override DEFAULT_CONFIG keys
```

The fixture is `scripts/search-battery.json` (one obviously-right page per
query, or a short list when two pages tie); its thresholds fail the run when
top-1 or top-3 drops. Give a page identifiers it canonically documents with a
`keywords: [html_sync, html!]` frontmatter list; that field is boosted above
body text. Regenerate the Orama bundle after bumping the `@orama/*`
devDependencies with `npm run search:vendor`.

The rsjs `SearchModal` island only owns the dialog (open, close, focus); see
the ownership note at the top of `public/search.mjs`.

## Content

Marketing docs and blog content are compiled by `proa_docs_build` at build time.
Keep route-owned MDX in these crate-local directories:

- `src/docs` -> `/docs`
- `src/blog` -> `/blog`

Each collection follows the fumadocs-style `meta.json` convention for sidebar
ordering and nested groups.

## Local Docker Build

Build from the repository root:

```bash
docker build \
  -f proa_marketing/Dockerfile \
  -t proa-marketing .
docker run --rm -p 5555:5555 \
  -e HOST=0.0.0.0 \
  -e PORT=5555 \
  -e SITE_URL=http://localhost:5555 \
  -e PROA_BUILD_REVISION=0123456789abcdef0123456789abcdef01234567 \
  proa-marketing
```

Health check:

```bash
curl -fsS http://127.0.0.1:5555/healthz
```

Full public-contract smoke check:

```bash
python3 deploy/marketing_smoke.py \
  --url http://127.0.0.1:5555 \
  --expected-revision 0123456789abcdef0123456789abcdef01234567 \
  --attempts 1
```

Docker reads `.dockerignore` from the build context root. This repository includes
a root `.dockerignore` so local `target/`, `.tools/`, and `node_modules/`
directories are not sent to the Docker daemon.

The Dockerfile builds entirely from this repository. No sibling checkout,
additional BuildKit context, or registry credential is required.

The Docker builder regenerates `proa_marketing/public/styles.css` from
`proa_marketing/styles/input.css` using Tailwind's standalone CLI. No Node or
TypeScript workspace install is required. The build stage downloads the pinned
Tailwind `3.4.18` Linux executable for the target architecture and verifies its
published SHA-256 before execution, so it needs outbound HTTPS access.

The strict linked release build takes about eleven minutes on a cold local
builder and can exceed Free or Trial build limits before its cache is warm. Use
Hobby or higher for reliable source builds, or have CI publish a prebuilt image
and point Railway at that image.

The release executable comes from the linked `cargo proa build` pipeline. Do
not replace it with a plain `cargo build`: only the linked pipeline resolves the
RSJS client-helper edges used by browser hydration.

## Railway

### Google Analytics

The site defaults to the GA4 web stream `G-7WSVNZVD6V`. Set
`GOOGLE_ANALYTICS_MEASUREMENT_ID` in the production service's runtime variables
to override that public measurement ID; an explicitly empty or malformed value
disables analytics. Restart or redeploy after changing it. No Google OAuth
token, service-account key, or Analytics API secret belongs in the site
configuration.

The shared document head uses `proa_third_party::google::GoogleAnalytics` on
marketing, docs, and ecosystem pages. The Google SDK waits until `window.load`
so it does not compete with the page's initial resources. The application
explicitly enables collection with `AnalyticsConsent::Granted`; the shared
crate otherwise requires a consent decision from its caller. The browser bootstrap
only loads Google's script on `proa.so` and `www.proa.so`, outside iframes, so
localhost, Railway previews, and embedded demos do not add production page
views. Google Signals and advertising personalization signals are disabled.
The normal GA4 configuration sends one automatic page view per document load.
See [third-party integration docs](src/docs/framework/third-party.mdx) for the
reusable API, consent handling, and loading tradeoffs.

To verify a deployment, open a production page and check for the
`googletagmanager.com/gtag/js` request and a `google-analytics.com/g/collect`
request with the configured measurement ID. Confirm the visit in the
property's Realtime report. Browser tracking protection may block these
requests.

### Deployment

The checked-in `railway.toml` selects this crate's production Dockerfile while
keeping the repository root as the Docker build context. This is required for
the shared Cargo workspace:

- Config file: `/railway.toml`
- Builder: Dockerfile
- Dockerfile: `/proa_marketing/Dockerfile`
- Health check path: `/healthz`
- Start command: inherited from the Dockerfile `CMD`

Railway injects `PORT`, `RAILWAY_GIT_COMMIT_SHA`,
`RAILWAY_ENVIRONMENT_NAME`, and `RAILWAY_PUBLIC_DOMAIN`. The image provides the
remaining runtime defaults. In production, absolute URLs in `robots.txt` and
the sitemap use `SITE_URL` when explicitly set, then the deployment's
`RAILWAY_PUBLIC_DOMAIN`, and finally `https://proa.so`. Non-production Railway
environments prefer their own injected public domain so inherited production
variables cannot leak the production canonical into a PR sitemap.

Production temporarily sets `SITE_URL=https://proa.so` while the legacy
`preview.proa.so` alias remains Railway's injected primary domain. Standard PR
environments copy base variables, so the resolver deliberately prefers the
environment-specific `RAILWAY_PUBLIC_DOMAIN` outside the production
environment. Once Railway reports `proa.so` as the production public domain,
remove the explicit `SITE_URL` variable.

Do not set `PORT`: Railway allocates and routes to it. To override defaults,
set only the values you need:

```text
HOST=0.0.0.0
RUST_LOG=info
STATIC_DIR=/app/static
SHOWCASE_PUBLIC_DIR=/app/showcase
PROA_CONSOLE_LOAD_TESTS=0
```

Use `SITE_URL=https://<domain>` on platforms that do not provide
`RAILWAY_PUBLIC_DOMAIN`, or for the temporary Railway production override
described above.

`/readyz` exposes Railway's commit SHA as `x-proa-revision` and returns
`Cache-Control: no-store`. `PROA_BUILD_REVISION` remains available as a
platform-neutral explicit override.

Link and deploy from the repository root:

```bash
railway link --project Proa --environment production --service proa
railway up
```

The Railway service is also connected to `Proa-Labs/proa` on `main`. Enable
**Wait for CI** in the service settings so a revision is built only after its
GitHub Actions checks pass.

Enable native previews in **Project Settings → Environments → PR
Environments**. Keep the Railway-provided domain on the production service;
Railway uses it to provision a distinct domain for every temporary PR
environment. Do not enable Focused PR Environments yet: this service depends
on crates and build inputs across the repository, not just files under
`proa_marketing/`.

`PROA_CONSOLE_LOAD_TESTS` defaults to disabled for the public marketing console.
Set it to `1` only for local or internal deployments where `/api/loadtest`
should be available.

The public playground offers Proa and the deployed Hono React companions
(Node and Bun), filtered by each showcase's supported engines. Setting
`PROA_COMPANIONS=0` leaves only Proa in the selector. The standalone playground
retains the other comparison engines for local benchmarking.

The eBay playground lives at `/playground/ebay/ebay-srp` and uses the interactive
renderer. The former `/playground/ebay-rsjs/ebay-srp-rsjs` URL redirects there.

The public Coolify deployment guide under `src/docs/framework/` remains part of
the Proa product documentation. The old repository-specific Coolify marketing
preview workflow has been retired in favor of Railway PR environments.
