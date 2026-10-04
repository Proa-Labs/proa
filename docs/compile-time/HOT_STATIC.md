# Explicit static-update development prototype

Use an explicitly identified sync template:

```rust
use proa_macros::html_hot_sync;

html_hot_sync! {
    "profile-heading";
    <h1 class="text-lg">"Profile: "{self.name}</h1>
}
```

Start the application with `proa dev --hot-static`. Ordinary templates continue
to use `html_sync!`. Without the development manifest environment, the new
macro emits the existing consolidated literal path.

The prototype updates literal text and static attribute string values when
the complete source shape remains compatible. It uses the existing parser,
security validation, escaping, and consolidation walk. Dynamic Rust slots,
element/attribute names, macro identity, and Rust outside the template stay in
the compatibility check. An incompatible change runs the normal linked Cargo
build and server restart.

The compatibility shape also includes static-run positions among dynamic
children and attributes. Emptying a literal can remove a run; such edits use
Cargo when the run layout changes, including movements with the same run count.

Every render loads one coherent template snapshot; a later publication cannot
mix two generations within that template. A full build stages its candidate
manifest under the same compile environment, then publishes after a successful
build and before activating the new server. A compile failure retains the old
server and its prior static data. Missing, malformed, or incompatible runtime
data falls back to the compiled bytes.

## Deliberate prototype limits

- Root application's regular Rust files under `src/` only; dependencies,
  generated modules, and templates inside another macro's token input are not
  a supported hot-update surface. IDs must be unique within the scanned app.
- Straight-line sync HTML, with typed dynamic child expressions. Control
  flow, child `text!`, script/style, async, and RSJS-analyzed templates remain
  outside this surface. Continue using the ordinary macro for those cases.
  Inline `text!` attributes retain their dynamic boundary; changing their Rust
  literals requires compilation.
- The explicit dev render path reads JSON and allocates. This trades rendering
  efficiency for editor latency; normal macro expansions retain their fast
  path. There is no browser DOM patch protocol; the dev client reloads.
- Each watcher session gets its own manifest path to isolate active servers.
  Starting a new session changes a tracked compile input and can cause a cold
  build. Within a session, incompatible edits retain incremental caches.
- The benchmark measures save to changed HTTP response, not browser paint.

The serial release-CLI trial measured a 0.129s median across three static text
edits with a 100ms debounce. The HTTP acceptance probe checks escaping,
attributes, dynamic Rust fallback, failed-build retention, and repair. Its
earlier debug-CLI timings overlapped a release compilation and are correctness
evidence only. Reproduce correctness checks with
`scripts/hot_static_acceptance.py`; detailed historical logs are omitted from
the implementation diff.

The final HTTP regression run also executes a pre-build task that changes Rust
template literals. It verifies both initial and fallback manifests include the
task's generated content. Staging happens after tasks/assets and after the watch
snapshot, so edits during staging/build remain visible to the next cycle.

Keep this explicit until source discovery, render overhead, session cache
isolation, and representative application adoption justify a broader surface.
