# Authoring rendered documents

Use Proa's format-specific macros for substantial authored output, including
route handlers, discovery files, cached responses, and error pages. Being
static, startup-only, or outside an HTML render hot path does not justify a
hand-built document string.

| Output | Authoring surface |
| --- | --- |
| HTML | `WebRenderSync` + `html_sync!`; `WebRender` + `html!` when awaiting |
| Markdown | `MdRenderSync` + `md_sync!`; `MdRender` + `md!` when awaiting |
| XML | `WebRenderSync` + `xml_sync!`; `xml!` when awaiting |
| Structured plain text | `md_sync!` for multiline documents; `text!` for formatted text leaves |
| Literal JSON | `const_json::const_json!`, producing a compiler-validated `&'static str` |
| Const-evaluable typed JSON | `#[derive(const_json::ConstJson)]` and `.to_json()` |
| Runtime JSON | `#[derive(serde::Serialize)]` models and the serializer/HTTP `Json` boundary |

## Choose the HTML render entry point

Choose the document's delivery path before writing its rendering wrapper:

| Use case | Entry point |
| --- | --- |
| Server page | Return `RouteResponse::ssr(document)` (or the matching async/streaming constructor); configure managed head on the response or `FrameworkBuilder` |
| Synchronous standalone HTML file | `HtmlDocumentContext::new()`, `cx.render(document)?`, then `cx.finish()?` for the bytes |
| Ordinary component fragment without managed metadata | Render into a `WebContext`, or use the existing fragment rendering helpers |

For a standalone document, compose the layout and Page normally, with one
explicit `ComponentRole::PAGE` and one `{cx.head()}` outlet. The document context
runs the managed metadata lifecycle; application code does not need to create
an empty island manifest, a request context, or a response finalizer. Direct
root `.render(&mut cx)` calls skip head hooks and are not a substitute for this
entry point. Keep lower-level setup in framework adapters and their tests.
The context's `render` runs immediately and accepts borrowed or non-`Send`
`WebRenderSync<NoLoader>` roots. Render exactly one document; a failure or second
render invalidates the context, and `finish` returns no partial HTML. Use
`HtmlDocumentContext::with_head(head)` for custom metadata defaults or URL policy.
The one-call `render_sync_html_document(document, head)` convenience wrapper uses
the same context lifecycle.

The context exports HTML, not asset files. External scripts and generated RSJS
assets still need resolvable URLs; do not assume an interactive island works
from `file://` merely because the HTML was written successfully. Use the route
pipeline for documents needing an external React island manifest or request
configuration. In downstream projects, verify the resolved Proa version exposes
the required API and align dependencies before building the example.

## Share page ownership across representations

Organize by the page or interface that owns the content, not by its audience.
Keep `WebRenderSync` and `MdRenderSync` implementations on the same component
in that page's module. Export its Markdown renderer from the negotiated route
and any `.md` / `llms.txt` aliases. A separate `agent.rs`, `AgentHome`, or
handwritten overview creates another owner for the same page and should not
be introduced. Documentation indexes and protocol documents belong with their
owning documentation/interface module.

### Shared-content workflow

1. Identify the core sections, headings, descriptions, links, examples, and
   data collections. Define each content value once in borrowed props, a shared
   model, or a small content constructor. Include labels, URLs, image
   descriptions, and browser-visible values. Preserve existing HTML copy and
   `PageMeta`; adding a render format is not an editorial rewrite.
2. Make HTML read those values. Make Markdown read the same fields and compose
   the same content-bearing children. Do not copy literal prose into a second
   template, even when both render implementations are in the same file. Do not
   maintain separate HTML and Markdown content constructors.
3. Give core sections their own `MdRenderSync` / `MdRender` implementations.
   Express structure with `md_sync!` / `md!`: headings, paragraphs, links,
   lists, tables, and typed code children. The page implementation composes
   those sections. Markdown can flatten tabs into a table or list all choices;
   it need not transcribe controls, decorative artwork, or visual demo fixtures.
4. Use a targeted `omit!` for a deliberate visual-only contribution or changed
   presence policy. Reuse shared data for all tab cases. Keep page coverage
   enabled; do not hide a separate summary behind a whole-page exemption.
   The linter is a backstop, not a deduplication tool or proof that two copies
   of a string remain synchronized.
5. Verify shared data with changed-input tests: update a title, description,
   destination, or collection and check both outputs. Test Markdown section
   order, tab-to-table behavior, escaping, and omissions. Do not require a full
   DOM transcription when the intended Markdown contract is the core content.

For example, this section defines its content once and exposes each format's
structure directly:

```rust
struct Section<'a> {
    title: &'a str,
    description: &'a str,
}

impl WebRenderSync for Section<'_> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <section data-section>
                <h2>{self.title}</h2>
                <p>{self.description}</p>
            </section>
        }.render(cx)
    }
}

impl MdRenderSync for Section<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        md_sync! { r###"
            ## {self.title}

            {self.description}
            "###
        }.render_md(cx)
    }
}
```

Keep the distinction between shared **content** and format-specific **structure**.
Do not create a universal document tree solely to remove HTML/Markdown layout
syntax. Small fields, existing data types, and typed children usually suffice.

### Give shared values one owner

Before adding a URL, organization name, contact address, tagline, route, or
other shared value, search for its existing owner and reuse it. Values that
must change together belong in shared constants or a small typed model, even
when their consumers are in different modules (navigation, JSON-LD, discovery,
OpenAPI, and page content). Derive child URLs, mailto links, and composed copy
from those values; copying the same base into several constants still drifts.
Use compile-time composition for static strings and the deployment-origin
resolver for runtime absolute URLs. Keep section-specific copy with its owning
component. Do not group unrelated values solely because their text happens to
match. See the marketing application's [`data/site.rs`](../../proa_marketing/src/data/site.rs)
and [`data/routes.rs`](../../proa_marketing/src/data/routes.rs).

### Existing HTML-only source data

Use Proa's explicit `to_markdown(...)` adapter only at a narrow boundary for
pre-existing HTML-only data, such as an imported highlighted code fragment,
when there is no shared semantic source. Do not use it as the default
implementation for authored page sections or interactive widgets. It buffers
HTML and a DOM, skips hidden content, and bypasses child Markdown renderers;
these properties do not express the core structure of an agent-readable page.
See [the converter contract](api-reference/render-traits.md#convert-existing-html-components-to-markdown).

Write Markdown as one multiline `r###"..."###` template. Newlines are literal;
avoid constructing paragraphs with `push_str`, `format!`, joins, or escaped
newline fragments. Raw-template interpolation stays literal inside code
spans/fences; use an explicit typed code child when an example needs dynamic
code values.

### Template modes

The body of `md_sync!` and `md!` is one string literal, optionally followed by
`name = value` arguments. Every template renders in one of two modes. The shape
of the literal selects the mode, the mode is applied when the template renders,
and it decides what the framework writes around the body:

| Mode | Select it with | Before the body | After the body | Use it for |
| --- | --- | --- | --- | --- |
| Block | A raw literal spanning source lines | Completes a paragraph boundary, adding only the missing line feeds and nothing at the start of output; errors inside an inline scope | Completes a paragraph boundary | Documents and sections: anything with headings, paragraphs, or lists |
| Inline | An ordinary string, or a raw literal on one line | Marks the output as inline content, so a block child errors | Restores the caller's mode | Labels, bold or link text, table cells, list rows |

Raw delimiter hashes only delimit the literal: `r"`, `r#"`, and `r###"` follow
the same rule, and dedenting is unchanged.

**Block.** The body starts and ends on a paragraph boundary, and authored
whitespace is never collapsed. After inline text:

```rust
md_sync! { "Intro" }.render_md(cx)?;
md_sync! { r#"
    ## Heading

    Body
    "#
}
.render_md(cx)?;
```

writes `Intro\n\n## Heading\n\nBody\n\n`. The two line feeds after `Intro` and
the final one after `Body` were added; every other byte is the literal.

**Inline.** Bytes are written exactly as authored, including `\n`:

```rust
md_sync! { "**{status}**" }.render_md(cx)?; // **Ready**
for item in items {
    md_sync! { "- {item}\n" }.render_md(cx)?; // - a\n- b\n
}
```

An inline template is an inline scope, so a block child inside it is an error:

```rust
md_sync! { "[{Section}](/docs)" } // Section renders a block template
```

fails with `cannot render a block Markdown template inside an inline scope`
after writing `[`. The inline parts of a block template behave the same way:
link text, headings, emphasis, strikethrough, and table cells. A directly
nested `md_sync!` or `md!` block literal called under its own name is rejected
at compile time; typed children, renamed imports, and children reached through
helpers fail at render time before the block writes its boundary or body.
Earlier bytes remain. Give such children an inline representation instead of
embedding a document in a link or table cell.

**Composing without authored Markdown.** When a renderer only sequences
children that write their own Markdown, do not wrap them in a template. Use a
tuple, which renders its elements in order and stops at the first error, or a
loop of `.render_md(cx)?` calls. Each child applies its own mode, so block
sections keep their paragraph boundaries and byte-exact output such as
`robots.txt` gains none. Token composition such as
`md_sync! { {overview}{impact} }` or `md_sync! { "- " {item} "\n" }` is a
compile error that points back to the string form.

```rust
(overview, impact, activity).render_md(cx)
```

**Guarantees in every mode.** The framework restores the enclosing inline
setting on success, error, panic unwinding, and (for `md!`) cancellation. It
does not reset list depth. Partial output and its boundary tracking remain
live; cleanup never rolls back bytes or appends a trailing
boundary after failure. Call `.render_md(cx)?` directly rather than saving and
restoring `md_set_inline`:

```rust
md_sync! { "**{self.children}**" }.render_md(cx)?;
```

**Edge cases.** An empty block body emits nothing at the start of output, but
can still finish a boundary after preceding prose. A body with no rendered loop
items still retains any authored newlines around that loop. Whitespace-only raw
literals keep their literal whitespace, including closing-line indentation under
the existing parser contract, then complete a trailing boundary. An empty
ordinary or single-line raw literal emits nothing.

### Migrate existing formatting scopes

Use an ordinary-string template for inline children rather than saving a
`Result` to restore `md_set_inline`. A component can propagate errors directly:

```rust
md_sync! { "**{self.children}**" }.render_md(cx)?;
```

For document children, choose a multiline raw template. An ordinary-string
wrapper such as `md_sync! { "{section}" }` selects inline mode and rejects block
sections. If existing separators must remain exact, render the sections in
sequence with a tuple or `.render_md(cx)?` calls and spell each separator as
its own inline template, such as `md_sync! { "\n\n" }`.

Do not restore an entire `md_state()` snapshot just to clean up formatting:
that snapshot also contains the logical output suffix. Snapshot restoration
is appropriate for intentional fragment isolation or after clearing a reused
buffer, not for keeping partial output after a rendering error.

Audit application renderers, scaffold copies, source fixtures, and helper
rustdoc together. Deliberate state-setting tests should keep exercising the
low-level APIs. Add recovery tests that check the enclosing setting and the
bytes retained before a child fails or panics.

The grammar has one rule at every depth: text outside `{ ... }` is Markdown,
and text inside is a Rust expression. Control flow is ordinary Rust (`if`,
`else if`, `for`, `match`, `if let`), and a branch renders only its final
expression. Earlier statements keep ordinary Rust semantics; a semicolon
discards a value. A branch re-enters Markdown only through a nested `md_sync!` (or `md!`) call; a bare string tail renders
as escaped text, so markup needs the nested template:

```rust
md_sync! { r###"
    ### Reviews

    {if self.reviews.is_empty() {
        md_sync! { "_No reviews yet_" }
    } else {
        for review in self.reviews.iter() {
            md_sync! { "- {review}\n" }
        }
    }}

    {if self.stock > 0 {
        md_sync! { "Only {self.stock} left" }
    } else {
        md_sync! { "Backordered" }
    }}
    "###
}
```

Use an ordinary one-line string for short inline markup or list rows:
`md_sync! { "**In stock**" }` or `md_sync! { "- {review}\n" }`. Keep the newline
when each iteration must produce a separate line. A single literal
interpolates `{expression}`; splitting it into quoted token pieces such as
`md_sync! { "- " {review} "\n" }` is a compile error.
Use raw literals with fewer hashes for substantial multiline fragments or when
they substantially reduce escaping. Each multiline literal removes its own
common indentation and retains its authored trailing newlines. The line
breaks directly after a slot at block position are boundaries rather than
bytes: written after inline text, skipped after a child that already ended
its line or paragraph. A block child on its own line therefore keeps exactly
one blank line after it, and row templates on consecutive lines stay one
list. Further authored blank lines stay literal, as does every break inside
an inline template. Dynamic content inside code fences, which raw templates
deliberately keep literal, goes through an explicit typed code child.
Nested `md!` and `html!` may borrow loop items and local strings across awaits.
Their shared typed async continuation keeps those borrows alive until rendering
finishes; neither requires cloning the input. Use a sync nested fragment when
it has no async children.
Plain Markdown is never parsed inside a slot, so `if`, `for`, and `match` are
ordinary words in Markdown text, and a line means the same thing at the top
level and inside a branch. Invalid Rust in a control-flow slot gets a diagnostic
showing the nested-template form. A bare word is valid Rust syntax and is resolved
as an identifier; an unknown name gets Rust's ordinary unresolved-name error.
Named arguments bind values for the whole template, including conditions,
iterators, and nested templates.

An unbounded `loop` can stream nested Proa templates and exit through `return`
or `?`, including on Rust 2024. Use absolute paths such as
`::proa_macros::md_sync!` for nested templates in these loops. Unqualified names
can refer to user macros, so their expansions remain opaque to loop-exit
analysis. A divergent loop containing an opaque macro may need a concrete
result type. Write control-flow exits directly in Rust when possible.

A raw Rust literal disables Rust backslash escaping, not template slots.
For example, `md_sync! { r#"{"Hello {name}"}"# }` renders `Hello {name}`:
the outer braces are a slot containing a Rust string. To print the braces and
quotes too, write `md_sync! { r#"{{"Hello {{name}}"}}"# }`. A string value
inside a slot is never recursively interpreted as a template.

Brace escaping happens once, before Markdown is rendered. With `name = "Ada"`:

| Template content | Markdown source emitted |
| --- | --- |
| `{{name}}` | `{name}` |
| `{{{name}}}` | `{Ada}` |
| `\{name}` | `\Ada` |
| `{{ name }}` | `{ name }` |
| `{ { name } }` | `Ada` |

A backslash is Markdown content, not a template-brace escape. In a cooked Rust
literal, write `\\` to produce that backslash; raw literals preserve it directly.
To enter a Rust block expression, separate the slot's opening brace from the
block's opening brace with whitespace, as in the last row. Inside inline code
and fences, including fences in blockquotes, leave braces as authored:
`{{name}}` in code stays doubled. Runtime strings are never scanned for slots.

Code boundaries follow CommonMark with GFM tables after template dedenting.
Fences inside lists and blockquotes, indented code blocks, and inline code all
keep braces literal. An inline code span cannot cross a paragraph, table cell,
or another block boundary. Rust slots are opaque: strings and layout inside a
slot cannot open a Markdown fence or HTML block in the surrounding template.

Indentation inside a slot is Rust layout and never reaches the output. A
whitespace-only template such as `md_sync! { "\n" }` is emitted verbatim, so
a branch can contribute a blank line. `{{` and
`}}` are literal braces in Markdown text; braces inside inline code spans and
fenced code blocks are literal without escaping. `proa fmt` re-anchors the
template one level beneath the invocation and preserves relative indentation;
it never rewrites slot contents. File-scoped `proa check` and `proa lint`
validate Markdown syntax without requiring a Cargo build, and `proa fmt`
refuses invalid templates.

Named arguments are Rust bindings: `md_sync! { "{value}", value = data.title }`.
They evaluate once, in declaration order, when rendering starts. Every
initializer sees the caller's scope; the argument names become visible after
all initializers finish. Branch locals, loop patterns, closures, and nested
templates then follow normal Rust scope and ownership rules. Borrow a value
when it must render more than once (`"{&value} {&value}"`). Duplicate names
are template errors; Rust diagnoses unused bindings, with the usual `_name`
convention for intentionally ignored values.

See [the marketing homepage](../../proa_marketing/src/pages/home.rs),
[About/Contact](../../proa_marketing/src/pages/info.rs), and
[documentation-owned exports](../../proa_marketing/src/pages/docs.rs).

## Keep serialization and escaping explicit

- `const_json!` accepts literals, not runtime expressions. Use it for schemas
  and fixed documents; use typed `Serialize` models when origins, collections,
  or values are supplied at runtime. Do not construct large runtime JSON trees
  with `serde_json::json!` when the shape is known. Tests and genuinely dynamic
  schema transformation may use `serde_json::Value`.
- Generate application API/tool contracts from the request and response types
  with the owning protocol's tooling (for example, Utoipa for OpenAPI and
  Schemars/RMCP for MCP). Do not maintain a parallel literal schema for a Rust
  model, or rewrite one protocol's serialized schema strings into another.
  Keep operation annotations with handlers and serialization at the cache boundary.
- Preserve format-specific escaping. Ordinary Markdown slots escape prose;
  link destinations use a small typed leaf calling `write_md_url`. XML uses
  `xml_sync!` escaping, not HTML escaping or concatenated XML strings.
- `UnsafeRaw` and `TrustedMarkdown` are boundaries for already-authored,
  trusted content, not shortcuts around macros or escaping. JSON embedded in
  an HTML `<script>` also needs script-safe escaping after JSON serialization.
- Allocate an owned response only at the response/cache boundary. Compose
  child renderers directly instead of rendering each child to a `String`.

Existing content collections, imported source documents, skill files, and
generated artifacts remain data. Keep their established build/validation
pipeline; this rule does not require rewriting every `.md`, `.mdx`, `.json`,
or static asset in the repository as Rust.

Executable browser code follows the
[RSJS-first policy](rsjs-islands.md#prefer-rsjs-for-browser-behavior).
An HTML macro around a raw JavaScript string does not satisfy that policy;
use RSJS for supported behavior and document any necessary bridge.

## Review before finishing

Inspect newly introduced `include_str!`, large quoted/raw strings, `format!`,
`.replace`, `push_str`, joins, and `json!` in document-producing code. Determine
whether each is source data, a small value operation, a serializer boundary,
or an authored template that belongs in a macro. A performance-lint suppression
does not waive the template authoring rule.

Run Proa formatting/lint and focused checks of rendered behavior: dynamic data,
escaping, shared HTML/Markdown values, link destinations, JSON types, and HTTP
status/content negotiation. Do not pin prose copied from the implementation.

### Markdown compiler dependencies

The `markdown` feature on `proa_macros` and `proa_dsl` is enabled by default.
HTML/XML-only consumers can set `proa_macros = { version = "0.3", default-features = false }`
to omit the Markdown parsers. Enable `features = ["markdown"]` explicitly when
using `md!` or `md_sync!` with default features disabled. Cargo unifies features:
if another dependency enables Markdown, its parser dependencies are still built.
Source tools and documentation builders enable Markdown explicitly.
