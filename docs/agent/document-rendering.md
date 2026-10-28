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
spans/fences; use an explicit typed code child or token composition when an
example needs dynamic code values.

See [the marketing homepage](../../proa_marketing/src/pages/home.rs),
[About/Contact](../../proa_marketing/src/pages/info.rs), and
[documentation-owned exports](../../proa_marketing/src/pages/docs.rs).

## Keep serialization and escaping explicit

- `const_json!` accepts literals, not runtime expressions. Use it for schemas
  and fixed documents; use typed `Serialize` models when origins, collections,
  or values are supplied at runtime. Do not construct large runtime JSON trees
  with `serde_json::json!` when the shape is known. Tests and genuinely dynamic
  schema transformation may use `serde_json::Value`.
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

## Review before finishing

Inspect newly introduced `include_str!`, large quoted/raw strings, `format!`,
`.replace`, `push_str`, joins, and `json!` in document-producing code. Determine
whether each is source data, a small value operation, a serializer boundary,
or an authored template that belongs in a macro. A performance-lint suppression
does not waive the template authoring rule.

Run Proa formatting/lint and focused checks of rendered behavior: dynamic data,
escaping, shared HTML/Markdown values, link destinations, JSON types, and HTTP
status/content negotiation. Do not pin prose copied from the implementation.
