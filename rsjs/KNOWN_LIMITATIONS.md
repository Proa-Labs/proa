# rsjs known limitations (v0.1)

Limitations of the current `#[rsjs(component...)]` analyzer + runtime that have been
hit by real consumers (`proa_demo`, `rsjs_macros::tests`). Each entry
captures the specific compile error or observed behavior, the use
case that surfaced it, the current workaround, and a sketch of the
shape that would close the gap.

These are *user-visible feature gaps*, not internal TODOs. Read
[`STATUS.md`](./STATUS.md) for the overall implemented/remaining surface and
[`../docs/agent/rsjs-islands.md`](../docs/agent/rsjs-islands.md) for supported
authoring. This file records what does not work yet when using RSJS today.

---

## 1. Broader browser-API escape hatch in handlers

**Blocks**: calling `fetch`, WebCrypto, arbitrary `window` /
`document` APIs, or any other browser API that is not part of rsjs'
whitelisted handler IR.

**Now supported**: `rsjs::window::{scroll_to,scroll_by}(...)`,
`rsjs::window::print()`, `rsjs::location::{href,origin,protocol,host,hostname,port,pathname,search,hash}()`,
`rsjs::location::set_hash(...)`, `rsjs::location::{assign,replace}(...)`, `rsjs::location::reload()`,
`rsjs::history::push_state(...)`, `rsjs::history::replace_state(...)`,
`rsjs::history::{back,forward}()`, `rsjs::history::go(...)`,
`rsjs::document::dispatch_event(...)`,
`rsjs::document::dispatch_event_with_detail(...)`,
`rsjs::document::set_title(...)`, `rsjs::clipboard::write_text(...)`,
`rsjs::local_storage::{get_item,get_item_or,set_item,remove_item}(...)`, and
`rsjs::session_storage::{get_item,get_item_or,set_item,remove_item}(...)`.

### Observed behavior

`lower_handler` accepts rsjs IR statements: signal writes,
handler-ref/callback invocations, typed event operations such as
`event.prevent_default()` / `event.value()`, built-in `console::*`
calls, whitelisted browser effects, and helpers declared via
one `#[rsjs::client]` annotation on each helper definition. Calls use ordinary
Rust imports, aliases, and re-exports; the semantic linker proves the resolved
definition.

Arbitrary browser API access still fails analysis because raw DOM
globals and JS-side assignments are intentionally not part of the
rsjs handler IR.

### Use case that hit it

`proa_demo`'s eBay filter chips previously needed each chip's
`on_click` to:
1. Toggle the chip's local boolean signal
2. Write the new combined filter state to `location.hash` so the URL
   becomes shareable + browser-back/forward works

### Current workaround

Location reads, hash writes, full-page navigation/reload, history writes,
custom events, title updates, clipboard writes, local/session storage reads,
writes, and removals, and window scroll/print effects can now happen directly
via the whitelisted helpers. More complex browser APIs still need a separate
vanilla-JS coordinator until RSJS exposes an explicit typed capability.
Origin-free `#[rsjs::client]` helpers can extend portable computation, but
cannot access raw browser globals or perform browser effects.

For APIs that are still outside the IR, the coordinator is external
to the rsjs reactivity graph: it cannot subscribe to chip signals and
often has to observe DOM state instead.

### Suggested fix shapes

Two options, in increasing order of ambition:

**a. Continue growing the allowlisted browser-API surface inside
handlers.** `window.{scrollTo,scrollBy,print}(…)`,
`location.{href,origin,protocol,host,hostname,port,pathname,search,hash}`,
`location.hash`, `location.{assign,replace,reload}(…)`,
`history.{pushState,replaceState,back,forward,go}(…)`,
`document.dispatchEvent(…)`, `document.title`, clipboard writes, and
local/session storage reads/writes/removals are now first-class helpers. Future
additions should follow the same shape when they are small, stable browser APIs
with clear fallback semantics.

**b. Generic "client escape hatch" attribute.** Annotate a function
with `#[rsjs::client_only]` and let handlers call it freely. The
function body is parsed as JS-flavored Rust (or just opaque tokens
for JS pass-through). Lets users write arbitrary client-side code
without growing `lower_handler`'s recognizer for every new browser
API.

Closing the broader escape hatch would let more URL-driven and
browser-integration coordinator patterns collapse into the island
itself, removing external runtime scripts.

Filed for M11+.

---

## Adding to this file

When you hit a limitation that blocks a real use case, add a section
here with the same shape (compile error / use case / current
workaround / suggested fix). Keep entries factual — the use case
documents *what someone tried to do*, not *what we should build*.
The fix sketch is just enough that whoever picks this up next can
recognize the shape without re-discovering the gap.
