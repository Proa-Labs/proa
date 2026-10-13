# Repository scripts

These scripts operate on this repository only. They do not discover sibling
checkouts, private deployment directories, or credentials stored elsewhere on
a maintainer's workstation.

## Marketing development watcher

Run the marketing site with:

```bash
./scripts/watch-marketing.sh
```

The watcher keeps Cargo's registry and source cache in the shared `CARGO_HOME`
but defaults build artifacts to `target/watch/proa-marketing`. A PID lock under
`target/watch-locks` prevents two watchers from contending for the same target.
Set `PROA_MARKETING_CARGO_TARGET_DIR` to override the target directory; an
existing `CARGO_TARGET_DIR` is also honored.

## Standalone repository guard

First-party source and configuration use a consistent copyright holder and
Apache-2.0 SPDX header:

```bash
python3 scripts/copyright_headers.py --check
python3 scripts/copyright_headers.py --fix
```

`--check` is read-only and runs in CI; `--fix` adds missing headers and
normalizes known first-party names to `Proa Labs, Inc.`. It refuses unknown
holders or licenses. Source-installed components, third-party assets,
generated files, and exact-output fixtures are excluded; see
[`LICENSING.md`](../LICENSING.md) and the script's exclusion list.
First-party scaffold sources are checked alongside their workspace copies so
generated applications retain the same notices. Docker parser directives stay
ahead of the copyright header.

Run the public-checkout boundary check with:

```bash
python3 scripts/check_repository_boundary.py
```

It rejects Cargo dependency paths and workspace members that escape this
checkout, literal Rust includes that point outside or are missing, sibling
checkout assumptions, developer-machine paths, and downstream-only
runtime-template terminology in the public tree. It also validates local
Markdown and MDX links so moved public documentation cannot silently leave
broken references. Tracked `.local/` content and public dependencies on that
local storage are rejected, even when files were force-added despite Git's
ignore rules. CI runs this check and its isolated repository tests in the
workflow-lint job.

Redistributed assets and synthetic benchmark fixtures have a separate,
lightweight provenance guard:

```bash
python3 scripts/check_asset_provenance.py
```

The check requires binary and captured-fixture-shaped files to match the
manifest in [`docs/asset-provenance.md`](../docs/asset-provenance.md), and
rejects retired third-party fixture origins in demo and benchmark sources.

Publishable crates have a second, narrower boundary: their registry archives
must compile without files elsewhere in this repository. Check literal Rust
includes and `#[path]` modules against Cargo's archive file list, and verify
that every archive carries the canonical Apache-2.0 license text, with:

```bash
python3 scripts/check_crate_archives.py
```

## Browser security smoke tests

The root Node dependencies exist only for real-browser acceptance tests. They
are exact-pinned in [`pnpm-lock.yaml`](../pnpm-lock.yaml) and require Node.js 22
or newer. The repository resolves Node dependencies with pnpm; `npm` and `yarn`
are refused by a `preinstall` guard so a second lockfile cannot appear:

```bash
corepack enable
pnpm install --frozen-lockfile --ignore-scripts
pnpm exec playwright install chromium
pnpm run stream:boundary-smoke
pnpm run react:csp-smoke
```

CI runs both checks in Chromium. The first exercises adversarial streamed
boundary framing; the second hydrates a real React 19 component under a strict
nonce-based content security policy.

## Release flow

The standalone CLI binary workflow uses
[`verify_cli_release_assets.py`](verify_cli_release_assets.py) to enforce the
native x86_64 Ubuntu archive layout and assemble the checksum file before a
GitHub Release can publish. Exercise that contract locally with:

```bash
python3 scripts/test_cli_binary_distribution.py
```

Source fallbacks preserve the selected release: official versions build from
crates.io, main snapshots build their full commit SHA, and custom repositories
build the matching `proa-cli-v<version>` tag. An explicit unversioned
`--build-from-source` / `-BuildFromSource` request builds Git HEAD. If binary
metadata is unavailable, the official installer uses the latest crates.io
release. A failed pinned build stops rather than retrying another revision.

## Quickstart acceptance

Build both local CLI binaries, then verify the Basic app using the examples
extracted from `proa_marketing/src/docs/index.mdx`:

```bash
cargo build -p proa-cli --bins
python3 scripts/check_quickstart.py --cli target/debug/proa --work-dir /tmp/proa-quickstart-acceptance
```

This requires Rust 1.93 or newer, Cargo registry access, and the standalone
Tailwind CLI on PATH. It creates a new project, applies the documented edits,
runs formatting, strict lint, site checks, generated integration tests, and a
linked build. HTTP checks cover HTML, both Markdown entry points, the About
route, readiness, and retention of the starter design in the generated CSS.
It also removes the Markdown heading temporarily to verify that content lint
rejects the omission, then restores the source. Use `--profile release` to verify
an optimized build as well. The work directory keeps the generated source,
binary, and server log for inspection.

Workspace CI runs `--source-only` against the candidate CLI: formatting, strict
lint, site checks, and the omission probe, without compiling another app.
The `Released CLI quickstart` workflow performs the full check against the
latest stable crates.io CLI every Monday. It can also run manually or be called
by a release workflow with an exact candidate version. That provides a place to
verify public installation after publication without changing the docs.

## crates.io releases

The CLI and reusable libraries support crates.io installation. GitHub releases
and Git source installation remain available. The manual publishing tools
target `crates-io`; no automatic Cargo publishing workflow is enabled. Before
publishing, review package-name ownership and release readiness in
[`docs/registry.md`](../docs/registry.md).

```bash
python3 scripts/check_publish_policy.py
python3 scripts/check_crate_archives.py
python3 scripts/publish.py --dry-run
```

The dry run runs policy/archive checks and prints dependency order without
invoking `cargo publish`. It does not verify crates.io ownership or build
unpublished dependent archives. Actual publication verifies each package and
waits for its version to resolve before continuing with dependents:

```bash
# Run cargo login locally, or supply CARGO_REGISTRY_TOKEN through your secret manager.
python3 scripts/publish.py --skip-published
```

The publisher honors crates.io's retry time when new-crate registration is
rate limited. It stops on authentication, verification, or other upload errors.
After publishing, check a credential-free registry installation and all five
generated site templates, retaining artifacts for inspection:

```bash
python3 scripts/check_registry_install.py --work-dir /tmp/proa-registry-acceptance
```

The registry checker installs the latest stable CLI by default. Its optional
`--version` flag selects an exact release when reproducing a failure.

`--only crate1,crate2` selects packages. `--skip-published` checks exact versions
through the public crates.io API without sending a token. `--no-verify` skips
Cargo's package rebuild; prefer the default verification.

After an initial crates.io release, maintainers can use the pinned release-plz
wrapper and [`release-plz.toml`](../release-plz.toml) for release preparation:

```bash
scripts/release-plz.sh install
# Supply GIT_TOKEN for GitHub and CARGO_REGISTRY_TOKEN for Cargo publication.
scripts/release-plz.sh release-pr
scripts/release-plz.sh release
```

Both tools run policy and archive/license checks before publishing. They do
not discover credentials in other repositories or infrastructure services.
Demos and tools remain `publish = false`; release libraries inherit the
workspace's explicit crates.io destination policy.
