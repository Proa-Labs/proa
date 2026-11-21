# Package distribution

Install the CLI and framework libraries from crates.io. No private Cargo
registry, GitHub credential, or registry token is required to install or build
Proa.

```bash
cargo install proa-cli --locked
```

Cargo installs both `proa` and `cargo-proa`. Generated projects use compatible
crates.io versions and commit their `Cargo.lock` for reproducible builds.

## Application dependencies

Most applications start with `proa`:

```toml
[dependencies]
proa = { version = "0.4", features = ["axum"] }
```

Rendering traits, buffers, metadata, and macros are exported from `proa::`.
Streaming components and loaders live under `proa::stream`; the Axum adapter is
`proa::axum`, and the portable framework API is `proa::framework`.

| Feature | Provides |
| --- | --- |
| `markdown` (default) | `md!` and `md_sync!` compilation |
| `stream` (default) | Runtime-independent loaders and streaming components |
| `rsjs` (default) | Browser-language macros and types through `proa::rsjs` |
| `axum` | Axum adapter, portable framework API, and streaming |
| `framework` (default) | Portable framework API without the HTTP adapter |
| `rt-tokio` | Eager Tokio loader execution |
| `actions` | Action data and the `Action` derive |
| `react` | React interoperability and the `island` attribute |
| `html-to-markdown` | Explicit HTML-to-Markdown conversion |
| `no-perf-hints` | Disable compiler performance hints |

RSJS retains its own package version and is re-exported by the default `rsjs`
feature; applications need no separate RSJS dependency. Font collectors
remain build dependencies. Advanced integrations may continue to depend directly
on implementation crates; the facade re-exports the same types and introduces
no additional rendering layer. Renamed facade dependencies are supported by the
macro compiler too.

## crates.io publication

Reusable libraries and `proa-cli` restrict publication to `crates-io`. The
marketing site, demos, benchmarks, and example applications remain unpublished.
The JSON and Tailwind helpers use Proa-prefixed package names because the
unprefixed names belong to unrelated crates.io projects:

| Package | Rust crate name |
| --- | --- |
| `proa_const_json` | `const_json` |
| `proa_const_json_macros` | `const_json_macros` |
| `proa_tailwind_merge` | `tailwind_merge` |
| `proa_tailwind_merge_macro` | `tailwind_merge_macro` |

For a library intended for publication:

1. Inherit `publish.workspace = true` and the workspace license/MSRV.
2. Give internal path dependencies matching versions without an alternative registry.
3. Add the package to [`release-plz.toml`](../release-plz.toml).
4. Inspect the archive and run the publication checks.

```bash
python3 scripts/check_repository_boundary.py
python3 scripts/check_publish_policy.py
python3 scripts/check_crate_archives.py
scripts/release-plz.sh check
```

Local commands and the manually triggered **Release crates** Actions workflow
share `scripts/release-plz.sh`. Install its pinned tools with
`scripts/release-plz.sh install`, then run `check`, `release-pr`,
`release-dry-run`, and `release` as documented in
[`scripts/README.md`](../scripts/README.md#cratesio-releases).
Release-plz owns version and changelog updates. The wrapper publishes libraries
before verifying generated projects against crates.io and publishing the CLI.
Nothing publishes automatically on push or merge. Never commit publishing
credentials.

## Component catalogs

`proa registry` and `proa ui add` refer to HTTP component catalogs, not Cargo
registries. Free components need no token; optional Pro components use
`PROA_PRO_TOKEN`. Existing Pro catalog credentials remain supported during
migration. See the [component catalog protocol](../proa_cli/docs/ui-registry-v1.md).
The old Cargo-only `proa auth` helper has been retired.

Never commit credentials. Private release runbooks belong in the release
owner's knowledge base. Report vulnerabilities through [`SECURITY.md`](../SECURITY.md).
