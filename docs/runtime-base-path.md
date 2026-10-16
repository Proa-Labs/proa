# Runtime base path: plan and design

Addresses #36 and #37: a proxy can expose `/apps/demo/docs/start` while
forwarding `/docs/start` to Proa. Root-absolute asset URLs and redirects then
escape the mount and islands fail to hydrate.

## Options considered

| Approach | Benefit | Cost |
| --- | --- | --- |
| Explicit startup base path, typed URL composition, and asset resolver | Works at any page depth; preserves streaming; one deployment setting | Application-written URLs use a helper; generated JS needs matching imports |
| Relative URLs or an HTML base element | Small template-only change | Nested routes and trailing slashes change resolution; a base element changes all relative navigation; ESM imports use module URLs |
| Response rewriting middleware | Can retrofit existing markup | Must parse arbitrary/chunked HTML correctly; cannot fix JS imports on its own; may buffer streaming responses |

Choose explicit startup configuration. It reuses RSJS's existing URL resolver
and import rewriter without parsing application output or changing routing.

## Implementation sequence

1. Add a validated, shared `BasePath` and allocation-free URL attribute values;
   attach the setting to transport-independent request metadata.
2. Configure the Axum builder/layer, handler URL helper, framework-owned URLs,
   and every buffered/local/Send streaming RSJS finalization path.
3. Add opt-in linked RSJS serving. Rewrite generated module runtime imports once
   at startup, keeping sibling dependencies relative and incoming paths local.
4. Wire runtime configuration into site/application scaffolds and deployment
   examples. Document explicit handling for application-authored URLs.
5. Verify prefix normalization, nested links/forms/redirects, dependency asset
   types, deferred hydration, stream delivery before completion, default-root
   compatibility, and render allocation behavior; open a draft PR with an example.

## Contracts

- Prefix selection is explicit at startup; forwarded headers are not trusted or
  inferred. Empty or `/` means origin root. Normalize one trailing slash and
  reject malformed or browser-normalized mount paths.
- The proxy strips the prefix. This setting never nests or rewrites incoming
  application routes. Deployments that preserve the prefix can nest the built
  router separately and configure the public base path to match.
- URL helpers take unprefixed application paths. External URLs, protocol-relative
  URLs, document-relative paths, queries, and fragments pass through.
- Component URL values share the prefix and write escaped segments directly.
  They never allocate an intermediate URL string. HTTP redirects may materialize
  one string at the response boundary.
- The captured request metadata carries the prefix into streaming boundaries;
  response finalization never depends on a live Axum task-local context.
- RSJS assets are opt-in so applications with existing routes do not collide.
  Custom asset handlers must apply the import rewriter themselves.
- Static CSS/JS content and third-party/raw HTML remain application-owned. A
  base path does not rewrite arbitrary literals or turn a URL into trusted input.
