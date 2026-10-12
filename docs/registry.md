# Package distribution

## crates.io release

The initial crates.io release is being prepared. Once publication completes,
install the CLI from crates.io:

```bash
cargo install proa-cli --version 0.1.1 --locked
```

For compiled HTML and Markdown components, use the facade:

```toml
[dependencies]
proa = "0.3.0"
```

The default facade feature re-exports rendering macros alongside core traits.
Enable `html-to-markdown` for runtime conversion or `axum` for the framework
adapter. Explicit dependencies such as `proa_core`, `proa_macros`, and
`proa_framework_axum` remain supported. Demos and benchmark applications stay
repository-only (`publish = false`).

Proa's support packages use distinct crates.io names so they cannot resolve to
unrelated packages. Existing import names can be retained with Cargo aliases:

```toml
const_json = { package = "proa_const_json", version = "0.1.2" }
const_json_macros = { package = "proa_const_json_macros", version = "0.1.2" }
tailwind-merge = { package = "proa_tailwind_merge", version = "0.1.2" }
tailwind-merge-macro = { package = "proa_tailwind_merge_macro", version = "0.1.2" }
```

## Release validation and publication

Publishable packages inherit the workspace's crates.io destination, license,
and minimum Rust version. Internal runtime dependencies have matching registry
versions. Internal test-only dependencies use paths without versions: Cargo
omits those local workspace tests from normalized registry manifests, avoiding
cycles during the first publication. The workspace still runs those tests.

```bash
python3 scripts/check_repository_boundary.py
python3 scripts/check_publish_policy.py
python3 scripts/check_crate_archives.py
python3 scripts/publish.py --dry-run
```

The script's dry run checks policy and archive boundaries and prints dependency
order. It does not upload or rebuild packages. Cargo can verify the complete
release set before upload by packaging all publishable packages together:

```bash
cargo package --workspace
```

Supply `CARGO_REGISTRY_TOKEN` from the intended publisher's secret manager, then
run the verified publishing flow:

```bash
python3 scripts/publish.py --skip-published
```

The default flow verifies each archive, publishes dependencies first, and waits
for each version to resolve before publishing its dependents. See
[`scripts/README.md`](../scripts/README.md). Publication is manual; no automatic
Cargo publishing workflow is enabled. Account ownership is represented by
crates.io/GitHub usernames rather than email addresses.

## Source installation

GitHub binary releases and public Git source installation remain supported:

```bash
cargo install --git https://github.com/proa-labs/proa --locked proa-cli
```

For reproducible source installs, add `--rev <full-commit-sha>`. Keep all Git
Proa dependencies on the same revision and commit the application's Cargo.lock.
No private Cargo registry is required to build or install Proa.

## Component catalogs

`proa registry` and `proa ui add` refer to HTTP component catalogs, not Cargo
registries. Free components need no token; optional Pro components use
`PROA_PRO_TOKEN`. Existing Pro catalog credentials remain supported during
migration. See the [component catalog protocol](../proa_cli/docs/ui-registry-v1.md).
The old Cargo-only `proa auth` helper has been retired.

Never commit credentials. Private release runbooks belong in the release
owner's knowledge base. Report vulnerabilities through [`SECURITY.md`](../SECURITY.md).
