# Proa editor support

Proa adds Rust navigation, hover, completion, rename, and semantic tokens inside
single raw-string `md!` and `md_sync!` templates, including `r###"..."###`.
Loops and conditions preserve Rust bindings. Code spans, fenced code, and escaped
braces remain literal Markdown. Rust analysis comes from ordinary rust-analyzer.

## VS Code

Use the [Proa extension](vscode/README.md) with the official rust-analyzer
extension. It attaches to that extension's existing client and server. It does
not use the standalone bridge or change `rust-analyzer.server.path`.

## Live lint diagnostics

Both integrations run the existing Proa source linter after a 250 ms typing
pause. Unsaved Rust buffers override disk contents, and content checks discover
their package's other views even when those files are not open. Adding a child
to HTML can therefore flag its absence in Markdown immediately. Squiggles and
the Problems/diagnostics list include the lint code, repair guidance, and a link
to the contribution in the other view. Fixing or deliberately omitting the
contribution clears the diagnostic. No application compilation or source writes
are involved. The usual formatting, accessibility, and performance lints also
run on open files.

Install a Proa CLI built from this checkout; older binaries do not have the
worker protocol:

```sh
cargo install --path proa_cli --bin proa --locked --force
```

VS Code starts `proa` from PATH. Set `proa.lint.path` to an absolute executable
path if needed, or `proa.lint.enable` to `false` to disable live lints. For
example, when developing Proa, build with `cargo build -p proa-cli --bin proa`
and point the setting at `/absolute/path/to/proa/target/debug/proa`.

The standalone bridge accepts `--proa /absolute/path/to/proa`, or the `PROA_LINT`
environment variable, and `--no-lint` disables live lints. A missing or outdated
CLI is reported in VS Code's Proa output channel (and a warning) or the bridge's
stderr; Rust navigation continues. Restart the bridge, reload VS Code, or change
its lint settings after repairing the executable.

The editor keeps one lint worker alive using `proa lint --stdio` and coalesces
edits while a check is running. A result is discarded if any buffer or watched
source changes during analysis. File watchers refresh closed counterparts after
external edits, additions, or deletions. Closing an unsaved buffer restores disk
contents for subsequent checks. Diagnostics from rust-analyzer remain visible
alongside Proa's own diagnostics.

Build trees such as `target`, `node_modules`, and `dist` are excluded before
file watches are installed. If a lint check times out, its worker is stopped
and the next edit starts a fresh worker; no editor restart is required.

This uses the same bounded HTML → Markdown policy as `proa lint`; it does not
invent a rendering for new children or require single-target components to gain
another view. Incomplete Rust syntax produces a syntax diagnostic and pauses
content comparisons for that package until it is parseable again. Ordinary Rust
type checking and compiler diagnostics remain rust-analyzer/Cargo's job. See
the [content coverage guide](../docs/agent/content-coverage.md) for supported
analysis, intentional omissions, and registering other render targets.

## Install the standalone bridge

For other LSP editors, `proa-lsp` launches one rust-analyzer process and adapts
its input and output. It uses the same projection and edit checks as VS Code.
Requires Node.js 22+, rust-analyzer, and the project's Rust toolchain (including
Cargo and rustfmt).

From this repository's root:

```sh
rustup component add rust-analyzer rustfmt
pnpm add --global ./editors/lsp
proa-lsp --version
```

The language-server package has no runtime dependencies and can be installed from this
checkout; these instructions do not depend on an npm registry release. To avoid
a global installation, use `node /absolute/path/to/proa/editors/lsp/bin/proa-lsp.mjs`
as the editor's command and arguments.

By default the bridge runs `rust-analyzer` from `PATH`. To use a specific binary:

```sh
proa-lsp --rust-analyzer /absolute/path/to/rust-analyzer
```

`PROA_RUST_ANALYZER` sets the same default. Arguments following `--` are passed
to rust-analyzer. Set the analyzer to the actual executable, not another copy
of the bridge. Node, Cargo, and rustfmt must also be available to the editor
process, including in SSH sessions and containers.

Configure the bridge as the editor's Rust server command. Avoid enabling a
second, direct rust-analyzer client for the same buffers: it would analyze the
unprojected raw strings and duplicate ordinary Rust features.

## Neovim 0.11+

If you already configure `rust_analyzer` through nvim-lspconfig, change only its
command, preserving your settings, capabilities, completion plugin, and keymaps:

```lua
vim.lsp.config('rust_analyzer', {
  cmd = { 'proa-lsp', '--stdio' },
})
vim.lsp.enable('rust_analyzer')
```

For a setup without an existing Rust LSP configuration, copy
[`neovim/lsp/proa.lua`](neovim/lsp/proa.lua) to `~/.config/nvim/lsp/proa.lua` and
enable it in `init.lua`:

```lua
vim.lsp.enable('proa')
```

Use one of these configurations. `:checkhealth vim.lsp` should show one attached
Rust client. `K`, `vim.lsp.buf.definition()`, `vim.lsp.buf.rename()`, and
`vim.lsp.buf.format()` use the normal Neovim LSP APIs. For example, to bind `gd`:

```lua
vim.api.nvim_create_autocmd('LspAttach', {
  callback = function(event)
    if vim.bo[event.buf].filetype == 'rust' then
      vim.keymap.set('n', 'gd', vim.lsp.buf.definition, { buffer = event.buf })
    end
  end,
})
```

See [Neovim's LSP documentation](https://neovim.io/doc/user/lsp/).

## Vim with vim-lsp

Install [vim-lsp](https://github.com/prabirshrestha/vim-lsp), then source
[`vim/proa.vim`](vim/proa.vim) from your vimrc. It registers `proa-lsp` for Rust.
Use vim-lsp's usual commands and mappings, including `:LspDefinition`,
`:LspHover`, `:LspRename`, and `:LspDocumentFormat`. Disable any existing direct
Rust server registration. Completion and semantic coloring depend on the Vim
client and plugins you enable.

## Helix

Set the Rust server command in `~/.config/helix/languages.toml`:

```toml
[language-server.rust-analyzer]
command = "proa-lsp"
args = ["--stdio"]
```

This preserves Helix's existing Rust language/server association and settings.
See [Helix language-server configuration](https://docs.helix-editor.com/languages.html#language-server-configuration).

## Zed

Set the command for Zed's existing Rust server in `settings.json`:

```json
{
  "lsp": {
    "rust-analyzer": {
      "binary": {
        "path": "/absolute/path/to/proa-lsp",
        "arguments": ["--stdio"]
      }
    }
  }
}
```

Use `command -v proa-lsp` to find the absolute path. See
[Zed's Rust binary settings](https://zed.dev/docs/languages/rust#binary).

## Formatting and edit safety

Format Document runs rustfmt against the original unsaved text, using the
owning Cargo target's edition and nearest rustfmt configuration. The bridge
requests `rust-analyzer.rustfmt` settings from the client, including `extraArgs`
and `overrideCommand`; clients without scoped configuration use initialization
options or `workspace/didChangeConfiguration` settings. A changed or closed
buffer invalidates a pending formatting result. Range and on-type formatting
are suppressed in projected files. `proa fmt` additionally normalizes Markdown
template indentation.

Analysis projections stay in memory. The bridge never writes them to source
files. Renames, completion edits, code actions, file-operation edits, and
server-initiated workspace edits are checked before forwarding; edits spanning
literal Markdown are rejected. Ordinary Rust requests pass through.

The bridge negotiates UTF-16 LSP positions, which preserve source coordinates
through the projection, including emoji and non-ASCII text. Clients that
explicitly exclude UTF-16 receive an initialization error.

## Scope and validation

The real-editor integration tests cover VS Code and Neovim. Vim, Helix, and Zed
configurations use their documented LSP launch interfaces; they have not yet
received equivalent end-to-end tests here. Feature presentation, including
semantic highlighting, depends on each client's capabilities.

Unclosed template delimiters, cooked strings, and named-argument templates
fall back to native Rust analysis. References inside **unopened** raw templates
remain limited to rust-analyzer's native analysis. Open those files before
workspace renames and review the edits; the transport does not provide a
workspace index of embedded expressions. Markdown prose retains the editor's
string styling; this release does not add a Markdown Tree-sitter grammar.

Run from the repository root:

```sh
pnpm run editor:test
CARGO_TARGET_DIR=target cargo test --manifest-path editors/vscode/test/fixture/Cargo.toml
pnpm run editor:test:neovim
```

For live lint integration tests, build the CLI with
`cargo build -p proa-cli --bin proa`, then run
`pnpm -C editors/lsp test:lint` and `pnpm -C editors/vscode test:live`.
The first exercises the real persistent CLI and standalone LSP transport. The
second uses a disposable project and isolated VS Code profile; it requires
`code` and an installed official rust-analyzer extension. Both verify unsaved
changes and closed counterpart diagnostics without changing project source on
disk. `PROA_TEST_LINT` can select another built Proa executable.

The Neovim test uses an isolated configuration and the real analyzer. It tests
definitions, hover, references, semantic tokens, rename, incomplete-expression
completion, Unicode, unsaved edits, restart, and formatting. It restores buffer
contents and checks that the fixture on disk is unchanged. Set `NVIM` and
`PROA_RUST_ANALYZER` to override test executables. Failures retain logs in the
printed temporary directory. Use `PROA_TEST_LOG_LEVEL=debug` for LSP message logs.

## Shared source and packaging

The maintained projection, protocol, formatting, and live lint modules live in
[`lsp/src/`](lsp/src/). VS Code's `prepare`, `test`, and `vscode:prepublish`
scripts copy them into an ignored `src/core/` directory inside the extension.
The VSIX is self-contained; it does not import files from elsewhere in the
checkout. Edit the shared sources, then run `pnpm run editor:test`.

```sh
# From editors/lsp: creates an installable npm tarball.
pnpm pack

# From editors/vscode: includes the refreshed shared core.
pnpm dlx @vscode/vsce package --no-dependencies
```
