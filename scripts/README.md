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

## Windows portability from macOS or Linux

Run the Windows GNU/Wine checks with Docker Desktop or a Linux Docker engine:

```bash
bash scripts/check_windows_wine.sh
```

This builds an amd64 Linux image with Rust, MinGW, and Wine 10, cross-compiles
Windows test binaries, and executes them under Wine. On Apple Silicon, Docker
translates the amd64 container. The checkout is mounted read-only; named Docker
volumes retain build artifacts and registry downloads. The first run downloads
and compiles dependencies, and requires several GB of Docker disk space.

The checks cover PE32/PE32+ normal and delay-load imports, COFF short-import
archive members, executable/PDB root selection, a real PE launch snapshot,
Windows file replacement, CLI argument handling, and `proa start` from a path
containing spaces. The PE/COFF fixtures also run directly on macOS:

```bash
cargo test --locked -p proa_build -p rsjs_transport --lib windows_
```

Wine is a Windows API compatibility layer, so this is supplementary coverage.
It does not reproduce Defender, Smart App Control, NTFS behavior, the MSVC
linker, or native Windows path-limit policy. The `Windows` GitHub Actions
workflow exercises real Windows/MSVC without requiring a local Windows PC.

## Browser security smoke tests

The root Node dependencies exist only for real-browser acceptance tests. They
are exact-pinned in [`pnpm-lock.yaml`](../pnpm-lock.yaml) and require Node.js 22
or newer. Every JavaScript project pins pnpm and checks in a pnpm lockfile.
The root install has a `preinstall` guard; CI also rejects competing lockfiles,
unpinned package managers, and package scripts using another manager in nested
projects. Enable pnpm with `corepack enable` before installing:

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

Source builds always download `proa-cli` from crates.io. They preserve the
selected published version, including when its binary archive is unavailable.
An unversioned `--build-from-source` / `-BuildFromSource` request installs the
latest published crate, as does a fallback when binary metadata is unavailable.
A failed pinned build stops rather than retrying another version. Main snapshots
require a prebuilt archive; choose a published version to build from source.

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

The CLI and reusable libraries support crates.io installation. Normal releases
use [`scripts/release-plz.sh`](release-plz.sh), both locally and through the
manually dispatched **Release crates** workflow. Both entry points use the
same pinned release-plz and cargo-semver-checks versions, configuration,
crates.io destination, policy checks, and archive/license checks. Ordinary
pushes and PR merges do not publish crates.
Changes to release tooling also run the same installer and `check` command
automatically in PR CI, without release secrets.

The installer selects a separate, pinned Rust toolchain to build the release
tools. This does not change `rust-toolchain.toml` or the toolchain used for
application builds and release preflights. All three pins live in the wrapper.

Install the tools once, then choose the same operation locally or in Actions:

```bash
scripts/release-plz.sh install
scripts/release-plz.sh check
```

| Operation | Local command | Effect |
| --- | --- | --- |
| `check` (workflow default) | `scripts/release-plz.sh check` | Check tool versions, publication policy, and archive/license boundaries; no remote changes |
| `release-pr` | `scripts/release-plz.sh release-pr` | Open or update the version/changelog PR |
| `release-dry-run` | `scripts/release-plz.sh release-dry-run` | Run release-plz publication checks without uploads or tags |
| `release` | `scripts/release-plz.sh release` | Publish pending versions to crates.io and create their tags/releases |

Live `release` publishes the libraries first with CLI publication disabled.
It then builds the candidate CLI and verifies all generated templates against
crates.io using a fresh Cargo home, without workspace dependency overrides.
Only after those checks pass and the source still matches current `main` does
it publish the CLI. Stable CLI binary releases run the same standalone check
before publishing their archives. Experimental main snapshots instead run
ORIGIN acceptance against workspace libraries, allowing snapshots of APIs
that have not been released to crates.io yet.

Registry resolution retries unavailable Proa/RSJS versions up to three times,
waiting 5 then 10 seconds for index propagation. Build, test, and HTTP failures
still stop the release. The workflow budgets include all five cold scaffold
builds: 120 minutes for CLI binaries and 180 minutes for the full crate release.

Use the [Proa release skill](../.agents/skills/proa-release/SKILL.md) for agent-led
releases. Run `release-pr` to prepare version changes; do not hand-bump package
versions, dependency requirements, or scaffold pins. Explicit overrides use the
wrapper’s `set-version` operation.

The shared preflight also runs `check_release_scaffold.py` with pinned release-plz.
In temporary fixtures, a library-only patch or minor change must bump the CLI,
update its inherited dev-dependency requirement, and appear in the scaffold
catalog generated by the real CLI build script. It makes no uploads or GitHub
changes.

The CLI derives generated dependency versions from its Cargo dev-dependency
declarations at build time. Workspace builds resolve inherited declarations;
packaged builds read Cargo’s normalized manifest. Normal version updates therefore
update both the library requirements and scaffold versions.

`release` publishes the versions prepared in package manifests; `release-pr`
is a separate version/changelog preparation step. Packages with source or
dependency changes need new versions, while unchanged published packages keep
their existing versions. Unpublished packages can retain their initial version.

`release --dry-run` is also accepted locally. A multi-crate publication dry run
can fail when a consumer needs an internal dependency version or feature that
is not published yet: the dry run never uploads the dependency for that consumer
to resolve. Prepare the version PR first and inspect the failure; the live
release publishes in dependency order and verifies each package. `check` is the
credential-free preflight; it does not rebuild every packaged crate.

To use Actions after this workflow is on `main`, open **Actions → Release
crates → Run workflow**, choose `main`, and select the operation. The equivalent
CLI trigger is:

```bash
gh workflow run release-plz.yml --ref main -f command=check
```

Every manual operation requires `main`; selecting another branch fails with
an error before checkout or tool installation. Each operation has its own
concurrency group, so a queued `release` is not replaced by a later `check`,
`release-pr`, or `release-dry-run`. GitHub keeps only the newest pending run
of the same operation; running manual operations are not cancelled.

Configure these repository secrets once:

| Actions secret | Local environment | Purpose |
| --- | --- | --- |
| `RELEASE_GITHUB_TOKEN` | `GIT_TOKEN` (or `GITHUB_TOKEN`) | A repository-scoped PAT or GitHub App token with Contents and Pull requests read/write access |
| `CARGO_REGISTRY_TOKEN` | `CARGO_REGISTRY_TOKEN` | crates.io publishing permission for the workspace packages; needed only for `release` |

Set Actions secrets with `gh secret set RELEASE_GITHUB_TOKEN` and
`gh secret set CARGO_REGISTRY_TOKEN`, which prompt for their values. Load local
tokens through your secret manager. The default Actions `GITHUB_TOKEN` cannot
replace `RELEASE_GITHUB_TOKEN`: its release PRs would not trigger CI and its
tags would not trigger the CLI binary workflow. See the
[release-plz token documentation](https://release-plz.dev/docs/github/token).
Private-repository metadata access also needs the GitHub token during dry runs.
The wrapper removes release-token environment variables during policy/archive
checks and tool installation. Locally, `CARGO_REGISTRIES_CRATES_IO_TOKEN` is
also accepted and normalized to `CARGO_REGISTRY_TOKEN` for publication.
Local publication requires one of these environment variables. The wrapper
does not use the token saved by `cargo login` in Cargo's credentials file;
load the publishing token from your secret manager into the environment.
This keeps credential selection explicit and consistent with Actions.

For a normal release:

1. Start from a clean checkout of current `main` and run `release-pr`.
2. Review and merge the release PR after its CI passes.
3. Update to current `main`, wait for CI, and inspect `release-dry-run` before running `release`.
4. Verify public installation with the registry checker below.

Local and CI live operations both reject dirty checkouts, a different origin
repository, and a revision that no longer matches GitHub's current `main`.
CI additionally supplies its expected checkout SHA. `release_always = true`
lets an explicit publication release pending versions even if another commit
has landed since the version PR; it does not enable automatic publication.
The wrapper rechecks the main revision after preflight, immediately before
the live operation. It does not provide a lock against a merge after that check.

For `proa-cli`, release-plz publishes to crates.io and creates the
`proa-cli-v<version>` tag. **Release CLI** then builds and verifies its binary
archives and owns the GitHub Release page. Other package GitHub Releases remain
owned by release-plz. The separate automatic `main` binary snapshots are
unchanged and do not publish crates to crates.io.

### Recovery and standalone verification

Use the same release-plz wrapper to retry a failed release. Review the failure
and current source before retrying; packages already published at their prepared
version are skipped. See [`docs/registry.md`](../docs/registry.md) for package
ownership and distribution policy.

After publishing, check a credential-free registry installation and all five
generated site templates, retaining artifacts for inspection:

```bash
python3 scripts/check_registry_install.py --work-dir /tmp/proa-registry-acceptance
```

The registry checker installs the latest stable CLI by default. Its optional
`--version` flag selects an exact release when reproducing a failure. Use
`--cli /path/to/proa` to check a candidate build (with `cargo-proa` alongside it),
or repeat `--template origin` to select individual templates. Both modes use
an isolated Cargo home and leave generated dependency declarations unchanged.
The work directory retains projects, logs, and target artifacts for inspection;
each invocation creates a fresh Cargo home, including its registry download cache.

The release wrapper runs policy and archive/license checks before publishing.
It does not discover credentials in other repositories or infrastructure services.
Demos and tools remain `publish = false`; release libraries inherit the
workspace's explicit crates.io destination policy.

## Rust validation in CI

Pull requests select changed Cargo packages and their transitive consumers,
including normal, build, dev, optional, and target-specific dependencies. All
files in a package count, including compile-fail fixtures and snapshots. The
source-linter’s cross-package fixture reads, docs icon drift check, relocation
fixture’s generated Cargo graph, and the quickstart document are included
explicitly. Unknown/shared inputs, manifest/lockfile changes, API
failures, and non-PR runs use full-workspace validation.

`scripts/rust_ci.py` prints the selection and runs the existing Clippy, library,
integration, feature-isolation, and converter-doctest checks for that scope.
Formatting and publication policy still run on every change. Regression tests:

```sh
python3 scripts/test_rust_ci.py
```

The Rust job retains Cargo registry and target artifacts across runs with keys
for runner platform, compiler identity, manifests, toolchain/configuration, and
commit, restoring the latest compatible build as a starting point. GitHub’s
branch cache isolation prevents trusted main jobs from consuming PR archives.
Source timestamps are restored only when tracked file contents and permissions
match the saved SHA-256 record, so unchanged workspace crates survive a fresh
checkout. Changed files keep their new timestamps; added/deleted inputs or
changed symlink targets disable timestamp reuse for that checkout. Cargo still validates
freshness and affected tests still execute; passing test
results are not cached. Nested trybuild/head-role targets and the relocation
fixture’s dependency target directory are retained too.

Cargo timing reports are uploaded as `rust-validation-timings`. Phase durations
include compilation performed inside tests; the first Cargo `Finished` line is
not a measurement of all compilation in that phase. A first cache fill is cold;
measure subsequent restores before reporting a CI speedup.
