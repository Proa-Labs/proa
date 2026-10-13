# Package distribution

Install the CLI and framework libraries from crates.io. No private Cargo
registry, GitHub credential, or registry token is required to install or build
Proa.

```bash
cargo install proa-cli --locked
```

Cargo installs both `proa` and `cargo-proa`. Generated projects use compatible
crates.io versions and commit their `Cargo.lock` for reproducible builds.
Git source installation remains available for unreleased revisions:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true \
  cargo install --git https://github.com/proa-labs/proa --locked proa-cli
```

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
python3 scripts/publish.py --dry-run
```

The dry run checks policy and archive boundaries and prints dependency order.
It does not upload or prove that dependent archives build against the registry.
Actual publication verifies each archive and waits for registry visibility
before proceeding. Forward development dependencies are path-only during the
initial bootstrap, so Cargo can publish production dependencies in order without
a cycle through test-only crates. Workspace tests retain those dependencies.
Configure a current crates.io publishing token locally; never commit it. See
[`scripts/README.md`](../scripts/README.md) for manual tooling.

## Component catalogs

`proa registry` and `proa ui add` refer to HTTP component catalogs, not Cargo
registries. Free components need no token; optional Pro components use
`PROA_PRO_TOKEN`. Existing Pro catalog credentials remain supported during
migration. See the [component catalog protocol](../proa_cli/docs/ui-registry-v1.md).
The old Cargo-only `proa auth` helper has been retired.

Never commit credentials. Private release runbooks belong in the release
owner's knowledge base. Report vulnerabilities through [`SECURITY.md`](../SECURITY.md).
