# Licensing Proa

Proa is licensed under the **Apache License, Version 2.0**. See
[LICENSE](LICENSE) for the full text.

Copyright 2026 Proa Labs, Inc.

There is no separate commercial license and nothing to purchase. Apache 2.0
permits commercial use, modification, distribution, patent use, and private
use, including inside closed-source products, provided you keep the license
and copyright notice and state any significant changes you make to the files
you redistribute.

Every crate inherits `license-file = "LICENSE"` from the workspace, so Cargo
ships the single root [LICENSE](LICENSE) in each published archive. Source
files carry the following header, using the file format's comment syntax:

```text
Copyright Proa Labs, Inc.
SPDX-License-Identifier: Apache-2.0
```

Use the exact legal entity name `Proa Labs, Inc.` for first-party copyright
notices, including generator output and site footers. Source headers omit the
year; the root license retains its dated notice. Header changes do not change
the license of the code.

Run `python3 scripts/copyright_headers.py --check` before submitting changes,
or `--fix` to add missing headers and normalize known first-party notices. CI
runs the check. The tool preserves shebangs, Python encoding declarations,
Docker parser directives, line endings, and executable permissions, and
refuses to replace an unknown holder or license.

The check covers first-party source, scripts, and configuration in Git-visible
files. Source-installed components listed in `proa.lock.json`, third-party
bundles, generated assets, and compiler/editor fixtures retain their existing
terms and byte layout. First-party scaffold sources are checked alongside
their workspace copies. The script documents
additional path exclusions. First-party documentation, data, templates, and
fixtures without individual headers remain covered by the root license unless
separately licensed. Private `.local/` records are excluded from this tooling.

Third-party material remains subject to its separate license and attribution
requirements; see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

Licensing questions: [legal@proa.so](mailto:legal@proa.so).
