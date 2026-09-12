# Package distribution

Proa currently distributes its CLI through GitHub releases and Git source
installation. Applications use Proa crates from the same Git repository;
third-party dependencies resolve from crates.io. No private Cargo registry,
index variable, or Cargo registry token is required to install or build Proa.

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true \
  cargo install --git https://github.com/proa-labs/proa --locked proa-cli
```

While the repository is private, Git needs a credential with repository read
access. The Git CLI option uses your configured Git credential helper.
For reproducible installs, add `--rev <full-commit-sha>`. Generated projects
use one canonical Git URL, `https://github.com/Proa-Labs/proa.git`; commit their
`Cargo.lock` and use the same revision for every Proa dependency. Existing
projects pinned to earlier commits need a dependency update to remove the old
registry metadata. See the [migration guide](migrations/0.3-standalone.md).

## crates.io publication

The Proa CLI and framework crates are not currently published on crates.io.
Do not use `cargo install proa-cli` or version-only Proa dependencies yet.
Selected library manifests restrict future publication to `crates-io`;
this allowlist is a destination policy, not a claim that releases exist.
The CLI, demos, and repository-only tools keep `publish = false`.

Before publishing, verify ownership of every package name and review the
packaged contents. Some dependency names, including `const_json` and
`tailwind-merge`, already exist on crates.io under other projects. Resolve
ownership or rename those packages before attempting a release. Never replace
a Git dependency with an unrelated package just because its name matches.

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
It does not upload, verify name ownership, or prove that unpublished dependency
archives can build on crates.io. Actual publication requires a separate release
review and `CARGO_REGISTRY_TOKEN`. No automatic Cargo publishing workflow is
enabled. See [`scripts/README.md`](../scripts/README.md) for manual tooling.

## Component catalogs

`proa registry` and `proa ui add` refer to HTTP component catalogs, not Cargo
registries. Free components need no token; optional Pro components use
`PROA_PRO_TOKEN`. Existing Pro catalog credentials remain supported during
migration. See the [component catalog protocol](../proa_cli/docs/ui-registry-v1.md).
The old Cargo-only `proa auth` helper has been retired.

Never commit credentials. Private release runbooks belong in the release
owner's knowledge base. Report vulnerabilities through [`SECURITY.md`](../SECURITY.md).
