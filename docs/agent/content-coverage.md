# Keep content across render targets

Run `proa lint src` to check content differences. The
initial policy compares HTML output contributions with Markdown for the same
component. It follows values into output: adding a field read, constructing a
child, or creating an unused template does not satisfy coverage.

This is a best-effort input coverage check. Passing a value to a rendered helper
counts as using it; the checker does not prove that the helper actually displays
it. It does not certify equivalent meaning, order, repetition, or accessibility.
Opaque output is informational. Definite omissions remain errors, and different
presence conditions remain warnings.
Rust continues to check field and child render types.

## Context and loaders are preparation

The check compares contributions that reach the rendered body. It does not
require matching `cx` accesses, queries, cache lookups, or loader calls between
views. Known `proa_core::WebContext` loader operations and request, response,
head, telemetry, and local-state setup do not create content obligations or
unchecked-body diagnostics by themselves. This also applies to supported aliases
of those context and metadata handles.

A loaded result only enters the comparison when it reaches a render sink.
An unused fetch cannot satisfy a missing component field or child. Matching
`cx.load_typed::<Payload>(key)` calls preserve field provenance across views,
even when their local variable names differ. The payload type, type arguments,
and supported key expression must match. Different queries or arbitrary loader
helpers are not assumed equivalent.

## Ordinary content needs no wrappers

Render fields and children directly. The checker also compares literal HTML
text and semantic attributes with Markdown text, links, and code. It normalizes
whitespace and removes Markdown formatting for this comparison; it does not
infer that differently worded copy means the same thing. Literal contributions are compared by text node or Markdown block. Keeping
shared copy together avoids false differences from differently split text.

Known Markdown layout operations such as `cx.md_block_boundary()?` do not
create content obligations. `cx.write_md_inline_code(value)` and
`cx.md_heading_with(level, |out| child.render_md(out))` retain the rendered
value or child's coverage. Standard `Some(value)`, optional field presence,
and `if let Some(value)` bindings are understood without annotations.

Direct writes and opaque helpers that receive the output context can remain
unchecked. These analysis limitations are informational and need no annotation.
The exemptions apply to recognized Proa APIs, rather than arbitrary methods named `load` or
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
cannot infer its concrete instantiation; use an existing child local instead of a type selector. If several distinct occurrences have different inputs or
presence conditions, use a local to narrow the exception.
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

## Helpers and shared inputs

Render ordinary fields, constants, and child components directly. The checker
follows aliases, dereferences, helper arguments, and method receivers into
rendered output. `text!()` contributes its literal text and rendered arguments.
Different helpers can consume the same shared value without annotations:

```rust
// HTML slot
{status_badge(self.stock)}

// Markdown slot
{status_text(self.stock)}
```

Both use `self.stock`. This is sufficient for input coverage, even though the
checker does not inspect the helpers or prove that they render equivalent text.
An unused helper result or a value read only in a condition does not count.

String selectors in `omit!()` select literal copy, such as
`omit!("Loading", "Visual loading indicator")`. Changing the source text makes
that exception stale. Use fields, child types, or borrowed locals for dynamic
content. No matching labels are required on the source renderer.

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

An explicit source policy pairs the two types by their rendered field names,
shared constants, literals, and child inputs. The author is responsible for
ensuring that corresponding fields mean the same thing.

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
value, child, or literal. Terminal output includes source frames in both files, a direct
cause, and a repair. Suggested omissions are scoped to an actual value or
resolvable child. They are suggestions requiring author judgment; the linter
never inserts an exception automatically.

| Code | Severity | Meaning |
| --- | --- | --- |
| `render/content-coverage` | Error | A supported rendered contribution is absent. |
| `render/content-coverage-unchecked` | Info / Warning | Opaque output is informational. Different presence conditions or ambiguous discovery remain warnings. |
| `render/content-omit-stale` / `-redundant` / `-ambiguous` / `-unresolved` | Info | The targeted exception needs repair. |
| `render/content-policy-invalid` | Error | The policy or target identifier is invalid. |

Missing contributions fail `proa lint` by default. Opaque-output information is
shown with `--min-severity info`, `--json`, or `--github`; it does not fail
`--deny-warnings`. Different presence conditions and ambiguous discovery remain
warnings. `proa check` and lifecycle source checks reject errors and warnings;
`--allow-warnings` permits warnings but still rejects missing content.
A targeted `omit!` marker acknowledges an intentional content difference.

```bash
proa lint src/components/product --deny-warnings
```

Paired, unpaired, and exempt discovery status remains informational. Omission
hygiene diagnostics also remain informational; deny their specific codes when
enforcing exception hygiene. Denied informational codes are shown even when the
display threshold is `warning`.
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

Known text and sync/async/Send adapters are transparent. General helpers and
transforms are compared by their content inputs. Mutation/control flow, direct
context writes, or futures with unknown invocation can remain opaque and are
reported as informational. Forwarding input counts as coverage without proving
that it is displayed. Static text cannot hide a confirmed missing dynamic child. Braces in Markdown code stay literal, and
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
