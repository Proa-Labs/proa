# Contributing To Proa

Proa is licensed under the Apache License, Version 2.0; see `LICENSE` and
`LICENSING.md`.

Contributions are accepted under the same terms. Apache 2.0 section 5 already
provides that any contribution you intentionally submit for inclusion is
licensed to Proa Labs under those terms, unless you state otherwise in
writing, so no separate contributor agreement is required.

First-party source files use this header (adapt the comment syntax to the file):

```rust
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
```

Run `python3 scripts/copyright_headers.py --fix` to normalize headers, then
`python3 scripts/copyright_headers.py --check` to verify them. The check also
runs in CI. Preserve upstream notices and separately licensed source-installed
components; see [`LICENSING.md`](LICENSING.md) for coverage and exclusions.

## Local Setup

Use the Rust toolchain pinned by `rust-toolchain.toml`.

```bash
cargo check -p proa_core
cargo test -p proa_core --lib
```

For the marketing/docs app:

```bash
cargo run -p proa_marketing
```

This checkout is a standalone workspace. The `proa_ui` component library and
the runtime-template/document system are downstream integrations with their own
repositories and contributor instructions; they are not prerequisites for
working on Proa.

## Before Sending Changes

Run focused checks for the crates you touched:

```bash
cargo fmt --all --check
cargo check -p <crate>
cargo test -p <crate> --lib
python3 scripts/copyright_headers.py --check
python3 scripts/check_repository_boundary.py
python3 scripts/check_asset_provenance.py
python3 scripts/check_publish_policy.py
python3 scripts/check_crate_archives.py
```

If you changed render paths, run:

```bash
python3 scripts/proa_perf_lint.py
```

The perf lint intentionally guards hot render paths. Prefer stack-allocated
class lists, static strings, and existing `WriteBuf` APIs. Use
`// proa-perf-ok: reason` only when the allocation is outside the hot path or is
otherwise intentional.

Changes to streamed boundary framing, CSP propagation, or React integration
also require the locked real-browser checks:

```bash
npm ci --ignore-scripts
npx playwright install chromium
npm run stream:boundary-smoke
npm run react:csp-smoke
```

## AI-Assisted And Generated Changes

AI-assisted contributions are allowed, but assistance does not change who is
accountable for a change. The person submitting a change must understand and
review it, be able to explain its design and failure modes, and run the same
tests and security checks expected for hand-written code. A human maintainer
reviews the change before it is accepted; generated output is not accepted on
the strength of a tool's review alone.

In the change description, disclose material AI or code-generation assistance
and identify the affected areas. Include enough provenance for reviewers to
reconstruct consequential inputs, such as the upstream implementation,
specification, issue, or benchmark fixture that informed the result. Prompt
transcripts and a tool inventory are not required. Never provide assistants
with credentials, private vulnerability reports, personal data, or source that
you are not authorized to share.

Generated tests are supporting evidence, not an oracle. Review expected values
against the governing Rust/browser semantics or public specification. For
parser, macro, transport, unsafe, authentication, or HTML-context changes, add
focused adversarial regressions and describe the trust boundary being changed.
Report security findings through `SECURITY.md`, including when a tool found
them.

Competitive benchmark ports have additional provenance requirements:

- pin and link the exact upstream version and source fixture;
- preserve the competitor's supported production mode, and call out every
  adaptation;
- prove byte-identical or normalized-DOM-equivalent output before timing;
- separate cached work, full render work, fresh ownership, and buffer reuse;
- match relevant hydration or island behavior, or exclude the row;
- retain confidence intervals and the complete same-machine result set rather
  than selecting favorable samples.

If those controls cannot be met, keep the result as a clearly labeled
directional experiment rather than a product claim.

Do not commit captured page HTML, screenshots, photographs, logos, fonts, or
API responses without an explicit redistribution grant. Prefer synthetic
fixtures and record every retained visual or captured artifact in
[`docs/asset-provenance.md`](docs/asset-provenance.md).

## Public API Expectations

- Keep stable examples focused on `proa_core`, `proa_macros`,
  `proa_framework_axum`, and `rsjs`.
- Mark experimental APIs explicitly in docs and module names.
- Avoid adding new public crates until the launch surface is finalized.
- Do not change license metadata in drive-by cleanup commits.

## Documentation

When adding public behavior, update the nearest public doc:

- root `README.md`
- `docs/README.md`
- `docs/agent/`
- crate-level `README.md`
- marketing docs under `proa_marketing/src/docs/`

Keep working RFCs, roadmaps, implementation plans, operational runbooks, audit
notes, and superseded guides in private storage. Local notes belong under the
Git-ignored `.local/knowledge-base/`, which is excluded from Docker build
contexts. Public docs and build inputs must not depend on that directory.
Review and rewrite internal material before publishing it as current guidance;
see [`docs/README.md`](docs/README.md) for the documentation boundary.
