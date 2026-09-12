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

## Registry release flow

Normal releases use [`release-plz.sh`](release-plz.sh). CI and local
maintainers call the same entrypoint:

```bash
scripts/release-plz.sh install

export GIT_TOKEN=<GitHub token>
export CARGO_REGISTRIES_PROA_TOKEN=<publisher token>
scripts/release-plz.sh release-pr
scripts/release-plz.sh release
```

The root [`release-plz.toml`](../release-plz.toml) selects released packages,
and [`check_publish_policy.py`](check_publish_policy.py) verifies that manifests,
release configuration, and CI all target the intended registry:

```bash
python3 scripts/check_publish_policy.py
```

The CI jobs run only when the repository variable
`PROA_STANDALONE_RELEASE_BASELINE` equals `0.3`. Keep it unset for the manual
standalone 0.3 bootstrap, verify those registry versions, and then set it to
enable normal release-plz automation. See
[`docs/registry.md`](../docs/registry.md) for package configuration and archive checks.

Local-only examples, demos, and benchmarks must set `publish = false`.
Publishable crates must inherit the workspace's explicit registry policy.

## Manual publish fallback

Use [`publish.py`](publish.py) only when release-plz is unavailable. The caller
must explicitly provide the registry credential:

```bash
export CARGO_REGISTRIES_PROA_TOKEN=<publisher token>
scripts/publish.py --skip-published
```

The script reads Cargo's locked workspace metadata, topologically orders
publishable crates, and treats an already-published version as an idempotent
skip. After an upload it waits for the exact version to become resolvable
before moving to dependent crates. Both this fallback and `release-plz.sh` run
the publish policy and crate-archive/license checks as a direct preflight. The
script never reads a token from another repository or an infrastructure
service.

Useful flags:

```bash
scripts/publish.py --dry-run
scripts/publish.py --only proa_core,proa_macros
scripts/publish.py --no-verify
scripts/publish.py --skip-published
```

`--dry-run` runs those preflight checks and prints the exact order without
invoking `cargo publish`. A full bootstrap cannot Cargo-verify every dependent
archive in advance because its fresh predecessors do not exist in the registry
yet; the real publish verifies each crate after those predecessors become
resolvable.

`--no-verify` skips Cargo's local package rebuild. Prefer the default
verification; the script's sparse-index wait is designed to avoid needing this
escape hatch.

See [`docs/registry.md`](../docs/registry.md) for the repository-side release
policy. Registry deployment and credential-provisioning runbooks belong to the
infrastructure repository that owns those systems.
