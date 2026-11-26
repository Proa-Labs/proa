# Managed public assets

The first build-output implementation covers a selected native application's
configured public directory. Rust refers to logical paths such as
`images/logo.svg`. The CLI copies and hashes public files after a successful
linked build; the server loads their manifest at startup. Asset edits change
resolved URLs without compiling those URLs into Rust or adding an application
compilation pass.

## Configure the inputs

To customize the build, author `proa.config.rs` in the project root. Wrap
a `Config` struct literal in `Ok` and inherit other settings with
`..Config::default()`. Use `proa run dev` and `proa run build` to run the
configured scripts. They default to the `proa dev` and `proa build` workflows.

Proa manages `public/` by default. Set `paths.public` to select a different
directory or set it to `None` to opt out.

```rust
use proa::config::{BuildConfig, Config, Result, ScriptsConfig};

pub async fn config() -> Result<Config> {
    Ok(Config {
        scripts: ScriptsConfig {
            dev: "bash scripts/dev.sh",
            build: "bash scripts/build.sh",
            ..Default::default()
        },
        build: BuildConfig {
            assets: Some(vec![]),
            ..Default::default()
        },
        ..Config::default()
    })
}
```

Proa generates a host Cargo runner under `<cargo-target>/proa-config/<key>/`.
Its `src/main.rs` imports `proa.config.rs` using `#[path = "..."]`, exports
the types through `proa::config`, and awaits `config()` on Tokio. The CLI uses the
result before starting the application build. Author-side Rust can use the
standard library, `proa::config::tokio`, and local modules declared with
`#[path = "..."]`. Add `--watch PATH` to the linked dev command in the wrapper
to reload the config when an additional input changes. The first build uses Cargo's registry cache
or fetches the runner's pinned dependencies.

The linked watcher evaluates `config().await` after each batch of edits.
It runs the same host executable directly while rustc's recorded inputs,
including files used by `include_str!` and `include_bytes!`, and the Cargo
settings it depends on are unchanged. A changed input or an uncertain
dependency list sends the runner back through Cargo. Files read inside the
async function are read afresh either way.

`proa init` and `proa new site` generate the Rust file.
Use `proa config validate` to check the authored settings.

`proa run dev` and `proa run build` launch the configured scripts in the project
directory and forward their CLI flags. In this example each wrapper enters
the linked pipeline using the CLI that launched it.

```bash
# scripts/dev.sh
exec "${PROA_SCRIPT_EXECUTABLE:-proa}" dev "$@"
```

```bash
# scripts/build.sh
exec "${PROA_SCRIPT_EXECUTABLE:-proa}" build "$@"
```

The `proa dev` and `proa build` commands enter the built-in linked
pipeline directly. Use `scripts.named` with `scripts.before.build` or
`scripts.before.dev` for preparation commands. Cargo discovers `build.rs` scripts in the application
and its dependencies during the Cargo passes.

Existing asset commands can populate the public directory. `build.assets: None`
lets Proa infer those commands, while `Some(vec![])` disables them. When the
default `public/` directory is absent after the build, Proa skips publication.
A missing custom directory fails the build. Projects with `paths.public: None`
keep their existing unmanaged serving path.
`--no-assets` on `build` or `dev` skips both asset commands and publication.

```sh
proa run --root path/to/app build --profile dev
proa run --root path/to/app
proa run --root path/to/app dev
```

The descriptor and final Cargo passes remain unchanged. For a configured
public directory, `run` executes build preparation and asset commands, builds
the exact final executable, publishes the catalog, then starts the server.
`dev` publishes before each successful launch and adds the public directory
to its watch set. A change confined to public files, or to the stylesheets
that generate them, is republished without Cargo. A server that obtained its
catalog from `FrameworkBuilder` or `PublicAssets::from_env` under `proa dev`
follows that publication in place: it registers as a live reader of the output
directory and reloads the catalog on its next request, so `proa dev` leaves it
running and only reloads the browser, or swaps stylesheets for a
stylesheet-only change (the dev client refetches the page to learn the
stylesheet's new hashed URL). Every other server is restarted so it can load the new
catalog. That covers a catalog loaded with `PublicAssets::load`, an application
built against a framework release without in-place reloading, `--no-reload`,
and a server that is still starting. Managed Tailwind is not rerun when every
changed public file is a binary format it never reads, such as an image, font,
or video. Static-template hot updates remain available when public inputs have
not changed. This is live reload, not state-preserving HMR.
The watcher detects directory symlink cycles; managed publication rejects
symlinks within the public tree.

The watcher also checks that the active manifest exists, including when
Cargo's target directory is outside the project. Removing the manifest or
moving its directory republishes the public files and restarts the existing
executable so it acquires the restored catalog's reader lock. Removing the
executable along with the catalog triggers a linked build. A normal atomic
manifest replacement does not trigger recovery.

`proa run dev --open` uses the browser URL reported by the app. Proa's Axum
serve helper reports its bound HTTP listener. A custom server can report
its own HTTPS origin or mounted path.

```rust
let startup = proa::axum::DevServerStartup::from_env()?;
// Build the routers and bind the listeners before reporting readiness.
startup.ready(&config.app_origin)?;
```

The incident desk reports the app origin once both TLS listeners are bound.
Apps using their own server without reporting readiness retain the TCP
probe and their configured HTTP URL.

## Minify generated RSJS

Linked Proa builds enable minification of generated island modules, component
fragments, client modules and classes, and both RSJS runtimes by default.
For readable JavaScript while debugging, set `build.minify_rsjs` to `false`
in `proa.config.rs`.

```rust
use proa::config::{BuildConfig, Config, Result};

pub async fn config() -> Result<Config> {
    Ok(Config {
        build: BuildConfig {
            minify_rsjs: false,
            ..Default::default()
        },
        ..Config::default()
    })
}
```

The linked Cargo build records the preference in the binary. `proa build`
applies it when it exports the modules into the bundle (see
[deployment](#publication-cleanup-and-deployment)). A server without exported
modules minifies once at startup, including when the binary is launched
directly. The modules are hashed after minification, so the setting changes
their file names. Public JS files are handled by `build.assets` commands.

## Output layout

The local output used by `proa dev`, `proa run`, and `proa test` is a store
under Cargo's resolved target directory, including `CARGO_TARGET_DIR`:

```text
target/proa-output/<application-target>/<selection-sha256>/
  .proa-assets                 ownership marker
  .writer.lock                 one publishing process
  .readers.lock                shared by live catalogs
  staging/attempt-*/           incomplete work, never served
  objects/images/logo.<16 hex>.svg
  manifest.json                active logical-path catalog
  fingerprints.json            durable immutable URL-to-full-digest reservations
```

The locks, staging area, and reservation history let a writer replace the
manifest under a running server. `proa build --artifact-out` writes a different,
read-only layout with none of them (see
[deployment](#publication-cleanup-and-deployment)). Both layouts serve the same
URLs.

The selection includes package, Cargo target kind/name, profile, explicit
target triple and feature selection. Source edits and CLI executable changes
do not change this output namespace. Files under Cargo's target directory are
excluded by the existing compiler input policy. Public source files remain
eligible compiler inputs: `include_str!` and build scripts may consume them.
There is no general asset-only Cargo bypass in this implementation.
An image used only through the catalog can reuse both Cargo passes. A file
also consumed by `include_str!`, `include_bytes!` or a build script must still
invalidate its Rust consumers. Public directories are not blanket-excluded
from compiler input tracking.

Publication currently scans, copies and verifies the complete public tree on
each build, including unchanged files. The existing Cargo scheduler owns Rust
compilation; public-file publication is serial. It flushes new object files
and then each affected directory once before replacing the manifest. Asset-only
watcher updates and compiler-cache changes require separate input-ownership
work; this catalog does not claim to eliminate their costs.

## Load and use the catalog

`proa run` and `proa dev` set `PROA_PUBLIC_ASSETS` on the server process after
publication. `FrameworkBuilder` loads and mounts that catalog at startup. An
exported executable started on its own loads the catalog bundled beside it
instead (see [deployment](#publication-cleanup-and-deployment)).
`proa test` also publishes the configured public directory and sets
`PROA_PUBLIC_ASSETS` when launching the selected test executable.
With the configuration above, place the source image at
`public/images/logo.svg` and use that same path in the template.

```rust
use axum::{routing::get, Router};
use proa::{html_sync, WebContext, WebRenderSync, WriteError};
use proa::axum::{FrameworkBuildError, FrameworkBuilder, RouteResponse};

struct Logo;

impl WebRenderSync for Logo {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <main data-logo>
                <img src={cx.url("public/images/logo.svg")} alt="Logo" />
            </main>
        }.render(cx)
    }
}

async fn home() -> RouteResponse {
    RouteResponse::ssr(Logo)
}

fn app() -> Result<Router, FrameworkBuildError> {
    FrameworkBuilder::new()
        .with_dynamic_routes(Router::new().route("/", get(home)))
        .try_build()
}
```

`cx.url("public/…")` resolves a logical path in the catalog. The `public/`
prefix selects the catalog even if `paths.public` names a different directory.
Nested and streamed components inherit the catalog from the request. Lookup
shares the URL prepared at startup and a missing asset returns a render error.
Query strings and fragments are preserved. For example,
`cx.url("public/icons.svg?v=2#home")` resolves `icons.svg` and appends
`?v=2#home` to its fingerprinted URL.
For an owned URL outside a template, use `cx.url(path).try_to_string()?`.
RSJS query and mutation request URLs run in the browser, which has no
catalog, so `cx.url("public/…")` there is a compile error. Resolve the URL
during SSR and pass it through a signal instead.
Literal attributes are emitted as written. Ordinary links such as
`cx.url("/products")` continue to use the app's base path.

The built-in page cache includes the catalog, base path, and asset prefix in
its cache key.
For a custom Axum router, merge `assets.router()`, wrap the result with
`assets.serve_root_files(router)` to answer the root icon paths, and apply
`RouteContextLayer::with_public_assets(assets)` to the combined router. For a
buffered render, pass the loaded catalog to
`RouteResponse::render_to_bytes_with_public_assets(&assets)`. A page with RSJS
islands needs the published module names as well: use
`RouteResponse::render_to_bytes_with_assets(&assets, Some(&rsjs))` with the
same `RsjsAssets` value the router serves.
For custom HTML caches, include `PublicAssets::cache_key()` or discard cached
HTML on catalog replacement. A `PublicAssets` value describes one catalog
generation. Read the key and URLs from the request's
`Extension<PublicAssets>`, which `RouteContextLayer` pins to the generation
that request renders with, or call `assets.current()` once per request when
rendering by hand. In production both are the catalog loaded at startup.

The default browser URL is `/_proa/immutable/images/logo.1234abcd5e6f7a8b.svg`.
A hashed file has exactly one URL: there is no alias at the logical path, so
always take the URL from `cx.url("public/...")`.
Use `with_base_path` only when the app is deployed under a URL subpath.
For example, a prefix-stripping proxy can expose the app at `/apps/site`.
The builder then prepares
`/apps/site/_proa/immutable/images/logo.1234abcd5e6f7a8b.svg`
using the same catalog and incoming routes. Only catalog entries are routed.
Private metadata, staging paths, other Cargo output, and unknown hashes return 404.

## Byte identity and delivery

Every public file is copied into private staging before hashing. SHA-256 is
computed with bounded buffering; the manifest records its full digest, length
and media type. Hashed file names carry the first sixteen hex characters,
matching generated fonts' filename convention. Already fingerprinted names
whose suffix matches their bytes are retained. A different full digest claiming
the same URL fails publication, including after the earlier object was pruned.
The reservation history in the local store records published immutable URLs.
Editing a public JS file updates its stable URL's validator without adding a
reservation.
Generated fonts keep their existing names and bytes; the catalog does not run
font generation again. Fonts vendored into `public/fonts` are published under
those names at `/_proa/immutable/fonts/`, included in the deployable copy, and
their copied bytes are verified.
The catalog does not trust a filename or an earlier font lock digest in place
of that integrity check.

Published URLs use two prefixes, split by cache policy, so one path rule per
directory is the whole cache configuration of a proxy or static host.

| Input | URL | `Cache-Control` |
| --- | --- | --- |
| Images, audio, video, downloads, fonts and other opaque files | `/_proa/immutable/<path>.<16 hex>.<ext>` | `public, max-age=31536000, immutable` |
| Stylesheets | `/_proa/immutable/<path>.<16 hex>.css`, hashed after their references are rewritten | `public, max-age=31536000, immutable` |
| RSJS runtimes and generated modules | `/_proa/immutable/rsjs/...<16 hex>.js` | `public, max-age=31536000, immutable` |
| Public JS/MJS/CJS, HTML, JSON, web manifests, source maps, text and XML | `/_proa/mutable/<logical path>` | `public, no-cache` plus a strong ETag |
| `favicon.ico`, `apple-touch-icon.png`, `apple-touch-icon-precomposed.png` at the top of the public directory | also `/favicon.ico` and so on | `public, no-cache` plus an ETag |
| Remote scripts and runtime uploads | Application-owned; outside this build catalog | |

A hashed file has exactly one URL. Files under `/_proa/mutable/` keep their
names, so relative references between them continue to resolve. The three root
files are answered at the origin root in addition to their hashed URL because
browsers request those paths on their own; an application route at the same
path wins, and no other public file gets a root URL. A public directory may not
contain a top-level `_proa/` directory.

### Stylesheets

Publication rewrites every `url(...)`, `@import`, and `image-set(...)` in a
stylesheet that names another public file by relative path. The reference
becomes a relative path to that file's published location, and the stylesheet
is hashed afterwards. A changed image therefore renames every stylesheet that
refers to it. The source files are never modified.

References that cannot follow are reported as build warnings:

- a relative URL that names no public file,
- a root-relative URL such as `url(/images/a.png)` (use a path relative to the
  stylesheet),
- a stylesheet that is not valid UTF-8 CSS.

A reference that leads back to the stylesheet it is in, directly or through
other stylesheets, is reported and left as written.

Files that keep their names (public JS, HTML, JSON, web manifests) are not
rewritten. A quoted relative path in one that names a hashed file is reported:
nothing is served at that path any more. Render the file's URL into the page
with `cx.url("public/...")` and read it from there.

### No environment in files

Files are served byte for byte as built under every base path and asset host.
The server no longer rewrites root-relative URLs inside catalog CSS or JS at
startup. Files retain the existing `tower-http` streaming, HEAD and Range
behavior. (`base_path_assets`, the wrapper for application-defined `/static`
routers, is separate and still rewrites.)

Hand-written public JS stays under `/_proa/mutable/` and is not hashed. When it
needs the base path, it reads it from the page: with a non-empty base path,
pages with RSJS islands carry
`<script type="application/json" id="__proa">{"basePath":"/shop"}</script>`
ahead of the runtime tags, and the RSJS runtime's `app_url` reads the same
element. When it needs the URL of a hashed file, render that URL into the page
with `cx.url("public/...")` (a `data-` attribute, for example) instead of
hard-coding a path.

### RSJS modules

`FrameworkBuilder` serves the runtimes and every generated module by default at
content-hashed names, cached for a year (call `.without_rsjs_assets()` to opt
out):

```text
/_proa/immutable/rsjs/runtime.<16 hex>.js
/_proa/immutable/rsjs/query.<16 hex>.js
/_proa/immutable/rsjs/island/<name>.<16 hex>.js
```

Modules import each other by relative path and contain no base path. Modules
that import each other in a cycle share one hash. Pages link the hashed names
automatically. Every script and `modulepreload` tag carries the CSP nonce and
an `integrity="sha384-..."` attribute. No import map is involved.

A custom Axum router prepares the same files itself:

```rust
let rsjs = proa::axum::RsjsAssets::prepare()?;
let router = router
    .merge(rsjs.router())
    .layer(RouteContextLayer::new(metadata).with_rsjs_assets(&rsjs));
```

Custom adapters that serve their own copies at the canonical paths with
`rsjs::CanonicalRsjsAssetUrls` and `rsjs::runtime_assets::with_base_path` are
unaffected.

## Publication, cleanup and deployment

Publication rejects unsafe paths, symlinks within the input/output trees, a
top-level `_proa/` directory in the public tree, observed source edits during
copying, and digest collisions. In the local store, objects are placed
before an atomic manifest replacement. Failed compilation or preparation keeps
the previous manifest. Loading a catalog verifies every referenced object's
bytes before accepting it.

One writer lock protects each local output scope. Each loaded catalog holds a reader
lock for that directory. Cleanup after successful publication removes
unreferenced objects only when no catalog is live; the dev supervisor retries
after stopping the old child. A dev server that follows publication in place
keeps its reader lock, so superseded objects stay on disk until that server is
next restarted. An interrupted attempt's staging directory is
removed by the next writer. Collision reservations survive object cleanup.
Post-publication cleanup or durability errors are reported as warnings because
the new pointer is already active. Cleanup never adopts unrelated directories.

For deployment, `proa build --artifact-out <path>` writes the complete output
side by side: the executable and, beside it, a read-only bundle in a directory
named after the executable with `.assets` appended. File names below are
illustrative.

```text
/out/
  app                                     linked executable
  app.assets/
    .proa-assets                          marker: Proa wrote this directory
    manifest.json                         server-only: logical path -> URL, hashes, cache policy
    public/                               mirrors browser URLs exactly
      favicon.ico                         root files, always served by the origin
      _proa/
        immutable/
          styles.6ca57fe9e41b02d7.css
          images/hero.1234abcd5e6f7a8b.webp
          fonts/geist-latin-400-normal.9f2c1a3b7d4e5f60.woff2
          rsjs/runtime.9a8b7c6d5e4f3a2b.js
          rsjs/query.2b3c4d5e6f7a8b9c.js
          rsjs/island/search.4e5f6a7b8c9d0e1f.js
        mutable/
          search-index.mjs
          data.json
```

The bundle holds no lock files, `objects/`, `staging/`, or `fingerprints.json`:
nothing writes to it again. It is assembled beside its destination and moved
into place whole, so an export that fails while building leaves the previous
bundle untouched. The move is two renames, the old bundle out and the new one
in. A build killed between them leaves no bundle at the destination; the old
one stays in a `.proa-export-*` directory beside it until a later export.

The RSJS modules get into the bundle because `proa build` starts the executable
it just built with `PROA_EXPORT_FRAMEWORK_ASSETS=<dir>`. The framework answers
before `main` runs, writes the modules, and exits, so no application startup
code (database, configuration, ports) runs during a build. An executable that
does not link the RSJS runtime is never started; one that links it is, even if
it serves its own copies of the modules. A cross-compiled executable that cannot
run on the build machine is skipped with a note, and the server then prepares
and serves the modules itself. When the bundle holds the modules exported from
the same executable, the server reads them from disk and does not parse,
rewrite, or minify any module at startup. It still computes one digest over
the linked sources to recognize them, and the catalog verifies each file.

Two limits. The export fails soft: if the executable cannot answer, the build
warns and writes the bundle without the modules, and the server prepares them
at startup. And the platform, not Proa, orders the code that runs before
`main`: another crate's constructor (a C++ static initializer, a `#[ctor]`
function) may run ahead of the export and therefore during the build.

Only an explicit `--artifact-out` bundles, and it publishes only there. A build
without it keeps publishing into the target directory, and `proa start` passes
that catalog to the executable under `<target-dir>/release/`. A path that
cannot hold the bundle is refused before anything compiles. Cargo's own output directory never
holds a bundle, so a plain `cargo run --release` cannot pick up a catalog that
an earlier `proa build` left behind. A server started without
`PROA_PUBLIC_ASSETS` loads the catalog beside its own executable, so copying
both to any directory is the whole deployment. No environment variable, extra
flag, or copy script is involved.

```dockerfile
RUN proa build --locked --artifact-out /out/app
COPY --from=build /out/ /app/
CMD ["/app/app"]
```

This layout follows the same idea as Nuxt's `.output` directory (built by Nitro): the build writes one directory that holds the server and the static files it serves, the server finds those files relative to itself, and deploying is copying that directory and starting the entry point.

```text
Nuxt: nuxt build                        Proa: proa build --artifact-out /out/app

.output/                                /out/
  nitro.json        build metadata
  server/                                 app                  server (one native executable)
    index.mjs       entry point
    chunks/         server code
    node_modules/   traced dependencies
                                          app.assets/
                                            manifest.json      server-only catalog
  public/           static files            public/            static files
    favicon.ico                               favicon.ico
    _nuxt/                                    _proa/immutable/
      entry.3f9a1c2b.js                         rsjs/runtime.9a8b7c6d5e4f3a2b.js
      styles.8d0e4a71.css                       styles.6ca57fe9e41b02d7.css
                                              _proa/mutable/
                                                data.json

node .output/server/index.mjs           /out/app
```

| | Nuxt `.output` | Proa |
| --- | --- | --- |
| Server | `server/`: entry module, chunks, traced `node_modules`; needs Node | one executable |
| Static files | `public/`, found relative to the server entry | `<executable>.assets/public/`, found beside the running executable |
| Location | fixed `.output/` in the project | wherever `--artifact-out <path>` puts the executable |
| Fingerprinting | bundler output under `public/_nuxt/`; other public files keep their names | hashed files under `public/_proa/immutable/`, files that keep their names under `public/_proa/mutable/`; `manifest.json` maps logical paths to URLs |
| Static host | upload `public/` | upload `public/_proa/` and set `PROA_ASSET_PREFIX` |
| Deploy | copy `.output/`, run `node server/index.mjs` | copy the executable and its `.assets` directory, run the executable |

The differences come from the server being a single native binary. There is no server directory to wrap it in, so the bundle is a sibling named after the executable instead of a shared root. Inside it, `public/` mirrors browser URLs exactly, the same way `.output/public/` does. The output path stays a flag because hosts that cache `target/` (Railway) need the result written outside it.

Bundle files are world-readable and its directories traversable whatever the
build's umask, so a non-root runtime user can load it from a read-only image.
The bundle is written before the executable is replaced, so a failed export
leaves the previous pair working. A build that publishes nothing, because of
`--no-assets` or `paths.public: None`, removes a catalog left beside the same
executable path by an earlier build once the new executable is in place. An export is refused while a running server is reading the bundle it would
replace: the build fails before the executable is replaced and leaves both as
they were. A directory at that path that Proa did not
create fails the build untouched, and so does an export path inside the public
directory. Do not manually modify files in the bundle.

`PROA_PUBLIC_ASSETS` still overrides the location. `proa dev`, `proa run`, and
`proa test` set it to the catalog they publish under the target directory, and
`proa start --public-assets <dir>` sets it for a catalog kept elsewhere.

```bash
proa start --root . --bin ./app
```

If `PROA_PUBLIC_ASSETS` is set or a Proa catalog directory exists at the bundled path, and
that catalog is missing or invalid, `FrameworkBuilder::try_build` reports a
startup error. A partially copied bundle therefore fails startup instead of
serving pages without their assets. Apps can also use
`with_public_assets` to supply an explicit catalog or check required logical
paths at startup. The catalog has no binary hash binding.

These are local-output cleanup rules. The active router exposes only its own
manifest, so an old browser URL may return 404 after a server replacement.
Retaining files for cached HTML, rolling deployments or rollback requires a
static host that keeps earlier files (next section). Cargo's existing scheduler
still owns application compilation.

## Serving the output from a static host

The bundle's `public/` directory mirrors browser URLs, so it can be uploaded to
a static host or CDN bucket as it is. Upload `app.assets/public/_proa/` as two
syncs:

| Directory | Sync | `Cache-Control` |
| --- | --- | --- |
| `_proa/immutable/` | additive, never delete | `public, max-age=31536000, immutable` |
| `_proa/mutable/` | overwrite | `public, no-cache` |

Finish the upload before the new server version takes traffic. Never delete
old files under `immutable/`, so pages from the previous deploy keep working
during a rollout. The host must send CORS headers
(`Access-Control-Allow-Origin`), because module scripts and fonts are fetched
in CORS mode.

Then set `PROA_ASSET_PREFIX` on the server to the URL the `public/` directory
was uploaded to, for example `https://cdn.example.com/site`, or a root-relative
path such as `/static-host`. The variable is read at startup and ignored under
`proa dev`. With it set:

- every `cx.url("public/...")` URL, every font URL from `Font::head`, and (when
  the bundle holds them) every RSJS module URL starts with that prefix instead
  of the base path,
- application routes such as `cx.url("/pricing")` keep using `PROA_BASE_PATH`,
- RSJS script and preload tags get `crossorigin`,
- the managed head's URL policy accepts the asset origin for icons and
  stylesheets.

The server keeps serving the same files itself, so the prefix can be removed at
any time. The root files (`/favicon.ico` and the two touch icons) are always
answered by the origin.

With a cross-origin prefix and a nonce policy (`with_strict_csp` or
`with_csp_policy`), `FrameworkBuilder` appends the asset origin to `style-src`,
`img-src`, `font-src`, `media-src`, and `connect-src`, including when the
application set those with `with_directive`. A directive set to `'none'` stays
closed. `script-src` is unchanged, because the nonce plus `'strict-dynamic'`
already covers scripts on any host.

The programmatic equivalents are `proa::axum::AssetPrefix::new(...)` and
`PublicAssets::load_hosted(output, &base_path, Some(&prefix))`.
