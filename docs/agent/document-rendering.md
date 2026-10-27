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

Implement HTML and Markdown on the same page/component types. Share their
data and compose typed children directly. A page may intentionally choose a
different structure for each representation, but do not maintain a second
`.md` template with string-replacement placeholders beside an existing Rust
page. Agent indexes and authentication documents also deserve named Markdown
components even when they have no HTML representation.

Write Markdown as one multiline `r###"..."###` template. Newlines are literal;
avoid constructing paragraphs with `push_str`, `format!`, joins, or escaped
newline fragments. Raw-template interpolation stays literal inside code
spans/fences; use an explicit typed code child or token composition when an
example needs dynamic code values.

See [the marketing homepage](../../proa_marketing/src/pages/home.rs),
[About/Contact](../../proa_marketing/src/pages/info.rs), and
[agent document components](../../proa_marketing/src/pages/agent.rs).

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
