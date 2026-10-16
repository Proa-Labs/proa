# Runtime base path: design note

Provides application URL composition for #36 and island asset loading for #37:
a proxy can expose `/apps/demo/docs/start` while
forwarding `/docs/start` to Proa. Root-absolute asset URLs and redirects then
escape the mount and islands fail to hydrate.

## Options considered

| Approach | Benefit | Cost |
| --- | --- | --- |
| Explicit startup base path, typed URL composition, and asset resolver | Works at any page depth; preserves streaming; one deployment setting | Application-written URLs use a helper; generated JS needs matching imports |
| Relative URLs or an HTML base element | Small template-only change | Nested routes and trailing slashes change resolution; a base element changes all relative navigation; ESM imports use module URLs |
| Response rewriting middleware | Can retrofit existing markup | Must parse arbitrary/chunked HTML correctly; cannot fix JS imports on its own; may buffer streaming responses |

The implementation uses explicit startup configuration. It reuses RSJS's
existing URL resolver and import rewriter without parsing application output
or changing routing.

## Architecture

1. A validated, shared `BasePath` produces allocation-free URL attribute values
   and lives in transport-independent request metadata.
2. The Axum builder/layer and handler URL helper share the setting with
   framework-owned URLs and buffered/local/Send streaming RSJS finalization.
3. Opt-in linked RSJS serving rewrites generated runtime imports once at
   startup, keeping sibling dependencies relative and incoming paths local.
4. The basic Axum site, including its docs/blog shell, the application scaffold,
   and ORIGIN read the startup setting. ORIGIN uses the shared linked RSJS router.
5. Font heads compose preload URLs and stream the generated CSS's known face
   URL slots with CSS-escaped prefixes. The root fast path keeps static CSS.
   Docs shell navigation uses request URL values; its adapter keeps the mount
   separate from the collection's build-time route.

Focused tests cover prefix normalization, nested links/forms/redirects, linked
asset types, deferred hydration, incremental streaming, default-root
compatibility, custom adapter emission, and render allocation behavior.

## Contracts

- Prefix selection is explicit at startup; forwarded headers are not trusted or
  inferred. Empty or `/` means origin root. Normalize one trailing slash and
  reject malformed or browser-normalized mount paths.
  Validation checks mount structure and normalization hazards, rather than
  enforcing the complete RFC 3986 character grammar; output contexts still
  perform their own escaping.
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

## Coverage limits

- Authored URLs inside compiled docs article HTML, arbitrary application CSS/JS,
  and raw third-party markup still require application-level configuration.
- Query/mutation endpoint URLs are application data. Prefixing the query runtime
  script does not prefix `/api/...` fetchers or provide a browser base-path API.
- The development reload EventSource still requests `/__proa/dev-events`.

See [deployment guidance](../proa_marketing/src/docs/framework/deployment.mdx#coverage-and-limitations)
for the resulting proxy and application requirements.
