# Keep content across render targets

Run `proa lint src --min-severity info` to discover content differences. The
initial policy compares HTML output contributions with Markdown for the same
component. It follows values into output: adding a field read, constructing a
child, or creating an unused template does not satisfy coverage.

This is a bounded source check. It catches supported omissions; it does not
certify equivalent meaning, order, cardinality, or accessibility across formats.
Rust continues to check field and child render types.

## Context and loaders are preparation

The check compares contributions that reach the rendered body. It does not
require matching `cx` accesses, queries, cache lookups, or loader calls between
views. Known `proa_core::WebContext` loader operations and request, response,
head, telemetry, and local-state setup do not create content obligations or
unchecked-body diagnostics by themselves. This also applies to supported aliases
of those context and metadata handles.

A loaded result only enters the comparison when it reaches a render sink.
An unused fetch cannot satisfy a missing component field or child. When views
load or transform equivalent content through different helpers, wrap the
rendered values with the same `content_part!` name. The name declares the
correspondence; the checker does not infer equivalence from query strings or
local variable names. Unmapped loaded output remains unchecked.

Direct writes and opaque helpers that receive the output context remain
unchecked unless their contributions have explicit mappings. The exemptions
apply to recognized Proa APIs, rather than arbitrary methods named `load` or
`loader`; nested render operations in preparation arguments are still inspected,
and callbacks that may write remain unchecked. This body-content check does not
compare response headers, head assets, or telemetry across targets.

## Use the smallest intentional exception

Put the marker **inside the render method that deliberately leaves content out**.
For HTML → Markdown checks, that is `render_md` in the `MdRenderSync` or
`MdRender` implementation. If the views live in different files, edit the file
containing the Markdown implementation. The marker is a Rust statement before
the template, rather than a file-level declaration or template text.

For example, this complete `src/components/product.rs` keeps the image in HTML
and deliberately excludes it from Markdown:

```rust
use proa_core::{MdRenderSync, WebContext, WebRenderSync, WriteError};
use proa_macros::{html_sync, md_sync, omit};

struct Product<'a> {
    title: &'a str,
    image: &'a str,
}

impl WebRenderSync for Product<'_> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <article data-product>
                <h1>{self.title}</h1>
                <img src={self.image} alt="" />
            </article>
        }
        .render(cx)
    }
}

impl MdRenderSync for Product<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        // A Rust statement INSIDE the Markdown method, before its template.
        omit!(self.image, "Image is only shown in HTML");

        md_sync! { r###"# {self.title}"### }.render_md(cx)
    }
}
```

The value must exist and type-check. The marker neither evaluates nor moves it.
For a local, use `omit!(&notice)` after its declaration and before it is moved.
Bare paths select types, while borrowed locals and dotted fields select values.
A child local selects that child's contribution, including its input provenance.
A field selects contributions depending on that field. If several children use
`self.data`, omitting `self.data` is ambiguous and waives none of them.

When the HTML view renders a child that has no local in Markdown, import its
type at file scope and select it directly inside `render_md`:

```rust
use proa_macros::omit;
use crate::product::RestockNotice;

impl MdRenderSync for Product<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        omit!(RestockNotice<'_>);
        md_sync! { r###"# {self.title}"### }.render_md(cx)
    }
}
```

This is an alternative Markdown implementation for a component whose HTML view
renders that notice. Ordinary Rust type arguments apply: specify required
lifetimes, type arguments, or const arguments as usual. The macro checks the
type without constructing a child. For a child with type/const parameters, the bounded checker
cannot infer its concrete instantiation; use an existing child local or named
part instead of a type selector. If several distinct occurrences have different inputs or
presence conditions, use a local or named part to narrow the exception.
Markers acknowledge missing coverage or deliberate presence differences. An
unconditional marker covers that contribution's source presence policy; a
conditional marker must match the source condition.

To exempt the entire representation, put this on either its impl or render method:

```rust
#[proa_macros::content(skip)]
impl MdRenderSync for Product {
    // ...
}
```

`#[proa_macros::content(skip, reason = "Browser controls only")]` also works.
Reasons are optional. Whole-view exemptions deliberately include future
additions. `proa lint --content-summary --min-severity info` reports them as
**exempt**, along with paired and unpaired discovery status. “Paired” means the
views were discovered, not that semantic equivalence has been proved.

A targeted marker is checked for stale, redundant, ambiguous, and unresolved
selectors. Whole-view exemptions suppress pair checks, including marker checks.
An omitted child in a parent does not exempt that child's own paired views.

## Name contributions that look different

Use the same semantic name for transformed content, static prose, or a helper
whose internals are outside the supported analysis:

```rust
// HTML slot
{proa_macros::content_part!("stock-status", status_badge(self.stock))}

// Markdown slot
{proa_macros::content_part!("stock-status", status_text(self.stock))}
```

`content_part!` evaluates its expression once and returns the original value.
The name is an explicit author assertion about correspondence. It does not
check the helper's meaning. Place the wrapper around the contribution inside its presence condition.
Supported structural conditions inside a wrapper also retain their guards. A
statement wrapper can map a helper that receives the output context or calls a
registered render sink; an unused named value does not satisfy coverage. Name meaningful static text
as a rendered child (for example, `content_part!("notice", text("…"))`).

To omit a named part, use the same file-level `omit` import shown above and
put a string selector inside the Markdown method:

```rust
impl MdRenderSync for Product<'_> {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        omit!("stock-status", "Shown in the overview instead");
        md_sync! { r###"# {self.title}"### }.render_md(cx)
    }
}
```

The string selects the matching `content_part!` name in the source view.

A marker can filter its source target: `omit!(self.image, from = "html")`
or `omit!(RestockNotice<'_>, "Browser only", from = "html")`.
The initial registry uses `html` and `markdown` as target identifiers.

## Pairing and single-target components

HTML-only and Markdown-only components are valid and need no omission markers.
The linter compares content when both views exist; it does not require a
component to implement another render target. Comparisons are directed policies
in `ContentOptions`.

If HTML and Markdown use separate types, explicitly select the source:

```rust
#[proa_macros::content(source = HtmlProduct, from = "html")]
impl MdRenderSync for MdProduct { /* ... */ }
```

Use matching `content_part!` names for such pairs. Equal field spellings on two
different types do not establish correspondence. Unmapped contributions remain
partially checked rather than receiving a parity certificate.

Pair identity includes the crate entry point, lexical module, component type,
reference form, generic shape, non-render generic bounds, and cfg boundaries.
Ordinary explicit imports and local reexports are resolved. Generic parameter
names may differ; different concrete instantiations are separate components.
Sync, async, and Send traits belong to one logical HTML or Markdown family.
Multiple execution variants can share a comparison when their supported output
signatures agree; differing variants or configuration boundaries are unchecked.

Local render impls also identify locally named macro-generated components.
Arbitrary macro expansion, glob-hidden identities, external component aliases,
and type aliases are not resolved. Whole-view skip is always available; it is
not a substitute for declaring correspondence when you want ongoing checks.

## Diagnostics and enforcement

Diagnostics identify the component, source target, checked target, and specific
value/child/part. Terminal output includes source frames in both files, a direct
cause, and a repair. Suggested omissions are scoped to an actual value or
resolvable child. They are suggestions requiring author judgment; the linter
never inserts an exception automatically.

| Code | Meaning |
| --- | --- |
| `render/content-coverage` | A supported rendered contribution is absent. |
| `render/content-coverage-unchecked` | Conditions, helpers, discovery, or another boundary prevent a conclusion. |
| `render/content-omit-stale` / `-redundant` / `-ambiguous` / `-unresolved` | The targeted exception needs repair. |
| `render/content-policy-invalid` | The policy or target identifier is invalid. |

Discovery uses informational diagnostics, so it preserves existing warning
policies in `proa check` and lifecycle checks. Opt a reviewed scope into CI with
specific codes:

```bash
proa lint src/components/product \
  --deny render/content-coverage \
  --deny render/content-coverage-unchecked
```

Also deny the omission codes above when enforcing exception hygiene. Denied
informational codes are shown even when the display threshold is `warning`.
`--deny` changes exit status, not the printed severity. `--json` adds labels,
notes, and structured suggestions to the existing diagnostic fields. `--github`
includes counterpart locations and the same notes and repair guidance.

`--changed` and `--staged` select **reporting paths**. Analysis loads unchanged
counterparts from their enclosing Cargo packages. `--root` can select a broader
analysis root; it also retains its RSJS sidecar role. A diagnostic affecting a
pair is reported when either view is selected. `--staged` reads working-tree
source from staged paths, as the CLI's other source checks do; it is not an
analysis of Git index snapshots. A deleted or unresolvable module, or an
unexpanded item macro, prevents a confirmed stale-omission assertion when a
source view cannot be found. Git selection excludes deleted paths; use a full
source scan when the only change deletes a view. Standard Cargo
`src/lib.rs`, `src/main.rs`, and `src/bin/*.rs` roots are indexed. Custom Cargo
entry paths are analyzed locally; their discovery remains bounded.

## Supported analysis and boundaries

Supported sinks are registered render methods called with the output context,
including UFCS calls, local template aliases, `?`, `.await`, and known
`RenderOutcome::pending(async { ... })` outcomes. Supported expressions include
field/local aliases, struct/tuple destructuring, child struct literals and
conventional `Child::new(...)` constructors, HTML and shared Markdown semantic
parsing, conditions, and loops with resolved collection element provenance.
Constructor argument positions, content field assignments, and literal inputs stay
separate. Child `class`, `style`, and `on_*` fields are presentation inputs. Different child inputs are unchecked rather than assumed equivalent.

HTML content attributes include `alt`, `title`, `value`, `placeholder`, `href`,
`src`, `aria-label`, `aria-description`, and `aria-valuetext`. Styling and event
attributes do not create content obligations. Attribute values being included
elsewhere does not prove equivalent accessibility or link semantics. A
collection's length does not cover its rendered elements.

Known text and sync/async/Send adapters are transparent. General helpers,
transforms, mutation/control flow, direct context writes, or futures with
unknown invocation remain opaque. Forwarding input to a child does not prove
that the child renders it. Unnamed static output is partially checked because
syntax alone cannot establish a shared semantic role; static text cannot hide a
confirmed missing dynamic child. Braces in Markdown code stay literal, and
literal/named-argument source locations retain their authored positions.

For sibling redistribution (for example, severity moved from Impact to Overview),
acknowledge that specific contribution in the checked Impact view. The checker
compares component-local obligations; it does not prove whole-page equivalence.

## Add another target

The reusable API is `proa_cli::lint::content`. A `TargetDescriptor` registers
trait identities, entry methods, templates, transparent adapters, known future
sinks, and either an existing syntax or a custom parser producing semantic
output/control nodes. A format may also produce `ViewSummary` values directly
and call `compare_views`. Pairing, uncertainty, exceptions, and diagnostics do
not depend on HTML being special. Traits are explicitly registered; names ending
in `Render` are not automatically treated as output targets.

The compiled reference pair is
[`proa_macros/tests/content_coverage.rs`](../../proa_macros/tests/content_coverage.rs).
CI enforces its HTML → Markdown obligations and tests actual output. The linter
suite also registers a third target with a different grammar and tests omission
handling using the same comparison engine.
