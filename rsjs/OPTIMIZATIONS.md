# rsjs optimization queue

Forward-looking optimization ideas, ranked by impact-per-effort. Distinct from
[`STATUS.md`](./STATUS.md) (current implementation coverage) and
[`KNOWN_LIMITATIONS.md`](./KNOWN_LIMITATIONS.md) (current user-visible gaps):
this is the queue of things that would make RSJS measurably better in
production.

For each entry: **status** (filed / proposed / not started), **target
workload** (what shape of app benefits most), **design sketch**, **expected
impact**, **dependencies** on other entries.

## Round 2 results

Snapshot of the headline numbers as of the most recent measured run.
Pinned here so the queue stays grounded in reality and the result is
not folklore. The long-form methodology + reproduction recipe lives in
[`proa_demo/bench/RESULTS.md`](../proa_demo/bench/RESULTS.md).

### What shipped in Round 2

The optimizations marked **DONE** below are the body of work measured
in this round:

| Item | Commits |
|---|---|
| (0a) Production asset pipeline (content hash + minify + brotli) | `05f7b7b`, `513f9fc` |
| (0b) Measurement harness (timing + lean fixture + RESULTS.md) | `12eda22`, `78e453f`, `c8670b7`, `ac2989c` |
| (1) Route asset graph (WebContext-collected) | `cca0cc9`, `d768c46`, `05f7b7b`, `838ff0b` |
| (2) Event delegation (one listener per event-name per island root) | `e270622` |
| (6) Microtask-batched signal updates | `b5efb9d` |
| (7) Index-walk hydration with dev fallback | `b5efb9d` |

### Lean benchmark page (framework isolation)

60 watch-heart toggles on minimal HTML, no third-party scripts.
Cold cache, 4× CPU throttle, wifi, chromium 147 (playwright headless).

| metric | proa-rsjs | marko 6 | react 19 | proa vs react |
|---|---:|---:|---:|---:|
| **min-gz JS shipped** | **2,189 B** | (TBD this run) | 60,606 B | **27.7× smaller** |
| **scriptCPU** | **4.29 ms** | (TBD) | 45.7 ms | **10.6× less** |
| TTFB | 0.80 ms | (TBD) | 2.40 ms | 3.0× faster |
| FCP / TTI | 40 ms | (TBD) | 56 ms | 1.4× faster |

scriptCPU is sourced from CDP `Performance.getMetrics().ScriptDuration`
(cumulative main-thread JS execution). 4× throttle approximates a
midrange phone. n=3 runs per cell — below publishable threshold;
methodology gaps tracked at the bottom of this file.

### eBay SRP page (real-world third-party-heavy comparison)

Realistic ~2.4 MB shell with eBay analytics, OneTrust, Mapbox, etc.
running identically across all engines, so the *delta* is what's
informative. Same network/CPU profile as above.

| metric | proa-rsjs | marko 6 | react 19 |
|---|---:|---:|---:|
| **wire size (brotli, first-party JS)** | **2,258 B** | 3,973 B | 52,642 B |
| **scriptCPU delta vs proa** (framework only) | **0** | +0 | **+180 ms** |
| TTFB | 1.80 ms | 4.50 ms | 22.2 ms |
| FCP | 184 ms | 192 ms | 264 ms |
| TTI | 2.20 s | 2.34 s | 2.71 s |

Per-asset breakdown (proa side, brotli, first-party): runtime 1,656 B
+ watchheart island 262 B + filterchips island 340 B = 2,258 B.
Earlier "11 KB gz runtime" numbers were harness artifacts before
minify+brotli landed in (0a); treat as superseded.

### Methodology fingerprint

Pinned so the result is reproducible from this file alone:

- **Browser**: chromium 147 (playwright headless).
- **CPU**: 4× throttle via CDP `Emulation.setCPUThrottlingRate`.
- **Network**: wifi unthrottled (lean), wifi+4g matrix (eBay SRP) via
  CDP `Network.emulateNetworkConditions`.
- **Cache**: cold (CDP `Network.clearBrowserCache` +
  `clearBrowserCookies` before navigation).
- **scriptCPU**: cumulative `Performance.getMetrics().ScriptDuration`,
  not Lighthouse's TBT (which excludes the first 5 s).
- **TTI**: harness heuristic (earliest moment after FCP with 2 s of
  longtask quietness). Not Lighthouse's deprecated 5-s rule.
- **Ports**: proa playground :3001 + auto-spawned marko :3300, lean
  servers :3501 (proa) / :3502 (marko) / :3503 (react), react
  full-page :3401.
- **Engines**: see `RESULTS.md` § Engines for esbuild flags +
  production-build verification per engine.

Numbers above are honest but should not be cited externally until
the open methodology gaps in `RESULTS.md` are closed (n=30, marko in
the lean run, machine spec, raw CDP JSON dumps).

---

## Tier 0: prerequisites

These are not optimizations in the "shave bytes off a hot path" sense, but
they gate the credibility of every entry below: without (0a) deployments
won't see most of the wins; without (0b) the rankings here are educated
guesses, not measurements.

### 0a. Production asset pipeline

**Status**: **0a.1 (content-hashed URLs + immutable cache headers) DONE**
in commit `05f7b7b`. **0a.2 (minification) and 0a.3 (precompressed
.br/.gz siblings) still pending.**
**Target workload**: every production deployment.

**Problem**: today the framework adapter serves the runtime + per-island
JS as raw `&'static str` literals embedded in the binary. Content
hashing now exists, but minification and precompression do not, so a
production deployment still ships ~3-5× more bytes than necessary.

**Fix**: standard production asset story.

1. ✅ Compute a content hash per asset at startup (`runtime.<hash8>.js`,
   `<asset_id>.<hash8>.js`). **Done in `05f7b7b`** (sha256-truncated
   8-hex via `RsjsAssetIndex` OnceLock in `proa_demo/src/playground.rs`).
2. ✅ Serve with `Cache-Control: public, max-age=31536000, immutable`.
   **Done in `05f7b7b`** (un-hashed routes return `max-age=60` for
   backward compat; hashed routes return `immutable`).
3. ✅ Build precompressed `.br` (quality 11) and `.gz` (level 9)
   siblings; serve based on `Accept-Encoding`. **Done.** All five
   `serve_rsjs_*` handlers in `proa_demo/src/playground.rs` negotiate
   `br > gzip > raw`, set `Vary: Accept-Encoding` + `Content-Encoding`,
   preserve `Cache-Control` headers from 0a.1.
4. ✅ Minify the runtime + per-island JS. **Done.** At server startup,
   `proa_demo/src/playground.rs` shells out to esbuild
   (`--minify --target=es2020`) for each asset. Found via PATH walk
   then `marko-server/node_modules/.bin/esbuild` then
   `react-ssr/node_modules/.bin/esbuild`. Loud `tracing::warn!` +
   unminified fallback if not found.
   - Runtime: 35,798 B unminified → **4,942 B minified raw / 1,656 B
     brotli / 1,906 B gzip** over the wire.
   - Per-island JS: WatchHeart 976 B → 445 B raw / 262 B br;
     FilterChips 2,498 B → 910 B raw / 340 B br.
   - Startup cost: ~56 ms total for 5 assets (esbuild spawn dominated
     by Node startup, not minify work).
   - Follow-up: the three lean-page servers in
     `proa_demo/bench/lean-page/` don't share this path yet; small
     to add when needed.
5. ✅ Manifest entries return the hashed URL, not the canonical name;
   the route asset graph (entry 1) consumes them. **Done.**

**Expected impact**: the difference between "first-load" and "every
navigation pays the same first-load cost." Has to ship before this queue
is meaningful in production.

**Dependencies**: none. Foundational.

---

### 0b. Measurement / benchmark harness

**Status**: **DONE.** `proa_demo/bundle_size.mjs` (wire-size),
`proa_demo/bench/{run.mjs, measure.mjs, format.mjs}` (timing harness
with CDP-sourced scriptCPU + network throttling + 6× CPU profile),
plus a separate "framework-isolation" lean benchmark at
`proa_demo/bench/lean-page/` (3 servers, no third-party shell noise).
Methodology limitations are recorded in `proa_demo/bench/RESULTS.md`.
**Target workload**: this queue itself.

**Problem**: most "expected impact" claims in this doc are educated
guesses. Without a repeatable harness we can't honestly rank, can't
catch regressions, and can't tell users which optimizations actually
move metrics on their workload.

**Fix**: a single benchmark target that exercises the full matrix.

- **Workloads**: 1, 10, 100 instances of the same island; N=3, 10, 30
  unique island types; small page (1 KB DOM) vs large (200 KB DOM).
- **Marker density**: 1, 5, 20 `data-rsjs-handler` / `data-rsjs-bind`
  / `data-rsjs-cond-then` markers per island.
- **Network**: cold cache + warm cache; bytes raw, gzipped, brotli'd.
- **CPU**: unthrottled vs Chrome 4× CPU throttle ("midrange phone")
  vs 6× ("low-end phone").
- **Metrics**: TTFB, FCP, LCP, TTI, total CPU time during hydration,
  total bytes shipped per page.

Pair with a real-user-monitoring (RUM) shim that ships per-page
hydration-time spans so production deployments can validate their own
numbers.

**Expected impact**: makes ranking the rest of this queue defensible.

**Dependencies**: none.

---

## Tier 1: high-impact production wins

### 1. Route asset graph (WebContext-collected)

**Status**: **DONE.** Commits `cca0cc9` (WebContext side-channel),
`d768c46` (macro emits the recorder), `05f7b7b` (framework adapter
filters by captured set), `513f9fc`, `838ff0b` (regression tests).
**Target workload**: any app with more than ~5 distinct island types.

**Problem**: `rsjs_script_tags()` walks `rsjs::manifest()` (the global
linkme registry) and emits a `<script>` tag for every registered island
type regardless of whether this page renders any. With 50 islands and
streaming SSR, the cost is even worse: the regex-postprocess workaround
"scan the rendered HTML" can't run until the body is fully buffered.

**Fix**: render-time asset graph collected on `WebContext`. Each hydrated
rsjs strategy result's emitted render code calls into a small helper:

```rust
cx.rsjs_island_used(asset_id);          // emit_rust generates this call
```

`WebContext` accumulates a `HashSet<&'static str>` of hydrated asset ids touched
during render. `ServerOnly` rsjs output records no id. The framework adapter
reads the set after render to compute the exact asset graph for this response:

- island JS for rendered islands only
- the query/mutation runtime only if at least one rendered island sets
  `uses_queries` or `uses_mutations`
- client-module bundles only if rendered islands import them (via the
  `client_module_imports` field on the manifest entry)
- `<link rel="modulepreload">` decisions driven from the same set
- `<script type="module">` tags emitted in dependency order

Streams safely (each chunk's island ids land in the graph before the
chunk flushes).

**Expected impact**: linear in distinct island count. For a 50-island app
emitting 5 islands per page: ships 90% less first-party JS. Matters more
the bigger the app.

**Dependencies**: design-coupled with (5) hydration strategies — both
need to know which islands are on the page. Land (1) first, (5) reads
from it.

**Reference for the WebContext side-channel pattern**:
[`proa_core/src/web/context.rs`](../proa_core/src/web/context.rs).

---

### 2. Event delegation (one listener per event type per island root)

**Status**: **DONE.** New `bind_delegated_events(root, eventName, handlers)`
runtime helper; `rsjs_ir/src/emit_js.rs::emit_event_bindings` buckets
non-submit events by name and emits one delegation call per bucket.
Goldens regenerated. `bind_event` retained for backward compat.
`bind_submit` keeps per-form path (preventDefault needs the form
element). Saves N `addEventListener` calls + N `walk_path` calls per
island; on a 60×5-handler page, 300 listener calls become 60.
**Target workload**: any island with >2 handlers; islands inside large
homogeneous lists.

**Problem**: every `on_*` handler emits its own `addEventListener` call
at hydrate time. An island with 10 click handlers does 10
`addEventListener` calls. A page with 60 such islands does 600.

**Fix**: emit one root-level `addEventListener` per distinct event type
in the island. The handler dispatches based on `event.target.closest('[data-rsjs-handler]')`'s
attribute value. The IR already has a stable handler index per node; emit
that as the dispatch key.

Works with today's IR — no new variants needed. Pure codegen + runtime
change. Lands independently of (3) `signal_array`.

**Expected impact**: 60 islands × 10 handlers each goes from 600
listeners to 60 (one per island root) or even 1 (one at the document
level if we're aggressive). Real CPU win on hydrate; small but non-zero
GC pressure win during long-lived pages.

**Dependencies**: none. Independent of (3).

---

### 3. `signal_array` + indexed handlers (sibling-island fusion)

**Status**: proposed.
**Target workload**: list / grid pages with N instances of an identical
interactive component (Airbnb search results, eBay SRP, e-commerce
listings).

**Problem**: 60 watch-hearts today means 60 separate `hydrate()` calls,
each creating its own `Signal<bool>` object. Even with (2) event
delegation, the per-instance bodies aren't shared.

**Fix**: three coordinated changes that go together.

1. **IR**: add `SignalArray { id: SignalId, len: usize }` so the analyser
   can recognise a list of identical islands as one declaration of N
   parallel signal slots.
2. **emit_js**: emit one shared handler body that takes an index
   parameter; the runtime supplies the index by reading
   `data-rsjs-row` from the dispatch target.
3. **Runtime**: a typed-array-backed signal pool (`Uint8Array` /
   `Int32Array` etc) so 60 booleans use 60 bytes, not 60 object
   allocations.

**Expected impact**: 60 `WatchHeart` instances go from 60 hydrate calls
to 1. Hydration time drops by an order of magnitude on list pages; GC
pressure during long sessions is much lower.

**Dependencies**: (2) event delegation as a prerequisite (we can't
fuse handlers without one root listener). (6) microtask batching too —
otherwise a row that toggles 3 fields per click triggers 3 layouts.

**Honest scoping note**: this is a substantially harder change than (2)
on its own. It's at least a week's work across IR, analyser, codegen,
and runtime. (2) lands first and gets most of the per-handler-call
savings; (3) is the consolidation pass that gets the per-instance
savings.

---

### 4. Hydration strategies (visible / idle / interaction / media)

**Status**: implemented for current island metadata and runtime entry points.
**Target workload**: long pages with islands below the fold (any feed,
listing, infinite scroll, e-commerce category page).

**Problem**: today every per-island JS module calls `hydrate(...)` at
module load. There is no per-instance opt-in to defer or skip
hydration. Browsers run all of it as soon as the script tag executes.

**Fix**: this is two changes (codegen + runtime) coupled tightly.

- **Codegen**: per-island JS exports a `hydrate(root)` *function* but
  does NOT call it at module load. The runtime's bootstrapping logic
  decides when to call it.
- **Runtime**: each `<rsjs-island>` is examined at boot; based on a
  `data-rsjs-strategy` attribute on the wrapper, the runtime hooks the
  appropriate trigger:
  - `eager` (default): call `hydrate(root)` immediately.
  - `visible`: register an `IntersectionObserver` with rootMargin
    200 px, hydrate when it fires.
  - `idle`: schedule via `requestIdleCallback` (fallback `setTimeout`).
  - `interaction`: register a one-shot capture-phase listener on the
    wrapper for click / focus / pointerover; hydrate on first hit.
  - `media:(query)`: hydrate when `matchMedia(query).matches` is true,
    e.g. `media:(min-width: 768px)` for desktop-only widgets.

Per-island opt-in uses
`#[rsjs(component, client, strategy = "visible")]` on the authored
render-trait impl, defaulting to `eager`.

**Expected impact**: TTI on a 60-item SRP drops from "all 60 hydrate at
once" to "the 5-8 visible cards hydrate, the rest load on scroll." Real
gains depend on viewport ratio. For pages with mostly-static below-the-fold
content (terms-of-service-style layouts) `idle` makes hydration
imperceptible.

**Dependencies**: requires codegen change to NOT auto-call `hydrate()`
at module load (Astro / Qwik both do this; ours doesn't yet). Builds on
(1) so the modulepreload hints can be skipped for islands with `idle`
or `interaction` strategies.

**References**: Astro's `client:visible` / `client:idle` /
`client:media`; Qwik's resumability model.

---

### 5. Inline tiny island JS

**Status**: not started.
**Target workload**: any island under ~1.5 KB compressed.

**Problem**: each per-island JS file is one HTTP request. For a Counter
or WatchHeart whose entire JS is ~250 B gzipped, the round-trip
overhead exceeds the bytes themselves.

**Fix**: framework adapter inspects each manifest entry's `js_source`
length. If gzipped under threshold (1.5 KB feels right), emit
`<script type="module">…body…</script>` inline instead of a `<script
type="module" src="…">`. Above threshold, external file +
`<link rel="modulepreload">`.

**Expected impact**: for sites with many small islands, eliminates N
round-trips on first load. For repeat visits the inline-vs-external
tradeoff inverts (external is cached, inline isn't), so this is a
first-load optimization. Worth measuring before-and-after via (0b).

**Dependencies**: (1) route asset graph (so we know which islands the
page uses).

**CSP caveat**: inline `<script type="module">` requires either a CSP
`nonce` per request or a `'unsafe-inline'` clause (which most production
deployments avoid). The framework adapter must mint a per-response
nonce, attach it to every inlined script tag, and put the matching
`script-src 'nonce-…'` value into the `Content-Security-Policy` header.
Sites with a strict CSP that uses hashes instead need each inlined
script's SHA-256 digest in `script-src`. Worth offering as a
configuration toggle: `inline_threshold_bytes: usize`, defaults to 0
(off) until a deployment opts in.

**Caveat 2**: the rsjs runtime must still load externally (it's bigger
and shared across pages). Only per-island JS is candidate for inlining.

---

## Tier 2: hot-path optimizations

### 6. Microtask-batched signal updates

**Status**: **DONE.** New `pending_effects: Set<EffectFn>` + `scheduled: bool`
scheduler in the runtime; `signal.set()` enqueues subscribers via
`schedule(effect)` instead of running them synchronously. First enqueue
per tick arms `queueMicrotask(flush)`. Flush snapshots-and-clears
before iterating to handle re-entrant writes. 4 unit tests in
`rsjs/runtime/rsjs-runtime.batching.test.mjs`.
**Target workload**: any island where one event handler writes more
than one signal; prerequisite for (3).

**Problem**: today each `signal.set(...)` immediately walks subscribers
and mutates the DOM. A handler that writes 3 signals causes 3 DOM
commits.

**Fix**: scheduler defers `apply` calls into a microtask queue, dedupes
by target node, flushes once per tick. `queueMicrotask` for the
actual scheduling; a small `Set<EffectFn>` for dedupe.

**Expected impact**: small per-handler today (most handlers write one
signal), but **prerequisite for entry (3)**. Without batching, an
indexed handler that updates 3 fields on a row click would cause 3
layouts.

**Dependencies**: none.

---

### 7. Index-walk hydration (with dev fallback)

**Status**: **DONE.** `compute_index_path` in
`rsjs_ir/src/emit_js.rs` walks the IR view tree to bake an index
path per marker. The runtime exports `walk_path(root, path,
marker_attr, marker_value)`; in dev mode it asserts the walked node
carries the expected marker attribute and falls back to
`querySelector` on mismatch. Note: with (2) event delegation also
landed, `walk_path` is no longer called for event handler nodes
(delegation skips it entirely); it's still used for `bind_text`,
`bind_input`, and `bind_conditional`.
**Target workload**: islands with more than ~3 marker attributes.

**Problem**: each marker lookup is `root.querySelector('[data-rsjs-…]')`.
For an island with 6 markers that's 6 selector evaluations per
instance.

**Fix**: the IR knows the tree shape at compile time. Bake an index
path into the per-island JS (`root.children[1].children[0]`) instead of
a selector. O(depth) per lookup with no string parsing or attribute
scan.

**Important: dev-mode fallback.** If the SSR'd DOM has been mutated by
a third party (an extension, a framework integration, an A/B testing
script that runs before hydration), the index walk lands on the wrong
node. In dev / debug builds the runtime should also lookup the
expected `data-rsjs-handler="N"` (or equivalent) and assert the
index-walked node carries it; on mismatch, log a clear console error
("rsjs hydration: index walk for handler 0 expected
`data-rsjs-handler='0'`, found `<div>` without that attribute. DOM
shape may have drifted between SSR and client.") and fall back to
`querySelector`. In prod builds, skip the assertion to keep the hot
path fast.

**Expected impact**: 20-40% faster hydration for medium-complexity
islands. Marginal for tiny islands like WatchHeart.

**Dependencies**: none. Codegen-side change in `rsjs_ir/src/emit_js.rs`.

---

### 8. Seed-state optimization (page-level seed table)

**Status**: not started, refined design.
**Target workload**: pages with many small island instances; pages with
runtime-typed initial values.

**Problem**: today each island instance has its own
`<script type="application/json" data-rsjs-island="…">{"signals":[…]}</script>`.
The runtime does `getElementById` + `JSON.parse` per instance. For 60
islands, that's 60 element lookups + 60 parses + 60 small JSON
allocations.

Note: post-runtime-signal-initializers, NOT every seed is a static
literal. `signal(self.foo)` produces per-instance state that has to
ship per instance. Static-literal seeds are still common (`signal(false)`)
and are the candidate for inlining.

**Fix**: layered.

1. **Static seeds inlined into per-island JS.** When ALL signals in an
   island type have static-literal initial values (the analyser knows
   this), bake the seed into the per-island JS module itself. Skip
   the seed `<script>` tag entirely. Saves N script tags + N parses.
2. **Page-level seed table for runtime seeds.** When seeds are runtime
   expressions (`signal(self.foo)`), keep one combined
   `<script type="application/json" id="rsjs-seed-table">{"watchheart_<hash>": [...], "filterchips_<hash>": {...}}</script>`
   per page. The runtime parses ONCE and dispatches by asset_id +
   instance index. Saves N-1 parses, N-1 script tags.

The codegen knows which mode applies per island type at compile time
(it's a property of the IR — are all signals `Value::Lit(_)`?).

**Expected impact**: small per page in absolute terms (a few KB at
most), but reshapes the hydration hot-path: one parse instead of N.
Compounds with (3) and (4).

**Dependencies**: cleaner alongside (1) (the route asset graph is
what generates the page-level table) and (3) (per-row instance state
flows through `signal_array`).

---

### 9. Disposer / unsubscribe lifecycle

**Status**: implemented for current runtime bindings and hydration strategies.
Generated island and fragment modules return disposer closures for their
bindings, query/mutation policies unsubscribe from timers/listeners, and the
runtime tracks both hydrated and pending deferred roots. Explicit
`dispose_hydrated(name, root?)` cleanup cancels immediate, visible, idle,
interaction, and media hydration paths; a shared `MutationObserver` also prunes
hydrated or pending roots when a route transition removes the island element
without an explicit framework callback. Starting a newer hydration for the same
island/root now cancels stale pending strategy observers/listeners before the
new request is registered.
**Target workload**: long-running SPAs that swap islands in/out without
full page navigation; eventual partial-navigation / View Transitions
support.

**Problem**: without an explicit lifecycle, subscriptions, event
listeners, and observers created at hydrate time would live until page
unload. For client-driven partial navigation (replacing one section of
the DOM without reloading), every removed island would leak its
subscriptions.

**Fix**: each `hydrate(root)` returns a `Disposer` (a closure that
unbinds events, unregisters from signal subscribers, disconnects
observers). The runtime tracks per-island disposers keyed by the
`<rsjs-island>` element. When a known island element is removed
(detected via a shared `MutationObserver`) or an adapter calls
`dispose_hydrated(name, root?)`, the matching disposer fires.

**Expected impact**: low overhead today (one shared observer only while
watchable hydration records exist). Prerequisite for any future
partial-navigation feature (View Transitions API integration,
Turbo-style partial swaps, `<dialog>`-based router).

**Dependencies**: none, but worth designing now so (4) hydration
strategies don't bake in assumptions that block this later.

---

## Tier 3: situational

### 10. Per-page runtime tree-shaking

**Status**: not started.
**Target workload**: low-interactivity pages (mostly static, one or two
islands).

**Problem**: `/rsjs-runtime.js` always ships every binding helper.

**Fix**: framework adapter computes the union of binding kinds used by
this page's islands; serves a tree-shaken runtime via a content-hashed
URL.

**Expected impact**: ~200-500 B saved per page; **kills cross-page
caching of the runtime** unless we route-cache by the binding-kind set
(which (0a) makes possible). Probably bad tradeoff for most apps;
worth it for marketing-style sites with one tiny interactive widget.

**Dependencies**: (0a) for content-hashed URLs that don't break caching.

---

### 11. Page-bundle mode

**Status**: not started.
**Target workload**: pages with 5+ distinct island types where
first-load performance matters more than cross-page caching.

**Problem**: 5 distinct islands on one page = 5 HTTP requests.

**Fix**: framework adapter offers an opt-in mode that bundles all
this-page islands into one `/rsjs/page-bundle/<route-hash>.js`. One
request, slightly worse cross-page cache reuse.

**Dependencies**: should NOT be combined with (5) inline tiny islands;
the two strategies overlap.

---

### 12. Production debug tooling

**Status**: not started.
**Target workload**: developers, not users.

Not an optimization per se: today there is no devtools extension or
runtime debug mode showing which signals an island has, what their
current values are, or what triggered a recent re-render. Worth filing
here so it doesn't get lost.

### 13. CDN distribution + CSP / SRI hardening

**Status**: not started.
**Target workload**: production deploys at any scale, multi-region SSR,
CSP-strict environments (banking, healthcare, gov, anything with a
SOC 2 / FedRAMP audit).

Today rsjs assets are baked into the Rust binary's `.rodata` and
served from memory by the framework adapter. `rsjs::manifest::write_manifest_assets`
already materialises the same bytes as a `dist/` directory + `manifest.json`
suitable for upload to S3 / Cloudflare / Fastly. What's missing is
the seam between SSR-emitted asset URLs and the CDN base URL, plus
the security-policy attributes that turn this from "works" into
"passes a SOC 2 review."

**Sub-items:**

1. **`RsjsAssetBase` config knob.** Today the framework hardcodes
   `/rsjs/island/<id>.<hash>.js` and `/rsjs-runtime.<hash>.js` paths.
   Make this configurable: `RsjsAssetBase::Origin` (current behaviour),
   `RsjsAssetBase::Cdn { base_url: "https://cdn.example.com" }`, or
   `RsjsAssetBase::Custom(Box<dyn Fn(&str) -> String>)`. Threaded
   through `WebContext` so `<script src="...">` emission picks it up.
2. **`cargo proa publish-assets` command.** Wraps `write_manifest_assets`
   with object-store upload (S3 / R2 / GCS). Idempotent: content-hashed
   URLs mean re-uploads are no-ops. Should run *before* the binary
   deploy to avoid the version-skew window.
3. **Subresource Integrity (SRI).** Asset URLs already carry a content
   hash; emit `integrity="sha384-<hash>" crossorigin="anonymous"` on
   every `<script type="module" src="...">` tag. Almost free since
   we compute the hash anyway. Major value-add when serving from a
   third-party CDN: protects against CDN compromise, mandatory in
   some compliance regimes. Requires switching the URL hash slug
   from whatever truncated hash we use today to a full SHA-384 (or
   computing both alongside).
4. **CORS headers on CDN-served assets.** `Access-Control-Allow-Origin`
   required for SRI error reporting and for cross-origin fetch in the
   runtime if/when it ever does that. Document the required CDN
   config; ideally provide a Terraform / Pulumi snippet.
5. **CSP nonce threading.** Today the seed-state blobs ship as inline
   `<script type="application/json" data-rsjs-island="...">` tags.
   These are *not* executable, but `script-src` covers them anyway,
   so a strict CSP without `'unsafe-inline'` blocks them. Solution:
   accept a per-request nonce in `WebContext`, emit `nonce="..."` on
   every rsjs-emitted `<script>` tag (data and module). The framework
   integration generates one nonce per request and threads it through.
   The runtime is already CSP-friendly (no `innerHTML`, no `eval`, no
   `Function()`, no `document.write` — verified `rg` clean on
   `rsjs/runtime/`).
6. **Trusted Types compatibility statement.** Document that the rsjs
   runtime does not assign to any DOM sink that Trusted Types covers
   (`innerHTML`, `outerHTML`, `Element.setAttribute('on*', ...)`,
   etc.). With `require-trusted-types-for 'script'`, rsjs apps work
   without registering a policy. Add a CSP test fixture to CI to
   keep this property from regressing.
7. **Documentation: a `docs/deployment/cdn-and-csp.md`** with the
   full policy header recipe, nonce generator examples for axum +
   actix, the SRI handshake, CORS config, and a known-good `Content-Security-Policy`
   header for a strict deployment.

**Expected impact.** Unblocks production deploys in regulated
environments and removes origin CPU spent on static-asset serving.
Doesn't help the bench numbers — it's a productionization gap, not a
performance one. Ship before any external pilot in finance / health /
gov verticals.

**Dependencies**: none on this list. Can be designed independently of
(4) hydration strategies. Item 5 (CSP nonces) and item 3 (SRI) are
small enough to land as a single PR; the CDN config knob (1) and
publish command (2) are the larger pieces.

---

## Sequencing notes

### Done (rounds 1, 2, 3)

- ✅ (0a.1) content-hashed URLs + `Cache-Control: immutable`
- ✅ (0a.2) minify runtime + per-island JS at server startup (esbuild)
- ✅ (0a.3) precompressed brotli/gzip siblings + Accept-Encoding negotiation
- ✅ (0b) measurement harness (wire-size + timing harness + lean benchmark page)
- ✅ (1) route asset graph
- ✅ (2) event delegation
- ✅ (6) microtask batching
- ✅ (7) index-walk hydration with dev fallback

### Remaining, recommended order

Sequencing as of post-Round-2. `signal_array` (item 3) is the most
valuable raw IR change but also the riskiest analyser surgery in the
queue; with batching and delegation already shipped its urgency
dropped, so it sits behind the more broadly useful product features.

1. **(12) devtools — debug manifest + `window.__RSJS_DEVTOOLS__.inspect(root)`.**
   High-leverage authoring/debugging win that doesn't threaten the
   tiny production runtime story (debug code lives behind a build
   flag / dev-only entry point). Start with the inspector; profiling
   and time-travel can layer in later.
2. **Lean-server minify parity.** The proa lean-page server doesn't
   yet share the minify+brotli pipeline from the playground (~30 min
   of work — mirror `RsjsAssetIndex` from `playground.rs` into
   `proa-lean-server`). Closes a real-deployment-story gap before
   the lean numbers go external. Marko + react already minify their
   bundles via esbuild at startup.
3. **(4) hydration strategies (visible / idle / interaction / media).**
   Most generally useful as a product feature; the codegen +
   runtime handshake for "modules export `hydrate` rather than calling
   it" is a one-time investment that opens up Astro / Qwik-style
   selective hydration. Pairs with (1) — `idle` / `interaction`
   strategies skip the modulepreload hint.
4. **(5) inline tiny islands.** Same area of code as (1); cheap win
   for sites with many sub-1.5 KB islands.
5. **(3) signal_array + indexed handlers.** High upside (collapses
   list-shaped pages back to O(1) JS), but the riskiest IR/analyser
   change in the queue. Deferred behind (4) and the devtools work
   because batching + delegation already cover the worst per-instance
   cost cases.
6. **(8) seed-state page-level table.** Depends on (3).
7. **(9) disposer lifecycle.** Design alongside (4); implementation
   waits until partial navigation is on the roadmap.
8. **(13) CDN distribution + CSP / SRI hardening.** Productionization
   gap, not a perf item. Sub-items split: nonces + SRI are a small
   focused PR; the `RsjsAssetBase` config knob + `publish-assets`
   command are bigger. Ship before any pilot in regulated verticals.
9. **(10), (11)** — situational, evaluate when needed.

### Methodology checklist for the public-facing comparison

The numbers in `proa_demo/bench/RESULTS.md` are honest but should not
be cited externally until:

- [ ] n=30 runs per cell (currently 3)
- [ ] Marko numbers in the lean comparison (currently proa vs react only)
- [ ] Exact machine spec recorded
- [ ] Raw CDP JSON dumped per run for outlier inspection
- [ ] Automated production-build verification for the React bundle
- [ ] Network throttling matrix on the lean page (currently wifi-only)
- [ ] Workload variance (1, 60, 300 widgets)

## See also

- [`STATUS.md`](./STATUS.md) — current implementation coverage and remaining
  compiler/runtime work.
- [`KNOWN_LIMITATIONS.md`](./KNOWN_LIMITATIONS.md) — current blockers,
  including the "per-element event listeners" item that's covered here
  by (2).
- [`proa_demo/bundle_size.mjs`](../proa_demo/bundle_size.mjs) — first
  measurement script; will become the seed for (0b).
