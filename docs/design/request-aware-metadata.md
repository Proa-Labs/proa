# Request-aware metadata

Metadata is resolved for a request before rendering its document. A route
pattern identifies a provider stack, not a globally valid set of head tags.
Providers can read params, decoded search params, headers, cookies, location
and the public base path, await the page's loader, and inspect their parent.

## Provider API

`Metadata` retains its existing getters as a compatibility adapter and adds
three entry points. Implement one:

| Method | Inputs | Resolution |
| --- | --- | --- |
| `static_metadata() -> Option<MetadataPatch>` | none | explicitly request independent; eligible for startup evaluation |
| `metadata(&MetadataContext, &ResolvedMetadata) -> MetadataPatch` | request and merged parent | immediate, once per render |
| `generate_metadata(MetadataContext, &ResolvedMetadata) -> MetadataOutcome` | request, merged parent and shared loader | immediate or asynchronous, once per render |

The default `generate_metadata` calls `metadata`. The default `metadata`
adapts `static_metadata` or the old getters. A provider declaring static
metadata promises its result needs neither the request nor the parent.
Old getters work per request until the provider explicitly opts into static
precomputation. `TitleOnly` opts in automatically.

`MetadataContext` borrows the render request and the exact loader instance
installed on that response. Typed, JSON and byte loading support full `LoadRequest` variants for hints. Providers are heterogeneous
route trait objects: their loader calls deliberately cross `DynDataLoader`.
Page loader calls retain concrete `L`. Install a fresh
`DataResolver::new_typed` per request to deduplicate metadata and page loads;
supplying an arbitrary loader alone does not promise memoization. No hidden
second loader or global metadata cache is created.

`MetadataOutcome::ready` has no boxed future. `pending` carries a Send future;
`pending_local` permits local futures on ordinary `WebRender` responses.
Send responses convert the outcome synchronously and reject local futures
before rendering. This preserves the render capability distinction without
requiring all metadata futures to be Send. A synchronous response rejects
pending outcomes with `AsyncInSyncResponse`; use an async response (with
`to_async` for a sync page) when its metadata awaits data.

## Values and cascade

`MetadataPatch` contains `Cow<'static, str>` text, a validated `MetadataBase`,
canonical URL and `OpenGraphPatch`. Open Graph text and media descriptors
accept owned text too. Literal text and explicitly static slices stay
borrowed. Dynamic strings belong to the request and are freed with it.

`CascadeValue::Unset` inherits, `Set` replaces, and `Tombstone` clears. Lists
also replace: `Set([])` clears, and extending a list requires explicitly
copying the parent's list into the new patch. Title templates retain the
existing `%s` syntax. Absolute titles ignore and clear inherited templates;
tombstones clear templates so fallback cannot resurrect a removed field.

One `MetadataState::apply` implementation backs startup and request resolution.
Each provider receives a snapshot of the preceding cascade, including title
template fallbacks. Parent URL references remain unnormalized during the
cascade; URL resolution happens after the last patch so a child's metadata
base applies consistently to the final document.

## Static prefixes and dynamic routes

`ComputedRouteMeta` is a resolution plan: cached cascade state and a remaining
provider suffix, shared through `Arc` when the plan is cloned. Route construction evaluates only the contiguous static
prefix. It never calls a dynamic provider, or a provider following one, with
invented request inputs. Request resolution starts from that prefix and walks
the suffix in order. The route snapshot selects the plan using the matched
route pattern, so `/blog/:slug` reuses its static prefix for every concrete
slug without caching any slug's generated values. Routes assembled without a
precomputed map build the same plan from their installed provider stack.

## URL and deployment policy

URL resolution chooses the final provider's metadata base, then a validated
request `MetadataBase` extension, then the origin of an absolute request URI
plus its `BasePath`. `FrameworkBuilder::with_metadata_base` and
`RouteContextLayer::with_metadata_base` configure a deployment default;
request middleware may override it with a validated extension. An explicit
base includes the public mount prefix and is treated as a directory.

For base `https://staging.example/app/`, both `/og.png` and `og.png` become
`https://staging.example/app/og.png`. Absolute HTTP(S) URLs are retained;
protocol-relative references use the selected scheme. Dot segments and
query/fragment references follow URL joining rules. Canonical, OG URL,
image, secure-image, audio and video URLs all use the same policy.

An origin-form HTTP request has no scheme. Proa does not assume HTTPS from
`Host` or trust arbitrary forwarding headers. Set a deployment base from
configuration, or derive a validated base in middleware using the deployment's
trusted proxy policy. Relative URLs without a base fail explicitly.

## Render and response lifecycle

Every sync, local async and Send async render entry resolves metadata after
installing request state and the loader, before invoking the root component.
Both in-order and out-of-order streaming resolve before producing body bytes.
An early metadata error therefore uses the adapter's existing first-item
error gate. This deliberately blocks the initial head on metadata; streaming
metadata into an already-open document is outside this change.

The resolved result is an `Arc<ResolvedMetadata>` in portable request
extensions. Context copies and deferred boundaries retain that result without
task-local lookups or regeneration. `metadata_from_context(cx)` reads it;
head helpers render it with attribute/text escaping and no string formatting.
Client navigation obtains its title from that same render's resolved result.

Page caches still cache whole responses under their explicit policy. Metadata
that depends on headers must use matching cache variation or private/no-store
responses, just like request-dependent page content.

## Migration and scope

- Existing `Metadata` getter implementations continue to work. Migrate to one
  patch-returning method, opting into `static_metadata` only for truly static
  providers.
- `ResolvedMetadata`, resolved Open Graph fields and media text use `Cow`.
  Construct dynamic values with `.into()` and borrow with `.as_deref()`.
- Use `OpenGraphPatch` for dynamic metadata. `CascadeOpenGraph`, `OpenGraph`,
  `OpenGraphImage` and `OpenGraphMedia` remain compatibility descriptors.
- `ComputedRouteMeta` exposes its static prefix through dereferencing, but is
  no longer a public field-by-field literal. Construct it with `cascade`, and
  call `resolve_sync`, `resolve` or `resolve_send` for a real request. The
  compatibility `resolved_metadata_from_computed` returns only that prefix.
- Prefer `metadata_from_context(cx)` over the legacy no-argument
  `resolved_metadata()`. The legacy accessor can only run immediate providers
  without a loader; it cannot represent async generation or deferred renders.
- Lists replace instead of implicitly appending. Append explicitly using the
  parent snapshot.

This branch starts from main independently of the Twitter Cards contribution.
It adds no Twitter-specific fields and does not modify that contribution.
New metadata families can use the same patch and resolution machinery later.

Reference: [Next.js metadata and OG images](https://nextjs.org/docs/app/getting-started/metadata-and-og-images).
