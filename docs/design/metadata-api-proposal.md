# Managed document head: implementation contract

Updated October 30, 2026. Implementation branch: `codex/managed-document-head`, based on main `fd3ccea96ae9feb19207185ef28b04f377757d84`.

Keep `WebRender::render` and `WebRenderSync::render`. Add optional head hooks on those traits and their Send counterparts. Ordinary components inherit `ROLE = ComponentRole::COMPONENT`; the selected page sets `ROLE = ComponentRole::PAGE`. No Page derive, additional route registry, returning-template API, or separate head trait is required.

```rust
impl<L: DataLoader> WebRender<L> for ProductPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    fn render_head(&self, cx: &mut WebContext<L>) -> impl HeadOperation {
        async move {
            let product = cx.load_typed::<Product>(product_key(self.id)).await?;
            cx.head_mut().title(product.name.clone())?;
            cx.head_mut().description(product.summary.clone())?;
            cx.head_mut().module(Script::Site, "/static/site.js")?;
            Ok(())
        }
    }

    // Existing render(self, cx) signature and implementation.
}
```

The document remains ordinary HTML, including fonts, classes and manual tags:

```rust
html! {
    <!DOCTYPE html>
    <html lang="en">
        <head>
            <meta charset="utf-8" />
            <meta name="viewport" content="width=device-width, initial-scale=1" />
            {cx.head()}
        </head>
        <body>{self.content}</body>
    </html>
}.render(cx).resolve().await
```

## Execution and publication

Macro and framework component dispatch runs each eligible hook before its body. Ancestor hooks supply defaults; deeper hooks override fields. The Page hook seals the head before its body. Plain trait `.render(cx)` calls remain body calls; they do not independently establish lifecycle dispatch. Tags and attributes remain direct buffer writes.

The default hook returns `NoHead`, an IntoFuture that deliberately is not a Future. Its type selects body-only dispatch without adding a hook future. Real hooks select the head-then-body strategy. This also lets dispatch reject an excluded hook before invoking application code or performing I/O.

`cx.head()` returns an owned outlet: obtaining it holds no context borrow and captures no values. Rendering it records an output offset. Neither outlet nor Page waits for the other. Once both are known, transfer the native 4 KiB pages and splice serialized head bytes at the recorded position. Publish prefix, head, retained suffix, then subsequent body bytes. Do not flatten the whole document or scan its HTML.

```mermaid
flowchart LR
    A[Ancestor head hook] --> B[Ancestor body]
    B --> C[Record head outlet]
    C --> D[Nested hooks]
    D --> E[Page head hook]
    E --> F[Seal and publish]
    F --> G[Page body and deferred content]
```

Managed mode is selected before constructing the response driver. Manual mode keeps the existing driver. Head and body share the request-scoped loader. Suspense, fragment captures and error/loading fallbacks are head-disabled, including ready/cached children. Raw loaders without a request cache do not gain memoization merely by being called from a hook.

## Review findings and selected mitigations

| Issue | Implementation decision | Required regression |
| --- | --- | --- |
| Original errors replaced by HookFailed/OrphanProvider | Preserve the original error and component identity in a cold, shared diagnostic. Poison the document without replacing its first cause. Only a successful provider returning without a Page is an orphan. | Hook and ancestor database errors retain source chains; ignored errors remain fatal; WriteError/native future sizes do not grow. |
| Empty head publication rejected or delayed | Ready is a control event independent of payload size. HTTP preparation observes it before waiting for body bytes. No synthetic HTML. | Empty head at offset zero returns status/headers while body remains pending, local and Send. |
| Detached context accepts ignored response changes | Detached response state is read-only even before parent publication. The primary context alone owns response decisions until Ready. | Header, status and cache changes fail in detached contexts; root decisions still reach HTTP. |
| Suspense clears held prefix on failure | Before publication use its buffered path; do not install the streaming whole-buffer cleanup guard. Sealed-without-outlet is still unpublished. Continue discovery without waiting on head readiness. | Failure, cancellation and unwind preserve prefix across page boundaries in local, Send and shared-writer Markdown paths. |
| Error recovery crosses head lifecycle | An ErrorBoundary may recover only within an unchanged head revision and before its own bytes are shipped. Hooks, Page selection and outlet/publication changes prevent rewind. Body-only fallbacks are required. | Named fallback diagnostics; reject directly identifiable Page fallbacks before invoking them; nested recovery and later siblings have explicit output assertions. |
| Native boundary storage regression | Keep lifecycle guards out of native ErrorBoundary and Suspense futures, using separate managed execution when needed. Measure latest-main baseline. | Allocation count/bytes, context/outcome/future size, poll count and output match on metadata-free paths. Measure latency separately; do not infer it from storage. |
| Readiness and transport ordering | A shared initial-shell pump drives managed local/Send in-order and out-of-order roots. Commit HTTP only at Ready. A newly published head yields once in async component dispatch before continuing body work; the pump delivers Ready first even if a source poll also fails. Release OOO completions after initial shell close; preserve RSJS tail ordering. | Early head with pending or immediately ready body; publication from Page seal and from an outlet inside the Page; same-poll failure after Ready; OOO boundary order, cancellation, backpressure and missing/duplicate Ready. |
| Unbounded preparation / deadlock | Sample the configurable preparation budget at outlet, sealing and explicit flush with a sink; include escaped head bytes. Never await capacity needed to discover a later Page. Suspense is not an implicit sample before publication. | Limit errors and deadline cancellation before Ready. Budget is sampled, not a hard peak-memory guarantee. |
| Unsafe output / origins | Escape text and quoted attributes separately. Validate patch values before mutation. Resolve URLs against trusted configured origin/mount; restrict resource origins. Nonce is initialization-only. | Quotes, closing-title input, non-ASCII, dangerous schemes, traversal, origin confusion, nonce and atomic failure. |

Before publication a failed Suspense child propagates to an enclosing ErrorBoundary, which replaces that whole buffered region. After publication an in-order Suspense fallback can replace only the failing child and retain later siblings. This difference is intentional and must be demonstrated in documentation/tests. Recovery never rewinds published bytes.

Managed lifecycle failures are not catchable by ordinary UI error fallback. Invalid field values are recoverable inside the hook and atomic. Failed serialization or publication never reopens the document. A new error document gets fresh state.

The publication yield guarantees **poll order**, not a scheduling barrier between
worker threads and the HTTP handler: a local or Send renderer may resume after
Ready is queued, before the handler returns its response. It cannot preempt
work before the outlet or an entire synchronous `WebRenderSync` / `html_sync!`
render, even when that work follows the outlet. Keep expensive body work in an
async component after the outlet or defer it through Suspense; page data needed
to determine the head still completes before publication.

## Fields, defaults and compatibility

Use owned/static text without public lifetime parameters. Initial shared defaults are reusable; child fields replace only fields explicitly written or cleared. Validate title templates to contain exactly one `%s`; the nearest ancestor template applies to a descendant title. Explicit social fields win, clears remain absent, and unspecified Open Graph title/description/URL fall back at emission to the final document fields. Structured image records replace as a list. Enum resource slots are keyed by enum type and value, with separate families; removal retains ordering.

Manual HTML is untouched. No metadata parser, tag deduplication, extra DOM pass, URL fetch, authentication policy or client-navigation runtime is introduced. Migrate existing route metadata values into defaults explicitly; do not run a second emitter. RSJS discovery continues through the body using its existing asset protocol. Custom writers must support owned segments and fail setup before rendering otherwise.

The initial configurable preparation limit is 1 MiB; serialized head 64 KiB; input value 16 KiB; resource/meta slots 128 including tombstones. Header commitment occurs at publication, not merely at sealing. Deferred mutations cannot change HTTP decisions. HEAD and bodyless statuses suppress output; transport owns framing headers. Deadline cancellation must stop both local and Send sources.

## Completion criteria

The implementation covers the framework/Axum drivers, typed fields/defaults, streaming and RSJS integration, response/cache decisions, MSRV and compile-fail checks. See [validation results and remaining limits](managed-head-validation.md) for measured evidence. Native-path microbenchmarks are not a universal zero-overhead guarantee.

## Implementation notes from the production port

The measured native ErrorBoundary and local/Send Suspense regressions are removed.
The same release probe source against latest main and this branch reports identical
allocation counts/bytes, returned-outcome sizes and poll counts for ready, pending,
recovered and in-order cases. These are storage measurements, not a universal latency claim.

ErrorBoundary reuses its existing transaction id to keep head revision checkpoints
in managed document state. This adds no field to native futures. Active managed
recovery scopes are capped at 1024. Suspense exclusion is entered/exited at existing
return points, without an additional async frame or retained optional guard.
Cancellation/unwind leaves preparation invalid; finalization rejects it. Detached
contexts are already permanently excluded and do not increment the root's exclusion
counter or hold its recovery checkpoints while deferred body work waits.

App defaults are immutable shared values. Configuring a request shares them; an
actual hook mutation uses copy-on-write, with static strings borrowed and owned
text/resource keys shared. Native requests do not allocate this state.

The portable managed pump wraps both existing delivery drivers, observing Ready
while the initial root awaits. Empty Ready is an explicit readiness state; the
adapter does not invent HTML or an empty body frame. In-order and out-of-order,
local and Send paths are covered through the actual Axum response preparation.
The native driver remains selected when managed mode is absent. Per-response
`without_head()` overrides app-level managed defaults.

### Response integration decisions

Managed client-navigation responses retain the existing JSON protocol. Its title
comes from the resolved head, including an explicit clear; status, cache headers
and cookies survive finalization. This is server payload integration, not a new
browser head-merging algorithm. Native navigation behavior is unchanged.

Buffered and streaming managed responses suppress bodies for 204/205/304, reject
informational final statuses and discard application-supplied framing headers.
A deferred head cannot loosen an existing middleware `private`, `no-cache` or
`no-store` policy; conflicting cache-policy overrides become `private, no-store`.
This applies to browser and supported CDN cache-control headers. Vary and cookies
accumulate. The preparation deadline defaults to 30 seconds; the Axum response
extension `ManagedPreparationTimeout` overrides it before RouteContextLayer runs.

Raw writer clearing/replacement by application code is outside the managed writer
contract. Custom writers without owned-page transfer are rejected during setup.
Defaults resolve URLs when configured: configure the trusted origin/mount first,
then build defaults. Rebuild defaults when switching deployment origin. No origin
is inferred from Host or forwarded headers, and metadata resources are never fetched.

Known boundaries remain deliberate: the Page role is static, but whether its branch
runs and whether a hook is late are runtime facts. A directly identifiable Page
fallback is rejected before the protected child runs; erased/wrapped descendants
still require runtime exclusion. Cancellation invalidates preparation rather than
rolling arbitrary component effects back. The preparation budget is sampled, not
an allocator-level hard cap. Send Suspense does not acquire a new ErrorBoundary API;
its existing error propagation remains in place.

### Macro fast path

The generated child boundary handles an already-completed render directly:

```rust
match proa_core::head_dispatch::begin(child, cx) {
    RenderOutcome::Done(result) => result?,
    RenderOutcome::Pending(future) => future.await?,
}
```

This is internal macro expansion, not another application render method.
Type-directed dispatch omits the hook future for `NoHead`. Avoiding an extra
`resolve()` future also reduces storage on ordinary async composition. Tags and
attributes keep their existing direct writes. See the measured before/after data
in the validation report.
