# Proa UI licensing

This repository contains software offered under different license terms.
Access to the repository, a private Cargo package, or a Proa UI Pro download
does not by itself change those terms.

## The mixed `proa_ui` crate is proprietary

The `proa_ui` Cargo package currently contains both Free and Pro components.
The package as a whole remains private and proprietary, as declared by its
`LicenseRef-Proprietary` Cargo metadata. It is not distributed as an
MIT-licensed or Apache-licensed crate.

The packaged crate preserves the same path boundary as this repository.
Inside an extracted `proa_ui` Cargo package, interpret an allowlisted path
beginning with `proa_ui/` as the corresponding package-relative path after
removing that prefix. For example,
`proa_ui/src/components/button/button.rs` identifies
`src/components/button/button.rs` in the package.

## Free source-install grant

The repository-relative source paths selected by
[`licensing/free-source-allowlist.toml`](licensing/free-source-allowlist.toml)
are available, at your option, under either:

- the MIT License in [`LICENSE-MIT`](LICENSE-MIT), or
- the Apache License, Version 2.0, in
  [`LICENSE-APACHE`](LICENSE-APACHE).

For a component directory in `free_component_directories`, the grant covers
every file recursively within that directory. It also covers the exact
`shared_support_files` and the files recursively contained in
`shared_support_directories`. These are the files that the Proa CLI may place
in a Free source-install payload. Free interactive components use the
allowlisted `runtime/proa-free.js` and `runtime/proa-free.css` assets. The
mixed `runtime/proa.js` and `runtime/proa.css` assets also contain Pro
component behavior and styles, so they remain proprietary and must never enter
a Free payload. Delivering those mixed assets to a browser does not change
their license.

Free catalog block source is reviewed separately in the component library's
`licensing/free-block-source-allowlist.toml`, which lives in that library's own
repository and is not part of a source install.
Each exact file listed in `free_block_files` is available under the same MIT OR
Apache-2.0 choice for Proa's contribution and carries that SPDX grant in the
file itself. Adapted portions remain subject to their recorded upstream MIT
terms. The block allowlist does not relicense other marketing code, and it is
not part of a component-only CLI install payload.

The machine-readable allowlist is intentionally narrower than the mixed Cargo
crate. A file is not Free merely because it is imported by, linked with,
stored beside, or distributed in the same repository as a Free component.
Some allowlisted files incorporate third-party MIT- or ISC-licensed material.
Those portions remain subject to their original terms and attribution
requirements; choosing Apache-2.0 for Proa's contribution does not remove
those obligations.

## Proa framework licensing is separate

The MIT OR Apache-2.0 grant above applies only to the expressly allowlisted
Proa UI files. It does not relicense Proa framework crates or other
dependencies. Proa framework code is offered separately under
`AGPL-3.0-only` or under an applicable commercial Proa license. A recipient
using the Free component source with Proa must independently comply with the
AGPL-3.0-only terms or obtain the commercial rights its use requires.

## Pro and other repository code

All files not expressly selected by the Free source-install allowlist remain
proprietary unless the file carries a separate license notice. This includes
the Pro component directories, the full mixed browser runtime, the marketing
application, the console, and the mixed crate entry points and catalogs.

The names, logos, and other Proa trademarks are not licensed by the Free
source licenses except as those licenses expressly provide.

No commercial license is granted by this file, marketing copy, an order
summary, package access, or payment alone. Commercial rights arise only under
the applicable Commercial License Agreement accepted or executed by the
customer, and remain subject to that agreement.

## Distribution requirements

Every Free source-install payload must preserve repository-relative paths and
include:

- `LICENSE`
- `LICENSE-MIT`
- `LICENSE-APACHE`
- `LICENSING.md`
- `THIRD_PARTY_NOTICES.md`
- `licensing/free-source-allowlist.toml`

Third-party code remains subject to its own terms. The notices file must
travel with the payload regardless of which licensing option the recipient
chooses.

Repository maintainers must validate a staged payload before distribution.
From a Proa UI repository checkout, run:

```sh
python3 scripts/stage_free_source_distribution.py \
  --all \
  --output /path/to/new-payload
```

The stager computes Free component dependencies and runs the payload validator
before returning success. The validator rejects Pro component paths, unlisted
support code, symlinks, missing legal files, and files that differ from this
repository. CI stages and validates the complete Free payload in addition to
checking that the allowlist stays synchronized with the component availability
catalog. The private Cargo package carries this policy but does not package
repository-maintenance scripts.
