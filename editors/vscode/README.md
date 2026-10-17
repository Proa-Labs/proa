# Proa for VS Code

The `proa-labs.proa` extension includes Markdown template support. Users need
one Proa extension alongside the official rust-analyzer extension.

For Neovim, Vim, Helix, and Zed, use the standalone bridge described in the
[editor setup guide](../README.md). Both integrations share the same template
projection and edit checks.

Rust highlighting, hover, completion, and go-to-definition inside inline
`md!` and `md_sync!` raw templates:

```rust
md_sync! { r###"
**{a.user_name.as_str()}** · via {client}

{a.note.as_str()}
"### }
```

This extension uses the **existing rust-analyzer extension and server**. It
does not launch a language server, run a proxy process, or change
`rust-analyzer.server.path`.

## Installation

Install the VSIX with VS Code's **Extensions: Install from VSIX…** command.
The official `rust-lang.rust-analyzer` extension is required. Reload the window
after installation; no workspace settings are necessary.

If you installed the earlier `proa-labs.proa-markdown` prototype, uninstall it
before installing Proa so that only one extension handles template analysis:

```sh
code --uninstall-extension proa-labs.proa-markdown
code --install-extension proa-0.2.0.vsix
```

`proa new site` already recommends `proa-labs.proa` in the generated
`.vscode/extensions.json`. The VSIX is currently packaged locally; publishing
this extension to the Marketplace is still required for new users to install
it through that recommendation.

To package from this directory:

```sh
pnpm dlx @vscode/vsce package --no-dependencies
```

Packaging refreshes the shared modules from `../lsp/src` into the VSIX. For
extension development, run `pnpm test` first to refresh those modules locally.

## How it works

The extension attaches middleware to rust-analyzer's exported `LanguageClient`.
For analysis, it replaces literal Markdown with spaces and leaves each
`{expression}` as Proa's existing Rust token-composition syntax. The analyzer
therefore sees real Rust expressions in their original scope.

Template `for`, `if`, `else if`, `else`, and `if let` blocks preserve their Rust
bindings. Their Markdown bodies become Rust expression statements for analysis.
Cargo still checks the original templates and their render-trait requirements.

While a field access is unfinished (`{a.}`), the projection temporarily exposes
an ordinary Rust block so completion can work without a successful procedural
macro expansion. Completing the expression restores normal macro analysis.

Every UTF-16 position and line break is preserved. Files on disk and text in the
editor keep the original Markdown. The running analyzer handles types and
navigation normally; the extension provides no separate type checker.

## Current scope

- Single raw-string `md!` / `md_sync!` templates, including qualified macro paths.
- Nested Rust expressions, Unicode, raw strings, comments, and character literals.
- Template loops and conditions, including nested blocks and `if let` bindings.
- Rust field completion while typing an unfinished access such as `{a.}`.
- Braces in Markdown code spans, fenced code, and escaped braces stay literal.
- Token-composition syntax and unrelated Rust are passed through unchanged.
- Templates with unclosed delimiters, cooked strings, and named-argument
  templates are left to native analysis until a supported template is available.

Format Document runs rustfmt on the original unsaved text, using the owning
Cargo target's edition and nearest rustfmt configuration. It respects
`rust-analyzer.rustfmt.extraArgs` and `rust-analyzer.rustfmt.overrideCommand`.
An edit made while formatting is running causes the stale formatting result to
be discarded. Use `proa fmt` to additionally normalize template indentation.
Range and on-type formatting are suppressed in projected files.

Renames, completion edits, and code actions are checked before use; edits spanning
literal Markdown are rejected. Workspace-wide navigation into raw templates in
unopened files remains limited to rust-analyzer's native analysis. Open those
files before renaming symbols used in their templates, and review rename results.

The extension requires a trusted workspace, the official rust-analyzer extension,
and the Rust toolchain used by the project. It supports raw templates on both
Rust editions 2021 and 2024. Markdown prose retains the editor's string styling;
Rust expressions receive rust-analyzer's semantic colors.

## Validation

```sh
pnpm test
CARGO_TARGET_DIR=../../target cargo test --manifest-path test/fixture/Cargo.toml
```

`test/vscode.cjs` is an extension-host integration test. Set
`PROA_EDITOR_TEST_FILE` to `test/fixture/src/lib.rs`, or to the
Incident Desk's `issue_report.rs`, and launch VS Code with this extension as
`--extensionDevelopmentPath` and the test as `--extensionTestsPath`. It exercises
VS Code's actual definition, hover, rename, formatting, and semantic-token
providers. Set `PROA_EDITOR_TEST_MUTATION=1` for the disposable fixture to also
test unsaved edits, incomplete-expression completion, and server restart. It
restores the editor text and verifies that the file on disk did not change.

Use an isolated `--user-data-dir` for the integration test. The fixture uses
Rust 2024 and covers nested control flow and code fences. The actual MCP example
provides the Rust 2021 check. No custom rust-analyzer server is used.
