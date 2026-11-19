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
`..Config::default()`. Dev and build default to `proa run dev` and
`proa run build`.

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

`proa dev` and `proa build` launch the configured scripts in the project
directory and forward their CLI flags. In this example each wrapper enters
the linked pipeline using the CLI that launched it.

```bash
# scripts/dev.sh
exec "${PROA_SCRIPT_EXECUTABLE:-proa}" run dev "$@"
```

```bash
# scripts/build.sh
exec "${PROA_SCRIPT_EXECUTABLE:-proa}" run build "$@"
```

The `proa run dev` and `proa run build` commands enter the built-in linked
pipeline directly. Use `scripts.named` with `scripts.before.build` or
`scripts.before.dev` for preparation commands. Cargo discovers `build.rs` scripts in the application
and its dependencies during the Cargo passes.

If you change which Bash command `scripts.dev` selects during a session,
the watcher prints the new command. Run `proa dev` again to enter that
wrapper and pick up its environment. Other edits continue to use the
current wrapper.

Existing asset commands can populate the public directory. `build.assets: None`
lets Proa infer those commands, while `Some(vec![])` disables them. When the
default `public/` directory is absent after the build, Proa skips publication.
A missing custom directory fails the build. Projects with `paths.public: None`
keep their existing unmanaged serving path.
`--no-assets` on `build` or `dev` skips both asset commands and publication.

```sh
proa build --root path/to/app --profile dev
proa run --root path/to/app
proa dev --root path/to/app
```

The descriptor and final Cargo passes remain unchanged. For a configured
public directory, `run` executes build preparation and asset commands, builds
the exact final executable, publishes the catalog, then starts the server.
`dev` publishes before each successful launch and adds the public directory
to its watch set. Changed public files require a server restart and browser
reload so the server can load the new catalog. Static-template hot updates
remain available when public inputs have not changed. Managed CSS currently
uses this restart path too. This is live reload, not state-preserving HMR.
The watcher detects directory symlink cycles; managed publication rejects
symlinks within the public tree.

The watcher also checks that the active manifest exists, including when
Cargo's target directory is outside the project. Removing the manifest or
moving its directory republishes the public files and restarts the existing
executable so it acquires the restored catalog's reader lock. Removing the
executable along with the catalog triggers a linked build. A normal atomic
manifest replacement does not trigger recovery.

`proa dev --open` uses the browser URL reported by the app. Proa's Axum
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

The linked Cargo build records the preference in the binary.
Axum minifies once at startup after applying the deployment base
path, including when the binary is launched directly. Generated RSJS keeps
its existing routes and cache headers. Public JS files are handled by
`build.assets` commands.

## Output layout

Output uses Cargo's resolved target directory, including `CARGO_TARGET_DIR`:

```text
target/proa-output/<application-target>/<selection-sha256>/
  .proa-assets                 ownership marker
  .writer.lock                 one publishing process
  .readers.lock                shared by live catalogs
  staging/attempt-*/           incomplete work, never served
  objects/images/logo.<hash8>.svg
  manifest.json                active logical-path catalog
  fingerprints.json            durable immutable URL-to-full-digest reservations
```

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
publication. `FrameworkBuilder` loads and mounts that catalog at startup.
`proa test` also publishes the configured public directory and sets
`PROA_PUBLIC_ASSETS` when launching the selected test executable.
With the configuration above, place the source image at
`public/images/logo.svg` and use that same path in the template.

```rust
use axum::{routing::get, Router};
use proa_core::{WebContext, WebRenderSync, WriteError};
use proa_framework_axum::{FrameworkBuildError, FrameworkBuilder, RouteResponse};
use proa_macros::html_sync;

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
        .with_rsjs_assets()
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

The built-in page cache includes the catalog and base path in its cache key.
For a custom Axum router, merge `assets.router()` and apply
`RouteContextLayer::with_public_assets(assets)` to the combined router. For a
buffered render, pass the loaded catalog to
`RouteResponse::render_to_bytes_with_public_assets(&assets)`.
For custom HTML caches, include `PublicAssets::cache_key()` or discard cached
HTML on catalog replacement.

The default browser URL is `/_proa/assets/images/logo.1234abcd.svg`.
Use `with_base_path` only when the app is deployed under a URL subpath.
For example, a prefix-stripping proxy can expose the app at `/apps/site`.
The builder then prepares `/apps/site/_proa/assets/images/logo.1234abcd.svg`
using the same catalog and incoming routes. Only catalog entries are routed.
Private metadata, staging paths, other Cargo output, and unknown hashes return 404.

## Byte identity and delivery

Every public file is copied into private staging before hashing. SHA-256 is
computed with bounded buffering; the manifest records its full digest, length
and media type. Preferred opaque-file URLs use the first eight hex characters,
matching generated fonts' filename convention. Already fingerprinted names
whose suffix matches their bytes are retained. A different full digest claiming
the same URL fails publication, including after the earlier object was pruned.
The reservation history records published immutable URLs. Editing JS or CSS
updates the stable URL's validator without adding a reservation for its
private storage object.
Generated fonts keep their existing names and bytes; the catalog does not run
font generation again. When fonts are inside the configured public directory,
they are included in the deployable copy and their copied bytes are verified.
The catalog does not trust a filename or an earlier font lock digest in place
of that integrity check.

| Input | Current behavior |
| --- | --- |
| Images, audio, video, downloads, fonts and other opaque files | Fingerprinted preferred URL; `public, max-age=31536000, immutable` |
| Public JS/MJS/CJS, CSS, HTML, JSON, manifests, maps, text and XML | Hashed storage object, stable logical public URL; `no-cache` |
| Existing embedded RSJS | Existing registry, loading plan and routes; unchanged by the public catalog |
| Remote scripts and runtime uploads | Application-owned; outside this build catalog |

Logical aliases revalidate, so existing relative dependencies inside public JS
and CSS continue to resolve within the public mount. Root-relative references
keep their application-root meaning; the application must still serve those
routes. With a base path, the existing JS/CSS parsers compose root-relative URLs
once at startup, with a 32 MiB total input budget. These derived responses stay
on stable URLs and get ETags for their actual delivered bytes. Fingerprinted
objects are never rewritten at delivery time. Files retain the existing
`tower-http` streaming, HEAD and Range behavior; rewritten JS/CSS responds with
the full representation to range requests.

Complete immutable JavaScript delivery is still separate work. It needs a
supported import dependency closure and final byte hashing for the RSJS
runtimes and path-named modules/classes. This change neither introduces import
maps nor exports embedded JS from the executable. It does not change the
binary's current embedded-JS deployment model.

## Publication, cleanup and deployment

Publication rejects unsafe paths, symlinks within the input/output trees,
observed source edits during copying, and digest collisions. Objects are placed
before an atomic manifest replacement. Failed compilation or preparation keeps
the previous manifest. Loading a catalog verifies every referenced object's
bytes before accepting it.

One writer lock protects each output scope. Catalog clones share one reader
lock for that directory. Cleanup after successful publication removes
unreferenced objects only when no catalog is live; the dev supervisor retries
after stopping the old child. An interrupted attempt's staging directory is
removed by the next writer. Collision reservations survive object cleanup.
Post-publication cleanup or durability errors are reported as warnings because
the new pointer is already active. Cleanup never adopts unrelated directories.

For deployment, copy the complete output directory with the binary and set
`PROA_PUBLIC_ASSETS` to its deployment location. `--artifact-out` exports only
the executable. The catalog can be loaded from a read-only deployment; the
reader lock file must be included. Do not manually modify immutable objects.

To start a packaged app with the CLI, pass the copied catalog alongside the
exported executable.

```bash
proa start --root . --bin ./app --public-assets ./public-assets
```

If the deployment path is set and the catalog is missing or invalid,
`FrameworkBuilder::try_build` reports a startup error. Apps can also use
`with_public_assets` to supply an explicit catalog or check required logical
paths at startup. The catalog has no binary hash binding.

These are local-output cleanup rules. The active router exposes only its own
manifest, so an old browser URL may return 404 after a server replacement.
Retaining files for cached HTML, rolling deployments, CDN export or rollback
requires a deployment retention policy. Cargo's existing scheduler still owns
application compilation.
