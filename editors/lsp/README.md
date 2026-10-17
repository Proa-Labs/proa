# Proa language-server bridge

`proa-lsp` provides Rust editor features inside Proa's raw-string `md!` and
`md_sync!` templates by adapting a normal rust-analyzer LSP connection. It
shares its template parser, formatting implementation, and edit guards with
the Proa VS Code extension.

Requires Node.js 22+, a working rust-analyzer executable, and the project's
Rust toolchain with Cargo and rustfmt. There are no npm runtime dependencies.

From a Proa checkout:

```sh
rustup component add rust-analyzer rustfmt
pnpm add --global ./editors/lsp
proa-lsp --version
```

Configure your editor's Rust language-server command as `proa-lsp --stdio`.
The bridge starts one rust-analyzer process. To select a different analyzer,
use `--rust-analyzer /absolute/path/to/rust-analyzer` or `PROA_RUST_ANALYZER`.
Arguments following `--` are forwarded to the analyzer. LSP traffic uses
stdin/stdout; logs use stderr. Restart through your editor's usual LSP controls.

Formatting uses the original unsaved text and rust-analyzer's rustfmt settings.
Analysis projections never overwrite source files. Edits that would overwrite
literal Markdown are rejected. UTF-16 positions are required and negotiated.

Support is limited to single raw-string templates with closed delimiters.
Workspace references inside unopened raw templates remain limited; open those
files before renaming and review the results. Semantic highlighting depends on
the editor's LSP support. Rich Markdown syntax highlighting is separate.

See the [editor setup and validation guide](https://github.com/Proa-Labs/proa/blob/main/editors/README.md)
for Neovim, Vim, Helix, Zed, and VS Code configurations.
