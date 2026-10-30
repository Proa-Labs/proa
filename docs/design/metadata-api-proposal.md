# Managed document head: implementation contract

Updated October 30, 2026. Implementation branch: `codex/managed-document-head`, based on main `ee29048e42d032d80f232444c06c2d592cb06e83`.

Keep `WebRender::render` and `WebRenderSync::render`. A head hook on those traits
or their Send counterparts identifies the selected Page. Ordinary components and
layouts inherit `NoHead`; there is no `ROLE` constant, Page derive, additional
route registry, returning-template API or separate head trait.

```rust
impl<L: DataLoader> WebRender<L> for ProductPage {
    #[allow(refining_impl_trait)]
    async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let product = cx.load_typed::<Product>(product_key(self.id)).await?;
        cx.head_mut().title(product.name.clone())?;
        cx.head_mut().description(product.summary.clone())?;
        cx.head_mut().module(Script::Site, "/static/site.js")?;
        Ok(())
    }

    // Existing render(self, cx) signature and implementation.
}
```

The scoped Rust lint allowance acknowledges that `async fn` returns a Future,
while the trait also supports the non-Future `NoHead` absence marker. No wrapper
block is required. Explicit `fn ... -> impl HeadOperation` remains supported;
synchronous hooks return `impl SyncHeadOperation` with direct mutations and a
`Result`. An empty successful hook identifies a Page that only inherits defaults.

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

Macro and framework component dispatch identifies the Page from its head operation type, without invoking it. Shared defaults come from `HeadConfig`; the Page overrides fields in its hook and seals the head before its body. Layouts have no hooks. A nested or second Page is rejected before its hook can run. Plain trait `.render(cx)` calls remain body calls; they do not independently establish lifecycle dispatch. Tags and attributes remain direct buffer writes.

The default hook returns `NoHead`, an IntoFuture that deliberately is not a Future. Its type selects body-only dispatch without adding a hook future. Real hooks select the Page strategy. The dispatch owns a cancellation guard before returning a pending Page operation, including before its first poll. This also lets dispatch reject an excluded hook before invoking application code or performing I/O.

`cx.head()` returns an owned outlet: obtaining it holds no context borrow and captures no values. Rendering it records an output offset. Neither outlet nor Page waits for the other. Once both are known, transfer the native 4 KiB pages and splice serialized head bytes at the recorded position. Publish prefix, head, retained suffix, then subsequent body bytes. Do not flatten the whole document or scan its HTML.

```mermaid
flowchart LR
    A[App defaults] --> B[Ordinary document body]
    B --> C[Record head outlet]
    C --> E[Page head hook]
    E --> F[Seal and publish]
    F --> G[Page body and deferred content]
```

Managed mode is selected before constructing the response driver. Manual mode keeps the existing driver. Head and body share the request-scoped loader. Suspense, fragment captures and error/loading fallbacks are head-disabled, including ready/cached children. Raw loaders without a request cache do not gain memoization merely by being called from a hook.

## Review findings and selected mitigations

| Issue | Implementation decision | Required regression |
| --- | --- | --- |
| Original errors replaced by HookFailed/OrphanProvider | Preserve the original error and component identity in a cold, shared diagnostic. Poison the document without replacing its first cause. A successful Page always seals before its body; completion guards retain the first failure. | Hook and Page body database errors retain source chains; ignored errors remain fatal; WriteError/context/outcome sizes do not grow; record pending-future storage costs. |
| Empty head publication rejected or delayed | Ready is a control event independent of payload size. HTTP preparation observes it before waiting for body bytes. No synthetic HTML. | Empty head at offset zero returns status/headers while body remains pending, local and Send. |
| Detached context accepts ignored response changes | Detached response state is read-only even before parent publication. The primary context alone owns response decisions until Ready. | Header, status and cache changes fail in detached contexts; root decisions still reach HTTP. |
| Suspense clears held prefix on failure | Before publication use its buffered path; do not install the streaming whole-buffer cleanup guard. Sealed-without-outlet is still unpublished. Continue discovery without waiting on head readiness. | Failure, cancellation and unwind preserve prefix across page boundaries in local, Send and shared-writer Markdown paths. |
| Error recovery crosses head lifecycle | An ErrorBoundary may recover only within an unchanged head revision and before its own bytes are shipped. Hooks, Page selection and outlet/publication changes prevent rewind. Body-only fallbacks are required. | Named fallback diagnostics; reject directly identifiable Page fallbacks before invoking them; nested recovery and later siblings have explicit output assertions. |
| Native boundary storage regression | Keep lifecycle guards out of native ErrorBoundary and Suspense futures, using separate managed execution when needed. Measure latest-main baseline. | Allocation counts, context/outcome sizes, poll counts and output match on metadata-free paths; record any allocated-byte differences. Measure latency separately; do not infer it from storage. |
| Inherited Page cancellation | A defaults-only Page supplies an empty hook and owns a completion guard. Keep that guard through the managed continuation; synchronous Page bodies also invalidate preparation on unwind. | Cancelling local, macro and Send dispatch before or after publication is sticky; dropping an unpolled Page continuation and synchronous/asynchronous unwind are rejected; successful Pages can publish at a later outlet. |
| Deferred fallback ignores a lifecycle failure | Recheck terminal head state after the error fallback and before committing its artifacts. | An ignored mutation in the last out-of-order fallback terminates the body, matching in-order behavior; a valid fallback still succeeds. |
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

Use owned/static text without public lifetime parameters. Initial shared defaults are reusable; the Page replaces only fields explicitly written or cleared. Configure title templates in defaults with exactly one `%s`; that template applies once when the Page sets its title. Explicit social fields win, clears remain absent, and unspecified Open Graph title/description/URL fall back at emission to the final document fields. Structured image records replace as a list. Enum resource slots are keyed by enum type and value, with separate families; removal retains ordering.

Manual HTML is untouched. No metadata parser, tag deduplication, extra DOM pass, URL fetch, authentication policy or client-navigation runtime is introduced. Migrate existing route metadata values into defaults explicitly; do not run a second emitter. RSJS discovery continues through the body using its existing asset protocol. Custom writers must support owned segments and fail setup before rendering otherwise.

The initial configurable preparation limit is 1 MiB; serialized head 64 KiB; input value 16 KiB; resource/meta slots 128 including tombstones. Header commitment occurs at publication, not merely at sealing. Deferred mutations cannot change HTTP decisions. HEAD and bodyless statuses suppress output; transport owns framing headers. Deadline cancellation must stop both local and Send sources.

## Completion criteria

The implementation covers the framework/Axum drivers, typed fields/defaults, streaming and RSJS integration, response/cache decisions, MSRV and compile-fail checks. See [validation results and remaining limits](managed-head-validation.md) for measured evidence. Native-path microbenchmarks are not a universal zero-overhead guarantee.

## Implementation notes from the production port

The same release probe source against main and this branch reports identical
allocation counts, returned-outcome sizes and poll counts for native ready,
pending, recovered and in-order cases. Inferred Page identity removes the
ordinary dispatch's Page branch, restoring native ErrorBoundary and local
in-order Suspense allocated bytes to main's values. Macro composition retains
its lower allocated-byte totals. See the validation report for exact counts and
separate latency measurements.

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
This applies across browser and supported CDN cache-control headers, including
`Surrogate-Control`: a restriction in one family constrains policies introduced
or replaced in any other family. Vary and cookies
accumulate. The preparation deadline defaults to 30 seconds; the Axum response
extension `ManagedPreparationTimeout` overrides it before RouteContextLayer runs.

Raw writer clearing/replacement by application code is outside the managed writer
contract. Custom writers without owned-page transfer are rejected during setup.
Defaults resolve URLs when configured: configure the trusted origin/mount first,
then build defaults. Rebuild defaults when switching deployment origin. No origin
is inferred from Host or forwarded headers, and metadata resources are never fetched.

Known boundaries remain deliberate: the Page hook capability is static, but whether its branch
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
Type-directed dispatch omits the hook future for `NoHead`. Ordinary components
return their existing `Done` or `Pending` outcome directly; they retain no Page
guard or Page continuation. Actual hooks select the Page strategy, which seals
metadata and yields after streaming publication before constructing the body.
The hook guard is acquired before returning the Page outcome, so dropping an
unpolled operation or unwinding invalidates preparation. Its completion guard
remains live through the body, including before an outlet exists.

Synchronous adapters preserve the operation marker: adapting an ordinary sync
component does not turn it into a Page, while adapting a sync Page preserves its
hook. Fallback validation uses the same inferred capability before executing the
protected child. Tags and attributes keep their existing direct writes. See the
validation report for measured native-path costs.
