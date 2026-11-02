---
name: proa-markdown
description: Author typed Markdown renderers and shared HTML/Markdown page content in Proa. Use for md_sync!, MdRenderSync, Markdown exports, or keeping agent-readable page representations in sync with HTML.
---

# Proa Markdown

Read [document rendering](../../../docs/agent/document-rendering.md) and the
[render trait reference](../../../docs/agent/api-reference/render-traits.md)
in the source checkout. In an application without those files, retrieve the
corresponding current documentation from `https://proa.so/docs/llms.txt`.

## Choose the rendering boundary

When the same Page also renders a managed-head HTML document, declare
`const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::PAGE;` on its
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
tree with `proa_core::to_md_async(...)`.

Keep the Markdown implementation with the owning page or component. Share
titles, prose, links, and collections with its HTML implementation before
writing either layout. Compose core sections as typed children. Preserve
browser-visible content; a Markdown export should not silently become a second
editorial summary. Omit decorative or interactive-only elements deliberately.

## Author the template

Prefer one multiline `md_sync! { r###"..."### }` template. Newlines become output
newlines. Write headings, links, emphasis, lists, and blank lines as normal
Markdown, rather than splitting them into quoted tokens and `"\n"` fragments.

Text outside `{ ... }` is Markdown; each slot is Rust at every nesting depth.
Use ordinary Rust `if`, `for`, and `match` inside slots, and explicitly re-enter
Markdown with a nested `md_sync!` (or `md!` when it awaits). Never treat bare
control words in Markdown as executable or write Markdown directly in a Rust
branch. A body renders expression statements without semicolons in order;
semicolon-terminated statements are ordinary Rust setup/side effects.

```rust
md_sync! { r###"
    # {self.title}

    ## Features

    {for feature in self.features.iter() {
        md_sync! { r#"
            - {feature}
            "#
        }
    }}

    {if self.in_stock {
        md_sync! { r#"**In stock**"# }
    } else {
        md_sync! { r#"_Backordered_"# }
    }}
    "###
}
```

Prefer nested raw fragments even for inline Markdown. Use fewer hashes inside
an outer raw literal (`r###"..."###` outside, `r#"..."#` inside, then `r"..."`
if another depth is needed); no inner closing delimiter may close its parent.
Each multiline fragment removes its own common source margin. Its trailing
newline is output, so do not add another escaped separator between ordinary
blocks. A bare Rust string tail is escaped text, not Markdown markup.

`{expression}` renders a typed child. Outside code fences and inline code,
`{{` and `}}` produce literal braces. Inside code, braces are already literal;
interpolating runtime values into a fenced example needs an explicit typed
child or token composition. Keep token composition for those cases, generated
or token-aware tooling, and tests of that API; it remains supported. Explicit
whitespace-only fragments such as `md_sync! { "\n" }` remain appropriate when
exact separators are the contract, as in robots.txt. Inside async `md!`, use
`md_sync!` for a loop fragment borrowing its item; nested erased async
fragments have the same borrowing limit as nested `html!`.

Ordinary slots escape prose. Link destinations need URL-context escaping;
follow the owning crate's typed URL leaf rather than inserting trusted raw
Markdown. Reserve `TrustedMarkdown` for a reviewed, already-authored source
boundary. Do not build documents with string concatenation or render children
to temporary strings.

For negotiated HTTP exports, serve `text/markdown; charset=utf-8` and
`Vary: Accept`; provide a discoverable `.md` or `llms.txt` link. Cache the final
owned document at the response boundary when its inputs are immutable.

## Verify behavior

Check dynamic titles, escaped Markdown punctuation, URL destinations, section
order, and intentional omissions. A changed-input test should show that both
HTML and Markdown consume the same content. Avoid tests that merely repeat
hard-coded prose.

In the Proa checkout, run `cargo proa fmt --check --verify <changed paths>`,
`cargo proa lint <changed paths> --deny warnings`, the owning crate's tests,
and `python3 scripts/proa_perf_lint.py`. In an application, use its established
build and test commands. Do not assume a neighboring `proa_ui` checkout.
