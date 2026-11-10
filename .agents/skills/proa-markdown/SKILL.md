---
name: proa-markdown
description: Author typed Markdown renderers and shared HTML/Markdown page content in Proa. Use for md_sync!, MdRenderSync, Markdown exports, or keeping agent-readable page representations in sync with HTML.
---

# Proa Markdown

Read [document rendering](../../../docs/agent/document-rendering.md) and the
[render trait reference](../../../docs/agent/api-reference/render-traits.md)
in the source checkout. In an application without those files, retrieve the
corresponding current documentation from `https://proa.so/docs/llms.txt`.

`md!` and `md_sync!` require the default-enabled `markdown` feature of
`proa_macros`. If the dependency disables defaults, enable `features = ["markdown"]`.
HTML/XML-only crates may disable it to omit Markdown parser build dependencies.

## Choose the rendering boundary

When the same Page also renders a managed-head HTML document, declare
`const ROLE: proa::ComponentRole = proa::ComponentRole::PAGE;` on its
HTML render-trait impl, even if it inherits all metadata. Ancestor HTML head
providers declare `ComponentRole::LAYOUT`. Run `cargo proa lint <changed-paths>
--deny warnings` to reject hooks with missing or explicit `COMPONENT` roles.
Native async hooks rely on this source check; sync hooks also retain a
code-generation assertion, so verify them with `cargo build`.
Markdown render traits
do not have this role constant. Follow the
[managed Page boundary](../../../docs/agent/components.md#managed-html-page-boundary)
for HTML lifecycle and route verification.

Use `MdRenderSync` with `md_sync!` for synchronous content. Use `MdRender` and
`md!` only when rendering awaits data. Lift a synchronous child into an async
tree with `proa::to_md_async(...)`.

Keep the Markdown implementation with the owning page or component. Share
titles, prose, links, and collections with its HTML implementation before
writing either layout. Compose core sections as typed children. Preserve
browser-visible content; a Markdown export should not silently become a second
editorial summary. Omit decorative or interactive-only elements deliberately.

## Author the template

Prefer one multiline `md_sync! { r###"..."### }` template for a document or
substantial section. Its source newlines become output newlines. Write headings,
links, emphasis, lists, and blank lines as normal Markdown rather than splitting
the document into quoted token pieces.

Keep short fragments on one line with an ordinary string literal:
`md_sync! { "- {feature}\n" }` for a list row or
`md_sync! { "**In stock**" }` for inline markup. A single string is still a
template, so `{feature}` interpolates normally. Keep `\n` when the fragment
must end a line; dropping it would join loop iterations. Do not turn a short
fragment into a multiline raw template just to avoid that one escape.

The macro body is one string literal. Every template renders in one of two
modes, chosen by the literal's shape:

- Block: a raw literal spanning source lines. Adds only the missing line feeds
  so the body starts and ends on a paragraph boundary. Use for documents and
  sections.
- Inline: an ordinary string or a one-line raw literal. Writes bytes exactly as
  authored, including `\n`, and rejects block children. Use for labels, bold or
  link text, table cells, and list rows.

Token composition such as `md_sync! { "- " {item} "\n" }` or
`md_sync! { {overview}{impact} }` is a compile error. To render children in
sequence without authoring Markdown, use a tuple or a loop of `.render_md(cx)?`
calls: `(overview, impact).render_md(cx)`. Each child keeps its own mode, so
byte-exact output such as robots.txt gains no boundaries.

Hash count never selects the mode. Scopes restore the caller's mode on errors,
unwinding, and async cancellation, preserving output and its suffix tracking.
Use `.render_md(cx)?` directly instead of manual `md_set_inline` save/restore.
Do not put a block template in an inline template, link text, heading,
emphasis, strikethrough, or table cell: a directly nested `md_sync!` or `md!`
block literal is a compile error, and typed children fail at render time before
the block writes anything. Give those children an inline representation. See
the "Template modes" section of the document-rendering guide for byte examples.

Text outside `{ ... }` is Markdown; each slot is Rust at every nesting depth.
Use ordinary Rust `if`, `for`, and `match` inside slots, and explicitly re-enter
Markdown with a nested `md_sync!` (or `md!` when it awaits). Never treat bare
control words in Markdown as executable or write Markdown directly in a Rust
branch. A body renders only its final expression, matching HTML slots. Earlier
statements are ordinary Rust setup/side effects; a semicolon discards a value.
To render several fragments, compose them in one nested template tail.

```rust
md_sync! { r###"
    # {self.title}

    ## Features

    {for feature in self.features.iter() {
        md_sync! { "- {feature}\n" }
    }}

    {if self.in_stock {
        md_sync! { "**In stock**" }
    } else {
        md_sync! { "_Backordered_" }
    }}
    "###
}
```

Use raw literals for genuinely multiline sections or when they materially
reduce escaping. Nested raw literals need fewer hashes than their parent
(`r###"..."###` outside, `r#"..."#` inside, then `r"..."` if needed); no inner
closing delimiter may close its parent. Each multiline fragment removes its own
common source margin. Its trailing newline is output, so do not add another
separator unnecessarily. A block child on its own line keeps exactly one blank
line after it: the parent's line break after the slot is a boundary, not extra
bytes. A bare Rust string tail is escaped text, not Markdown markup.

`{expression}` renders a typed child. Outside code fences and inline code,
`{{` and `}}` produce literal braces. Inside code, braces are already literal;
interpolating runtime values into a fenced example needs an explicit typed
child. Explicit whitespace-only fragments such as `md_sync! { "\n" }` remain
appropriate when exact separators are the contract, as in robots.txt. Nested
`md!` and `html!` can borrow loop items and local strings across awaits through
their shared typed async continuation. Use sync nested fragments when they have
no async work.
Raw literals do not disable interpolation: use `{{`/`}}` for literal braces,
even around quoted text. A Rust string inside a slot stays literal text.
Do not use `\{name}` to escape a slot: it emits a backslash followed by the
rendered value. With `name = "Ada"`, `{{name}}` emits `{name}` and `{{{name}}}`
emits `{Ada}`. A Rust block slot needs a space between its opening braces:
`{ { name } }`; `{{ name }}` emits literal `{ name }`. Escaping happens once;
runtime strings are not parsed as templates. Code spans and fences (including
fences inside blockquotes) preserve braces verbatim, even doubled braces.
Code boundaries follow CommonMark with GFM tables after dedenting, including
list/blockquote fences and indented code blocks. Inline code cannot span
paragraph or table-cell boundaries.
Rust strings and layout inside a slot cannot open Markdown code in its parent.

Named arguments (`md_sync! { "{title}", title = data.title }`) bind once, in
declaration order, when rendering starts. Their initializers use the caller's
scope; branches and nested templates use ordinary Rust bindings. Borrow owned
values used repeatedly (`{&title}`); arguments do not clone automatically.

Ordinary slots escape prose. Link destinations need URL-context escaping;
follow the owning crate's typed URL leaf rather than inserting trusted raw
Markdown. Reserve `TrustedMarkdown` for a reviewed, already-authored source
boundary. Do not build documents with string concatenation or render children
to temporary strings.

For negotiated HTTP exports, serve `text/markdown; charset=utf-8` and
`Vary: Accept`; provide a discoverable `.md` or `llms.txt` link. Cache the final
owned document at the response boundary when its inputs are immutable.

## Migrate manual formatting cleanup

- Replace inline `md_set_inline` save/render/restore sequences with an ordinary
  single-literal template, such as `md_sync! { "**{self.children}**" }.render_md(cx)?`.
  Saving a `Result` only to restore inline mode is unnecessary; restoring after
  `?` is incorrect, and manual cleanup does not cover panic unwinding.
- A document wrapper must allow block children. Use a multiline raw template,
  or render the sections in sequence with a tuple when exact existing
  separators must stay byte-for-byte identical. `md_sync! { "{section}" }` is
  an inline wrapper and rejects a block section, even when the section is a
  typed component.
- Never restore a whole `md_state()` snapshot solely to clean up formatting:
  it also replaces output-suffix tracking, even though partial bytes remain.
  Keep deliberate snapshot/low-level-setter tests and buffer-reset benchmarks.
- Audit scaffold copies, source fixtures, and helper rustdoc alongside production
  components. Test a failing or panicking child followed by fallback rendering;
  verify both formatting restoration and the exact retained output.

## Verify behavior

Check dynamic titles, escaped Markdown punctuation, URL destinations, section
order, and intentional omissions. A changed-input test should show that both
HTML and Markdown consume the same content. Avoid tests that merely repeat
hard-coded prose.

For tests that store Rust source containing Markdown macros, prefer a raw outer
Rust string to avoid escaping every inner quote. Keep the fixture's decoded
bytes unchanged. Inner list-row `\n` escapes, CRLF data, and explicit tests of
cooked strings or source encoding still need their intended escapes; do not
remove backslashes indiscriminately.

In the Proa checkout, run `cargo proa fmt --check --verify <changed paths>`,
`cargo proa lint <changed paths> --deny warnings`, the owning crate's tests,
and `python3 scripts/proa_perf_lint.py`. In an application, use its established
build and test commands. Do not assume a neighboring `proa_ui` checkout.
