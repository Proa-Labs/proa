# Changelog

All notable public changes are tracked here.

## Unreleased

- **Breaking:** renamed `rsjs::computed` / `Computed<T>` to `rsjs::memo` /
  `Memo<T>` and the browser runtime export to `memo`, without compatibility
  aliases. Memos remain read-only cached derivations with compiler-inferred
  dependencies and batched recomputation.
- **Breaking:** RSJS debounce now takes the handler first:
  `rsjs::debounce(|event| ..., delay_ms)` and runtime `debounce(fn, delay_ms)`.
  The analyzed delay remains an integer literal; trailing-edge behavior and
  cancellation on island teardown are unchanged.
- HTML and Markdown now share `WebContext<L, B>`. `MdRenderSync` and `MdRender`
  take loader-first generics, default to `NoLoader` and `RenderBuffer`, and no
  longer require context or render-method lifetimes. `MdContext` is removed
  without a compatibility alias. See [the migration guide](docs/migrations/web-context.md).
- Markdown supports the same request loader and metadata as HTML, including
  loader-aware in-order streaming and deferred Axum responses. Format-specific
  helpers use `md_*` / `write_md_*` names; `render_html_in_md` tracks HTML output
  through the live context without allocating another root.
- Buffered Markdown HTTP responses transfer owned render pages without flattening
  multi-page output and preserve response status.
- HTML and Markdown response finalization now retain every value of repeated
  headers such as `Link` and `Vary`. Component headers replace framework defaults
  once per name; `Set-Cookie` values continue to accumulate.
- The Airbnb home showcase's Markdown listing details now escape interpolated
  locations and ratings consistently with other Markdown component output.

- **Breaking:** `RouteResponse` is now an opaque deferred-render plan with a
  synchronous plan and one async plan whose delivery policy is buffered or
  streamed. The Axum type is a thin wrapper over `proa_framework_core` instead
  of a duplicated enum. The public `Ssr`, `SsrAsync`, `SsrStream`,
  `StaticHtml`, and `ClientSide` variants and the `static_html` / `csr`
  constructors were removed; keep using `ssr`, `ssr_async`, and `ssr_stream`,
  return ordinary Axum HTML responses for already-rendered content, and mount
  browser-rendered applications with `ExternalApp::csr_app`. The corresponding
  static/client finalizer helpers and the internal `StreamRoute*` responder
  surface were removed as well. `with_render_mode` now maps `Static` and
  `Agent` to buffered delivery and both `Chunked*` modes to streamed delivery,
  so mixed render/delivery states cannot be constructed.
- **Breaking:** route `Layout` values now provide cascading metadata only and
  no longer rewrite responses through `Layout::wrap`. Compose shared page
  chrome as a typed render component (for example `RootDocument`) so it applies
  uniformly to sync, buffered async, and streaming pages.
- **Breaking:** renamed the Axum/Tokio adapter crate from `proa_framework` to
  `proa_framework_axum`. Update Cargo dependency keys, Rust imports, feature
  references, and package-selecting commands accordingly. The portable
  `proa_framework_core` contract remains runtime- and HTTP-framework-agnostic.
- Removed the legacy unpublished `proa_framework_example` crate. The Axum
  adapter's integration tests, CLI scaffold tests, and maintained applications
  now provide its former coverage without a separate framework example crate.
- **Breaking:** RSJS event handlers now preserve their event parameter in an
  `EventBinding<E>` instead of erasing it to `fn()`. Template event families,
  concrete current targets, and local or inbound `NodeRef<T>` bindings are
  checked by rustc against the generated `rsjs_web` DOM marker and capability
  schema. Existing `rsjs::elem::*` and root node-ref marker paths remain
  compatibility aliases, while new code can use `rsjs::web::dom::*`.
- **Breaking:** `on_document_*` and `on_window_*` handlers now use exact
  non-element `rsjs::web::dom::Document` and `Window` current-target markers.
  Unparameterized event handles default to the element-only `dom::Any` marker
  and are rejected on global bindings; annotate those handlers explicitly.
  Their runtime current target is the actual global object, while the source
  element remains available internally for repeated-handler payload resolution.
- `#[rsjs::client]` inline modules can now define closed, scalar-field value
  objects with constructors and borrowed inherent methods. The same typed
  Client IR can be exposed explicitly as JavaScript classes with
  `#[rsjs::client(class)]`; class assets are registered separately from island
  hydration and retain checked integer and Rust copy semantics.
- Primitive float rendering now uses Zmij for shortest-round-trip output, while
  `text!("{:.N}", value)` selects an exact, allocation-free fixed-decimal fast
  path for concrete `f32` and `f64` values. General `Display` formatting and
  uncommon numeric cases retain the compatible streamed formatter fallback.
- **Breaking:** fonts are now declared in code only. `google_font!` works from
  a plain `build.rs` collector call: `proa_fonts_build` gained a default-on
  `fetch` feature that vendors Google sources at build time, gated by
  `proa-fonts.lock` so unchanged declarations rebuild offline from the
  committed vendored files, with integrity checks and cleanup of files whose
  declaration disappeared. The macro grammar gained `preload` and `license`.
  `cargo proa font add`/`remove` and `proa-fonts.toml` were removed: declare
  fonts at the call site instead; `cargo proa font sync` now runs the same
  collector outside a build (`--offline` verifies the lock, for CI), and
  `cargo proa font list` still prints the lock. The lock schema now records a
  declaration fingerprint and per-face metadata; pre-existing locks are
  treated as stale and re-vendored once.
- **Breaking:** relicensed the repository from `AGPL-3.0-only` to
  `Apache-2.0`, copyright 2026 Proa Labs, Inc. Every crate inherits the single
  root `LICENSE` through the workspace `license-file` field, and all
  `SPDX-License-Identifier` headers now say `Apache-2.0`. Commercial use,
  including inside closed-source products, no longer requires a separate
  agreement, so the Community Commercial and Proa Professional draft licenses
  and their `/legal/*` routes were removed.
- **Breaking:** removed the experimental `proa_macros::mdx!` proc macro. Use
  `md_sync!` or `md!`, with one multiline `r###"..."###` Markdown template as
  the canonical authoring form. Source newlines render directly, common Rust
  indentation is stripped, and `{expression}` composes typed children while
  braces in fenced and inline code remain literal. The expression-visible token
  form remains supported; its top-level source lines establish context-aware
  Markdown block boundaries. Bridge validated inline HTML with
  `MdContext::render_web(html_sync! { ... })`. The `proa_docs_build` `.mdx`
  document pipeline is unchanged.
- `proa fmt` now normalizes multiline raw `md_sync!` / `md!` template
  indentation and places the closing macro brace on its own aligned line.
  `proa lint` reports drift as `format/markdown-layout`, while formatter verify
  mode compares dedented Markdown semantics before allowing a rewrite.
- **Breaking:** removed `proa_core::RenderOptions` and the options argument
  from the synchronous, asynchronous, and Markdown root render helpers.
  `DEFAULT_RESERVE_BYTES` and `WebContext::{with_options, with_dyn_options,
  with_loader_options, options}` are also gone. Reuse a caller-owned buffer
  when capacity control matters, and construct contexts with `with_buffer`,
  `with_dyn_loader_buffer`, or `with_loader_buffer` as appropriate. HTML
  boolean attributes now always use native HTML serialization.
- **Breaking:** Client IR is now schema v3. Range statements carry an exact
  integer domain, standalone helper metadata and linked plans require v3
  bodies, and stale generated artifacts fail closed instead of being decoded
  as the current schema. Regenerate the application and all RSJS dependencies
  with one toolchain revision after upgrading. Client-helper returns must be
  owned; return `String` rather than borrowed `&str`. Client-helper `f32`
  parameters, locals, and returns are rejected until per-operation `f32`
  rounding is represented; use `f64` for now.
- The final RSJS emission phase now consumes rustc-resolved standalone-helper
  edges, authenticates each helper signature and body plus its transitive call
  graph, emits reachable helper definitions into the owning module, and writes
  direct private references at call sites. Deployable JavaScript is rejected
  if any reserved unlinked-helper identifier survives final emission.
- **Breaking:** reorganized the `proa-cli` library crate. The flat
  `proa_cli::rsjs_*` modules now live under `proa_cli::rsjs`
  (`rsjs_build` becomes `rsjs::build`, `rsjs_final` becomes
  `rsjs::final_compile`, and so on), subcommand argument types live in a
  dedicated commands layer, and `proa_cli::Cli` plus `proa_cli::run` remain
  the library entry points. The command-line surface (subcommands, flags,
  help, completions) is unchanged.
- **Breaking:** `proa_cli::rsjs::abi_adapter::CanonicalDescriptorBridge` now
  carries one private, kind-tagged component-or-helper codegen payload, so
  missing, doubled, or cross-kind states cannot be constructed through its
  public API. Use `envelope()`, `witnesses()`, `component_codegen()`, and
  `helper_codegen()` instead of field access; obtain bridges from the adapter
  rather than constructing them directly.
- **Breaking:** removed the dead `legacy_module` field from
  `rsjs_ir::ClientFunctionRef::Helper`. Downstream pattern matches should omit
  the field or use `..`; the active `ClientFunctionRef::Import` and
  `#[rsjs::client_module]` path are unchanged.
- **Breaking:** removed `#[proa_component]`, the inherent
  `#[rsjs(component)]` shorthand, and RSJS `async fn render` sugar. Custom
  components now author `WebRenderSync` or the canonical `WebRender` signature
  directly. RSJS analysis applies to those authored trait impls:
  `#[rsjs(component)]` permits composition without local origins, while
  `#[rsjs(component, client)]` permits signals, handlers, queries, mutations,
  shared-state acquisition, node refs, and browser work.
- **Breaking:** removed component-level `clients(...)` allowlists. A reachable
  call to a standalone `#[rsjs::client]` helper now creates a Rust-resolved
  linker edge automatically; `#[rsjs::client_module]` remains optional
  grouping.
- **Breaking:** deployable RSJS binaries now require the linked `cargo proa`
  pipeline. Ordinary `cargo check` and rust-analyzer retain provisional type
  checking, but a plain `cargo build` of an RSJS application fails at link time
  with an actionable `cargo proa build` diagnostic instead of silently
  producing a provisional binary.
- Linked static direct-component sites now preserve inbound timing through the
  final owner: direct reads stay live, signal initializers sample once per
  mounted occurrence, callback props resolve the current callback at
  invocation, and live query keys rebind. Reachable component-local queries,
  mutations, SharedState acquisitions, and node refs survive canonical linking
  and final code generation, while dead declarations remain inert.
  Required `NodeRef<T>` fields may now alias one parent-owned ref through a
  static direct composed child: the linker authenticates both local identities
  and the compatible terminal element family, fuses one owner, and emits no
  serialization, duplicate acquisition, or fallback object. Optional,
  explicit-island, conditional, repeated, keyed, and multiply-bound ref
  crossings remain fail-closed, as do other cross-component resource handles
  and unsupported dynamic/keyed component sites.
- Static SharedState provider topology now links end to end. Each recognized
  provider carries a stable `ProviderSiteId`, a rustc-witnessed schema, and a
  topology-only direct-child edge. Provider-dependent definitions receive a
  `ContextRequired` standalone-root disposition instead of a false synthetic
  root. Final server lowering preserves wrapperless provider rendering and
  rewrites only `children` with the exact linked occurrence; generated
  acquisition resolves that `(plan, host candidate, provider site)` and never
  falls back to another provider with the same `type_key`. Nested and sibling
  providers therefore retain distinct response-local scopes. Provider seeds
  remain latent until an admitted consumer acquires them and follow artifact
  commit/rollback. Conditional, repeated, keyed, and deferred provider
  creation remains rejected, and hydrated cross-response provider-cache replay
  remains disabled pending portable occurrence identity.
- Linked direct components governed by one authenticated conditional now
  execute as lifecycle groups with their static descendants. The active group
  mounts parent-first and disposes child-first without removing SSR DOM;
  `LiveRead` stays aliased, `SeedOnce` samples fresh state on remount, query
  observers are occurrence-disposed, and initialization or transition failures
  roll back to inert, retryable markup. Repeated, keyed, and
  nested-conditional component occurrences remain fail-closed.
- Final linked occurrences now omit compiler-owned `data-rsjs-*` lookup
  attributes, hydration-only text carriers, seed payloads, and marker payload
  construction when no browser owner needs them. The same reusable component
  retains those artifacts at owned occurrences, including mixed static/live
  text, and authored `data-rsjs-*` attributes are rejected as collisions with
  the compiler-owned namespace.
- Made `proa_core::RenderBuffer` the canonical reusable buffer for ordinary
  buffered rendering. `DefaultChainedBumpWriter` remains as a deprecated
  compatibility alias; advanced callers can continue to select an explicit
  page size with `ChainedBumpWriter<N>`.
- **Breaking:** removed the experimental `RouteResponse::ssr_zero_copy`,
  `RouteResponse::SsrZeroCopy`, and `proa_framework_axum::ZeroCopyBody` APIs. Use
  `RouteResponse::ssr` and the standard `RenderBuffer` path for synchronous
  framework routes. `ZeroCopyBuf` remains available as a low-level writer for
  benchmarks and custom transport experiments.
- **Breaking:** established the repository as the standalone source boundary
  and moved the coordinated SSR release group to 0.3.0. The incomplete mobile
  target, downstream runtime-template derives and documentation adapters, and
  product-specific route code generation were removed. `proa_codegen`,
  `proa_docs`, and `proa_docs_build` move to 0.2.0; the coordinated `rsjs`,
  `rsjs_ir`, and `rsjs_macros` group also moves to 0.2.0. The new `proa_ir`
  0.1.2 and `proa_dsl` 0.1.3 package baselines complete the framework
  dependency floor. Fresh 0.1.2 artifacts for `const_json`,
  `const_json_macros`, `itoa_lut`, `proa_og`, `tailwind-merge`, and
  `tailwind-merge-macro` ensure that every changed license/source archive has
  a new immutable registry version. See the
  [0.3 migration guide](docs/migrations/0.3-standalone.md).
- Added `proa_framework_axum::Readiness`, a generic async dependency probe that
  composes into Axum as GET/HEAD `/readyz` or a custom `MethodRouter`.
  Successful probes return a non-cacheable 200; errors and configurable
  timeouts return a non-cacheable 503 while keeping internal details in
  structured logs. Probe futures are cancelled when their request is dropped.
- Added per-request `CspNonce` propagation across framework-generated rsjs,
  React, client-shell, and streaming script tags. This adds a public
  `csp_nonce` field to `FrameworkRequestContext`; external exhaustive struct
  literals must initialize the new field.
- Added deterministic response-local `rsjs::PageRuntimePlan` generation and
  automatic framework finalization for sync, async, in-order, and
  out-of-order responses. `WebContext` now exposes transportable
  `RsjsRenderArtifacts` with cached-fragment replay and rollback-safe streaming
  branches, so pages no longer collect island IDs or assemble runtime tags.
  Streamed query-seed conflicts now converge on the same deterministic winner
  regardless of boundary completion order without overwriting newer
  browser-owned cache data. Artifact-aware out-of-order drain helpers are now
  canonical; the byte-only helpers are deprecated because they discard
  boundary-local island IDs and query seeds.
  **Breaking:** `PendingBoundaryFuture` now resolves to `RenderedBoundary`
  instead of a raw `Vec<u8>` so committed boundary HTML and its RSJS artifacts
  cannot be separated; custom producers with no artifacts can use
  `RenderedBoundary::html(bytes)`.
- Extended `#[derive(ProaComponent)]` to preserve component generics, infer
  direct generic-child render bounds, and optionally generate an
  allocation-free `async_render` bridge. Its opt-in `children_builder`
  generates a zero-allocation, type-inferred `with_children` method for direct
  `Option<C>` children and diagnoses unsupported generic shapes. Opt-in
  `constructor`, field defaults, and explicitly named optional-prop builders
  now cover structured component construction without boxing or type erasure.
- Historical, now removed: added `#[proa_component]` for custom synchronous
  render logic. A render-only inherent `fn render(self)` could contain ordinary
  Rust prelude work and finish in `html_sync!`; the macro generated portable
  buffer/loader/context/result plumbing, explicit generic-child render bounds,
  and an optional immediate async bridge while preserving lifetimes, const
  generics, and static dispatch. This authoring experiment was superseded by
  the explicit-trait contract above before release.
- Added explicit `boxed_sync_render(...)` and `boxed_render(...)` escape
  hatches for runtime-selected heterogeneous children. The sync wrapper
  retains `WebRenderSync` and exposes the concrete runtime size hint; both
  wrappers keep their boxed-component and vtable cost separate from Proa's
  generic default. Pending erased futures are cancelled when dropped.
- Added stable render-trait diagnostics for missing synchronous children,
  sync children placed in async-capable positions without `to_async(...)`, and
  mismatched loader types.
- Brought rsjs string `.len()` and `.trim()` closer to Rust semantics, reject
  conditions whose non-boolean type is statically proven, and preserve Rust's
  `NaN` / `inf` / `-inf` spelling for text expressions proven to be f32/f64.
- Made rsjs fixed-width client arithmetic explicit and checked: `i32`/`u32`
  remain Number-backed, `i64`/`u64` are always BigInt-backed, overflow and
  invalid division/remainder fail before mutation, and mixed or unsupported
  integer domains are compile errors. Boundary codecs may still transport
  other integer-typed fields without enabling arithmetic on them.
- Preserved those exact domains through typed client-helper parameters and
  locals, signal writes/updates, and explicit shared-field
  `rsjs::div_int::<T>(...)` calls. Ambiguous shared division now fails with a
  domain-specific fix-it instead of falling back to representation-only
  arithmetic.
- Made typed signal hydration recursively validate integer leaves inside
  `Option`, `Vec`, and structural objects, converting canonical `i64`/`u64`
  decimal wire strings to BigInt before client code observes them.
- Activated out-of-order `Suspense` priority and timeout scheduling. All
  boundaries still start concurrently; priority orders same-turn completions,
  while timeout cancels stalled work, emits an observable marker, and preserves
  the shell fallback.
  `PendingBoundary` gained public `priority` and `timeout` fields, so external
  exhaustive struct literals must initialize them or migrate to
  `PendingBoundary::new` / `PendingBoundary::with_options`.
- Replaced the starter root README with Proa-specific repository guidance.
- Added public contribution, security, generated-asset, and docs-index files.
- Licensed the repository copy under `AGPL-3.0-only` while retaining a
  separately contracted commercial option for closed-source use.
- Removed the unused standalone `proa_forms` workspace crate from the launch
  surface.
- Removed `proa_forms` again after it was reintroduced as a Wellformed wrapper.
  Nothing depended on it, and the `client = ::rsjs` wiring it provided is a
  parameter of `wellformed_macros::form_schema!`, so consumers should call
  Wellformed directly.

## 0.3.0

Initial public source release. This is the baseline the Unreleased entries
above build on: the `proa_core` / `proa_macros` / `proa_framework` 0.3.0
workspace with compiled `html_sync!` / `html!` / `md_sync!` / `md!` templates,
RSJS islands with the linked `cargo proa` pipeline, streaming SSR, the
`proa-cli` developer tooling (`fmt`, `lint`, `check`, `chunks`, scaffolding,
fonts), and the documentation site sources. Earlier internal iterations were
not published; their history is in the repository log.
