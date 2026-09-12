# Application-first site scaffolding

`proa new site` creates a production-shaped Rust SSR application rather than a
template-only example. The generated project owns its routes, representations,
database migrations, source UI, styling input, deployment files, and operational
endpoints.

## Create a site

Pass the complete destination path. Its final path segment becomes the Cargo
package name; `--title` controls the human-facing title independently.

```text
proa new site ./atlas --yes
proa new site ./marketing --template marketing --yes
proa new site ./docs --template docs --deploy docker --yes
proa new site ./empty --template minimal --starter blank --yes
```

The product presets are:

| Preset | Intended starting point |
|---|---|
| `app` | SQLite/SQLx, source UI, Tailwind source, and guided application content |
| `marketing` | Content-oriented site without a database |
| `docs` | Documentation content and HTML/Markdown representations |
| `minimal` | Small CSS-based application with optional blank starter content |
| `origin` | The complete ORIGIN routing atlas: guided dossiers for four networks, a neighbourhood graph, narrated live feeds, the flood-the-table rsjs island lab, self-hosted fonts, and a golden SSR byte-equality test |

Capabilities are orthogonal. Override a preset with `--database sqlite|none`,
`--ui proa|none`, `--styling tailwind|css`, `--starter guided|blank`, and
`--deploy none|docker|railway|coolify`. The `origin` preset owns its
application content, so only `--deploy` can be overridden there;
`examples/origin-routing-atlas` in this repository is the same template
rendered with workspace path dependencies, and a drift test keeps the two
byte-identical. The generated site pins its server-rendered `/` document with
`proa test --test golden` (refresh after intentional markup changes with
`PROA_UPDATE_GOLDEN=1 proa test --test golden`). `--dry-run --json` resolves the complete
plan without touching the filesystem. `--offline` forbids network-dependent
steps. Git initialization, verification, starting, and opening the browser are
explicit operations; `--yes` never opts into them.

Scaffolding is transactional. Files are assembled and validated in a staging
directory, then committed to the destination only after the plan succeeds.
Existing destinations and conflicts fail without partial writes.

## Generated application contract

The default application includes:

- an Axum/Proa server with `/healthz` and `/readyz` probes;
- typed HTML pages and Markdown representation negotiation;
- SQLite migrations, SQLx integration, and request-scoped `DataResolver` use;
- source-owned components under `src/components/`;
- static data under `src/static_data/` and public assets under `public/`;
- Tailwind source plus a checked-in prebuilt stylesheet, so the first page is
  styled even when a local Tailwind executable is unavailable;
- `proa.config.json`, registry lock metadata, local environment examples, and
  optional provider-native deployment files.

Run the generated app with:

```text
cd atlas
proa db migrate
proa dev
```

Use `proa build` for a linked release build and `proa start` to run an already
built release without compiling or watching.

`proa test` creates a unique migrated SQLite database under
`target/proa-tests/`, runs binary and integration-test targets through the
linked pipeline, and removes the database plus its WAL/SHM companions. Use
`--watch` to keep one isolated database while inputs change, or
`--keep-database` to preserve and print a failing run's exact path. Values from
`.env.test` overlay `.env` for test children while explicitly exported values
still win; neither file is rewritten.

## Evolve an existing site

Capability commands update configuration and owned files through the same
plan/commit machinery:

```text
proa add database
proa add representation markdown
proa add ui
proa add docs
proa add blog
proa add deploy docker
```

When adding a database to a non-generated or structurally customized server,
the CLI installs the complete SQLx module and records
`database.integration = "manual"`; it prints the remaining state-wiring step
instead of rewriting unfamiliar application code. `proa site check` recognizes
and reports that explicit partial integration.

Database lifecycle commands are scoped to the configured SQLite database and
its exact companion files:

```text
proa db create
proa db migrate
proa db status
proa db rollback
proa db reset
proa db drop
```

`reset` and `drop` require their confirmation flags in non-interactive use.
Migration rollback only applies migrations that have a generated down file.

## Generators

Application-aware generators register routes or modules instead of merely
dropping disconnected files:

```text
proa generate page about --markdown
proa generate endpoint api/events --method post --response json
proa generate model route-event prefix:text seen_at:datetime
proa generate migration add_route_visibility
proa generate component topology-card --children --recipe --markdown
proa generate component live-tape --async
proa generate layout docs --section
proa generate layout shell --document --replace-root
proa generate data navigation
```

Component generators default to `WebRenderSync`; `--async` emits an owned
`WebRender` future, and `--markdown` adds a semantic `MdRenderSync`
representation. Class composition uses stack-backed `ClassList`. Nested names
such as `pricing/feature-row` create and register the matching module hierarchy.

Only one generated document layout carries `// proa:document-root`.
`--replace-root` deliberately transfers that marker. All file generators support
dry-run planning, and component/layout/data generators also support overwrite
control with `--force`.

## Inspect and verify

```text
proa site status
proa site routes
proa site inspect /
proa site check
proa check --all
```

`site status` summarizes resolved capabilities, `site routes` discovers owned
routes, and `site inspect` explains a route's handler and representation policy.
`site check` validates the application structure and reports intentional manual
integration states. `check --all` combines Proa formatting/lints with the linked
Cargo/RSJS checks used by generated applications.

Registry source components remain under `proa ui`. `proa ui search` inspects the
configured catalog and `proa ui add` performs verified transactional installs,
recording hashes and provenance in `proa.lock.json`.
