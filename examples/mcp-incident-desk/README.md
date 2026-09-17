# Incident Desk

Incident Desk is a production-shaped Proa example that serves the same authorized incident data to people and agents. The browser gets server-rendered HTML. MCP clients get a concise Markdown brief or a typed JSON projection. Report sections, SQLx policy, the request-scoped loader, and the resolve command are shared.

The app is fictional and Sentry-inspired. It is not affiliated with Sentry and contains no copied product UI or customer data.

## Run it

From this directory:

```console
./scripts/setup.sh
bash scripts/dev.sh
```

The setup script creates a short-lived local CA, a localhost certificate, ignored `.local/` key material, and `.env`. Review the printed `security add-trusted-cert` command before running it if you want Chrome to trust the local HTTPS certificate. The app listens on `https://localhost:5570`; the demo OAuth issuer listens on `https://127.0.0.1:5571`.

`scripts/dev.sh` builds assets and watches for source and stylesheet changes through
`proa dev`. It uses the installed CLI, or `cargo proa` inside this checkout,
and keeps one watcher per example. Refresh the browser after changes; this
custom HTTPS shell does not embed the browser reload client. Use
`bash scripts/build.sh` for a release build. Both scripts forward additional
CLI arguments. Run the TLS setup once before starting development.

The first application start prints one password for each seeded demo account. Save those values locally. To rotate Alice's password later, stop the app and run `cargo run --bin mcp-incident-desk -- reset-password alice`. The accounts are Alice (Aster maintainer), Ben (Aster viewer), and Casey (Harbor maintainer). The database is `sqlite://data/mcp-incident-desk.db`; startup runs the committed SQLx migrations and seeds only when the seed marker is absent.

Use the generated Proa lifecycle with `cargo proa site check`, `cargo proa check --all`, `cargo proa db status`, `cargo proa db migrate`, `cargo proa dev`, `cargo proa build`, `cargo proa start`, and `cargo proa test`. `proa db rollback` is for a deliberate development rollback. `proa db reset` is destructive and should only be used when you intend to delete this local demo database and reseed it.

## Try the browser

Open `https://localhost:5570`, sign in, and open `AST-1042`. The issue page has the same overview, impact, release evidence, runbook, activity, and permission-aware action summary used by MCP. The `.md` and `.json` suffixes are authenticated alternate representations; `Accept: text/markdown` negotiates Markdown on the canonical issue URL. `/connections` shows grants without exposing credential values. `/lab` compares HTML, Markdown, and JSON for one loaded revision and reports keyed loader reads.

## Try MCP

The resource endpoint is `https://localhost:5570/mcp`. The reference client performs protected-resource discovery, dynamic client registration when needed, PKCE S256 authorization, browser callback validation, token exchange, refresh rotation, and bearer attachment. Credentials remain in memory for the process:

```console
cargo run --bin mcp_demo_client
cargo run --bin mcp_demo_client -- --write
cargo run --bin mcp_demo_client -- --stdio
cargo run --bin mcp_demo_client -- --stdio --write
```

The stdio mode is a local OAuth bridge for desktop connectors. For Claude Desktop, configure that command as a local stdio server with this example as its working directory. For Codex Desktop, add the Streamable HTTP endpoint with OAuth client registration set to DCR, or use the stdio bridge when the desktop build cannot reach a loopback resource. Browser authorization is real: PKCE, signed RS256 tokens, hashed server-side credentials, current membership checks, and revocation are enforced. Desktop clients must trust the CA created by `setup.sh`; do not disable TLS verification.

The six tools are `whoami`, `search_issues`, `get_issue_brief`, `get_issue_data`, `create_issue`, and `resolve_issue`. The first four require `issues:read`; creating and resolving require `issues:write` and a current maintainer role. The brief contains content text only. JSON includes a structured projection and a short fallback for clients that do not expose structured content.

To create an incident, use project, release, and runbook IDs returned by `get_issue_data`, then call `create_issue` with a title, severity (`low`, `medium`, `high`, or `critical`), affected-user/event counts, optional event samples, an audit note, and an idempotency key. The server generates the incident ID and takes workspace and owner from the authenticated session. Creation, samples, audit attribution, and retry protection commit in one transaction; retries with the same key and input do not duplicate incidents. Newly created incidents immediately support the existing HTML, Markdown, JSON, search, and resolution paths.

For Codex, register the server with `codex mcp add incident-desk --url https://localhost:5570/mcp --oauth-client-registration dcr`. To start a fresh authorization, run `codex mcp login incident-desk --oauth-client-registration dcr --scopes issues:read,issues:write`. Use the newly opened browser tab and complete sign-in and consent within five minutes. An old authorization link can still display a login form after the desktop client's callback listener has expired; restart from Codex instead of reopening that link. Keep each attempt in one browser and use the latest authorization tab.

Native browser forms require the `same-origin` referrer policy used here: `no-referrer` causes form POSTs to send `Origin: null`, which the server correctly rejects. The HTTPS integration tests check this response header in addition to exercising Origin and CSRF validation.

## What it demonstrates

- SQLite through SQLx 0.8, generated migrations, WAL, foreign-key enforcement, bounded cleanup, and transactional seed/mutation paths.
- OAuth/OIDC-shaped authorization code flow with S256 PKCE, OIDC nonce/state checks for the website, RSA-3072 RS256 signing, Argon2id passwords, opaque hashed sessions/codes/refresh tokens, rotation, replay revocation, and separate browser/MCP grants.
- Tenant-qualified queries and reauthorization at request and write boundaries. A viewer can read Aster issues but cannot resolve them; Harbor data never appears in an Aster projection.
- A request-local `DataResolver` that deduplicates keyed project loads before synchronous HTML or Markdown rendering. The async Markdown test lifts a sync report with `to_md_async` without carrying `WebContext` across an await in an MCP handler.
- `IssueReport` and section components implementing both `WebRenderSync` and `MdRenderSync`, with typed escaping and no render-path allocation findings from the Proa performance lint.

Tests cover browser-like HTTPS OAuth, MCP SDK tool calls, PKCE and JWT negatives, refresh replay, CSRF and Origin checks, tenant isolation, escaping, loader deduplication, concurrent resolve conflicts, representation negotiation, and persistence after reopening the database. Native Codex OAuth, Markdown/JSON tool reads, and a real issue resolution have also passed, with the result confirmed in the browser. See [`VERIFICATION.md`](VERIFICATION.md) for exact evidence and the remaining desktop-conversation checks.

See [`DESIGN.md`](DESIGN.md) for the full acceptance contract and security decisions.

## Presentation and Proa UI

The interface uses source-installed `@proa/button`, `badge`, `card`, `table`,
`input`, and `textarea` from [ui.proa.so](https://ui.proa.so), with their
foundation and spinner dependencies pinned in `proa.lock.json`. The bundled
compatibility stylesheet is imported by `styles/input.css`; the app theme and
layout live in `public/incident.css`. These are server-rendered components and
do not require client JavaScript.

`IssueReport` composes the same overview, impact, deployment evidence, runbook,
action summary, and activity components for HTML and Markdown. Markdown uses
adaptive code fences for telemetry, numbered runbook steps, a canonical issue
link, and actions filtered by the authenticated projection. JSON retains the
exact typed fields. Native form fields remain where registry props do not
cover dynamic values or OAuth submit-button names.
