# HTML-to-Markdown coverage

Use `proa_core::to_markdown(value)` with the `html-to-markdown` feature to
reuse a synchronous HTML renderer. See [the usage examples](render-traits.md#convert-existing-html-components-to-markdown).

This is a content conversion, not a browser or a lossless HTML round trip.
The output uses CommonMark syntax and GitHub Flavored Markdown (GFM) pipe
tables. Consumers need GFM table support to render those tables as grids.
CSS, JavaScript, layout, accessibility relationships, and most attributes are
not reconstructed. Use a handwritten Markdown implementation for those
representations.

## Element policies

The coverage suite contains explicit fixtures for all **141 built-in Proa
tags**, including HTML and SVG. SVG-only tags are tested in an SVG context.
This includes documented omission and text fallback policies, rather than
claiming every element retains its original semantics. Unknown/custom HTML
elements normally retain their child content; their styling and behavior are
not inferred.

| Elements | Conversion |
|---|---|
| `h1`–`h6`, `p`, `blockquote`, `br`, `hr` | Headings, paragraphs, quotes, hard breaks, thematic rules. |
| `strong`, `b`, `em`, `i` | Strong/emphasis delimiters; combinations are tested. Some HTML emphasis boundaries cannot be expressed faithfully in pure Markdown. |
| `ul`, `ol`, `li` | Lists; ordered lists retain a representable starting ordinal. Reversed lists, per-item values, negative starts, or ordinals outside Markdown's 0–999999999 range become bullets with explicit numeric labels. CSS list styles and `type` presentation are not retained. |
| `code`, `pre` | Adaptive code spans/fences from literal DOM text, including highlighted children. Empty inline code emits nothing; code-span line endings become spaces. Fenced code preserves whitespace. `language-*` classes on the immediate code child or pre element provide the fence label. |
| `a`, `img`, `picture` | Inline links and images. URLs/titles/alt text are escaped. Links without `href` keep child content. Images without `src`, including `srcset`-only images, keep alt text as ordinary Markdown text; no responsive candidate is chosen. |
| `table`, `caption`, `thead`, `tbody`, `tfoot`, `tr`, `th`, `td` | Pipe tables for simple grids; ordered rows with nested cell items for complex grids. See below. |
| `html`, `body`, `header`, `footer`, `nav`, `main`, `section`, `article`, `aside`, `hgroup`, `address`, `div`, `figure`, `figcaption`, `dl`, `dt`, `dd` | Child content with the converter's block boundaries; container roles, figures, and definition relationships are not reconstructed. |
| `abbr`, `bdi`, `bdo`, `cite`, `data`, `dfn`, `kbd`, `mark`, `q`, `rp`, `rt`, `ruby`, `s`, `samp`, `small`, `span`, `sub`, `sup`, `time`, `u`, `var`, `del`, `ins`, `slot`, `selectedcontent`, `wbr` | Child text/content. No inferred quotes, ruby layout, directionality, underline, strike-through, superscript, or annotation semantics. `wbr` emits no text. |
| `form`, `label`, `button`, `select`, `datalist`, `optgroup`, `option`, `textarea`, `output`, `progress`, `meter`, `fieldset`, `legend` | Child text only. Input values, checked/selected state, numeric attributes, validation, and control behavior are not synthesized. All option text may remain, rather than only the current selection. |
| `details`, `summary`, `dialog`, `menu`, `search` | Child content; disclosure/open state, modal behavior, and interaction are not simulated. |
| `audio`, `video`, `canvas`, `map`, `iframe`, `object` | Parsed fallback text/children only. No media transcription, canvas description, iframe fetching, or script execution. |
| `input`, `area`, `embed`, `param`, `source`, `track`, `col`, `colgroup` | No textual content is synthesized from attributes. |
| `head`, `title`, `meta`, `link`, `base`, `script`, `noscript`, `style`, `template`, `svg` and its children | Metadata, executable/style/template content, noscript subtrees, and SVG subtrees are omitted wherever the parser places them, including in the body. Comments and doctypes are omitted. |

MathML, which is outside Proa's built-in tag catalog, has a tested child-text
fallback. Spatial notation and MathML semantics are not converted to LaTeX.
CSS classes do not define Markdown semantics; class-based math markup may be
recognized by the upstream converter, but is not a general math renderer.

## Tables

Simple grids retain every `th` and `td` in source order, including mixed rows,
row-header cells, multiple body sections, footer cells, captions, empty cells,
and Unicode text. Ragged rows are padded to the widest row. A headerless table
gets an empty header row, leaving its first data row intact. Empty headers
still have a valid delimiter row; an entirely empty table emits nothing.
Alignment, widths, `scope`, and `headers` relationships are not retained.

Pipes are escaped for GFM table parsing, including inside code spans, links,
and image labels. Backslashes, backticks, entity-shaped literals, and link
destinations are covered by round-trip parser assertions.

Tables with spans, multiple header rows, nested tables, block content, line
breaks, or unknown cell elements use an ordered list of rows containing nested
cell items. This preserves each cell's content and nested Markdown structure
without assigning it a misleading pipe-table column position. It does **not**
retain the visual geometry of `rowspan`/`colspan`, or duplicate a spanning cell
into other rows. A nested simple table can remain a pipe table within that
fallback. Captions remain separate content before the rows.

These choices reflect the [GFM table grammar](https://github.github.com/gfm/#tables-extension-),
which has a header/delimiter row and inline cell content, and the additional
geometry in the [HTML table model](https://html.spec.whatwg.org/multipage/tables.html).

## Escaping, visibility, and state

Runtime text is first escaped by the HTML renderer and parsed as HTML.
Entity-shaped literal text must not be decoded again by Markdown. Conservative
entity references also protect literal markers split across inline nodes.
Image alt text, link destinations, titles, and adaptive code delimiters have
dedicated escaping paths. Unsafe URL schemes are neutralized by the core URL
escaper; this is not an HTML sanitizer API.

`hidden` and `aria-hidden="true"` subtrees are omitted before conversion.
This includes case-insensitive `true`; the boolean `hidden` attribute is
present even when written as `hidden="false"`. CSS visibility, media queries,
browser selection, and inherited external state are not evaluated. Malformed
HTML uses the HTML parser's repairs, not the original token nesting.

Capture shares the live loader and request metadata but isolates HTML escaping,
Markdown formatting, and streaming hooks. Those scopes are restored after
success, errors, and panics. Child components use HTML even if they also have
a Markdown trait; explicitly compose their custom Markdown beside converted
fragments. Async HTML sources are not supported.

Conversion buffers HTML, a DOM, and Markdown. Destination capacity/metering
applies to the final Markdown write, not to the temporary conversion buffers.
The fragment has no surrounding newline, so its containing template owns
separators. It does not adapt its headings to an enclosing Markdown heading
offset or infer whether a block fragment belongs in an inline slot.

## Performance

Direct `md_sync!` writes compiled Markdown with runtime text escaping.
`to_markdown` additionally captures HTML, builds and prepares a DOM, converts
it, and writes the resulting Markdown on every render. Its `LazyLock` only
caches converter configuration; it does not cache converted content.

The [comparison benchmark](../../../proa_core/benches/html_to_markdown.rs)
checks equivalent parsed Markdown semantics before timing three dynamic
fixtures. It measures both a fresh default context/output buffer and a warmed,
reused context with its output cleared and Markdown state reset. Converter
initialization and fixture construction are outside timing; fresh-context
measurements include context construction and destruction. Output buffers
are observed through `black_box`, without flattening them into strings in the
timed loop.

Two local release runs on an Apple M4 Max with Rust 1.95.0, workspace release
settings (`opt-level=3`, thin LTO, one codegen unit), and the system allocator
gave the following ranges of Criterion point estimates. Each benchmark used
50 samples, 300 ms warmup, and 1 s measurement. These are workstation results,
not portable latency guarantees or regression thresholds.

| Fixture | Context | Direct `md_sync!` | Converted HTML | Conversion / direct |
|---|---|---:|---:|---:|
| 28-byte paragraph with strong text | Fresh | 0.129 µs | 1.865–1.913 µs | 14.5–14.8× |
| 124–126-byte product card | Fresh | 0.179–0.190 µs | 3.432–3.683 µs | 19.1–19.4× |
| 128 paragraphs, about 21 KB | Fresh | 16.255–16.284 µs | 272.790–275.800 µs | 16.8–16.9× |
| Paragraph | Reused | 0.0135–0.0136 µs | 1.726–1.831 µs | 126.6–135.6× |
| Product card | Reused | 0.0615–0.0641 µs | 3.287–3.544 µs | 53.5–55.3× |
| 128 paragraphs | Reused | 15.651–15.790 µs | 273.580–274.460 µs | 17.4–17.5× |

Separate allocation measurements were identical across both runs. With a
reused context, direct rendering requested 0, 0, and 2 allocations for these
fixtures; conversion requested 28, 53, and 1,814 allocations, respectively
(10,502, 12,559, and 478,592 cumulative requested bytes). Counts include
`alloc`, `alloc_zeroed`, and `realloc` calls. Requested bytes are not peak live
memory. Allocation counting is disabled during Criterion timing.

Conversion offers reuse of an HTML implementation at a measurable runtime
cost. Handwritten Markdown keeps the compiled rendering path.

Reproduce with:

```bash
cargo bench -p proa_core --features html-to-markdown --locked --bench html_to_markdown
```

## Verification

[The adapter contract tests](../../../proa_core/tests/to_markdown.rs) cover
sync/async Markdown composition, custom writers/loaders, escaping, large
captures, metadata, streaming isolation, errors, and panic restoration.

[The element/edge-case suite](../../../proa_core/tests/to_markdown_coverage.rs)
gates the complete native tag catalog, exercises table variants and list
ordinals, and parses the generated Markdown to check cell contents, structure,
and absence of raw HTML in its fixtures. Three property tests exercise Unicode
text, dynamic table shapes/content, and code with pipes/backslashes/backticks.

Run both suites and doctests with:

```bash
cargo test -p proa_core --features html-to-markdown --locked
```
