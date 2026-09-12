# Open-source repository boundary

Proa is a standalone source workspace. A fresh clone
must be understandable, resolvable, buildable, testable, and documentable
without a sibling checkout.

## Dependency rule

Dependencies flow out from product repositories into Proa:

```text
downstream products and component libraries
                    |
                    v
          published Proa crates
```

Proa must not depend on a sibling product repository, its source tree, build
artifacts, test fixtures, or unpublished implementation crates. Local
development may use Cargo patches outside the repository, but committed
manifests, source includes, scripts, workflows, tests, and documentation must
not assume that layout.

In particular:

- no committed dependency path may escape the repository;
- no source file may `include_str!` or `include_bytes!` from another checkout;
- public commands must work from this checkout alone;
- product-specific tests, release tooling, RFCs, and authoring instructions
  live with the product that owns them;
- generic extension points belong in Proa and use product-neutral names.

## Public terminology

- **Compiled templates** are ordinary Rust-authored `html_sync!`, `html!`,
  `md_sync!`, and `md!` templates compiled with the application.
- **Runtime templates** are template source supplied as data and validated when
  it is saved.
- A **template program** is the validated, disposable cache artifact produced
  from a runtime template.
- A **template interpreter** is the bounded renderer for a template program.
- **Runtime documents** are Markdown or HTML documents compiled when saved.
- A **component catalog** is the allowlist of compiled components a runtime
  template may invoke.

“Page VM,” “Document VM,” and bare “VM” are implementation terms, not
user-facing product concepts. Public Proa APIs and diagnostics must not require
readers to know those names.

## Documentation boundary

The tracked `docs/` directory contains current contributor guides and public
repository policies. Website documentation is maintained separately under
`proa_marketing/src/docs/`.

Working RFCs, roadmaps, implementation plans, operational runbooks, audit notes,
and superseded guides belong in private storage. Local records may be kept in
`.local/knowledge-base/`. Git and Docker ignore `.local/`; the repository
boundary check also rejects tracked local-only files, symlinks to them, and
Cargo dependencies, Rust includes, or Markdown links that target them.

A public guide must stand on its own in a fresh checkout. Review and rewrite
internal records before publication, verifying supported behavior against the
source and tests. Removing a file from the current tree does not remove it
from earlier Git history.

## Enforcement

A directory containing only this repository must pass:

```bash
cargo metadata --format-version 1 --locked
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo doc --workspace --no-deps --locked
```

[`scripts/check_repository_boundary.py`](../scripts/check_repository_boundary.py)
and the workflow-lint CI job reject:

- tracked local-only content or public dependencies on `.local/`;
- Cargo paths that resolve outside the repository;
- committed references to a sibling `proa-ui` checkout;
- absolute developer-machine paths;
- literal Rust source includes that escape the repository or target missing
  files;
- local links in Markdown and MDX that point to missing repository files;
- unsupported CLI binary-download URLs;
- an unlocked root Node dependency graph.

The same CI job runs [`scripts/copyright_headers.py`](../scripts/copyright_headers.py)
to require consistent first-party copyright and SPDX headers. Its `--fix` mode
normalizes known first-party notices while preserving separately licensed
material and exact-output fixtures; see [`LICENSING.md`](../LICENSING.md).

[`scripts/check_crate_archives.py`](../scripts/check_crate_archives.py) applies
the same boundary to each publishable Cargo archive and requires a top-level
copy of the canonical Apache-2.0 license, plus public repository/homepage metadata
and a crate-specific description. Real-browser streaming and CSP acceptance
use the checked-in pnpm lockfile and run in the browser-smoke CI lane.

[`scripts/check_asset_provenance.py`](../scripts/check_asset_provenance.py)
requires redistributed binaries and captured-fixture-shaped files to appear in
the public provenance manifest, and rejects retired third-party fixture
origins.
