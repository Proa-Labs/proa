# rsjs mega status

This is the single human-readable status board for rsjs. It tracks what is
implemented, what is intentionally unsupported today, what is partially
implemented, and what should be built next. Keep detailed design discussion in
the linked docs; keep this file as the scoreboard.

## Status legend

| Status | Meaning |
|---|---|
| Done | Supported behavior with tests or real component coverage. |
| Partial | Some layers or positions work, but the feature is not fully author-facing. |
| Expected fail | Recognized unsupported shape with a stable diagnostic. |
| Planned | Intended direction, but not implemented yet. |
| Out of scope | Not a goal for rsjs. |

## Source of truth

| Source | Purpose |
|---|---|
| [`docs/agent/api-reference/slot-grammar.md`](../docs/agent/api-reference/slot-grammar.md) | Current author-facing rsjs slot grammar. |
| [`rsjs_macros/tests/ui/conformance/pass`](../rsjs_macros/tests/ui/conformance/pass) | Executable supported DSL surface. |
| [`rsjs_macros/tests/ui/conformance/fail`](../rsjs_macros/tests/ui/conformance/fail) | Executable unsupported DSL surface with diagnostics. |
| [`rsjs_conformance/features.json`](../rsjs_conformance/features.json) | Machine-readable feature/status denominator for conformance fixtures. |
| [`rsjs_macros/tests/ui/fail`](../rsjs_macros/tests/ui/fail) | Focused compile-fail diagnostics. |
| [`rsjs/KNOWN_LIMITATIONS.md`](./KNOWN_LIMITATIONS.md) | User-visible gaps hit by real use cases. |
| [`rsjs/BROWSER_API_PRIORITY.md`](./BROWSER_API_PRIORITY.md) | Ranked web-platform API inventory for deciding which browser helpers belong in rsjs. |
| [`rsjs/OPTIMIZATIONS.md`](./OPTIMIZATIONS.md) | Performance-specific roadmap. |

## Numeric contract

Accepted client arithmetic has four fixed-width domains: `i32`, `u32`, `i64`,
and `u64`. The 32-bit domains are Number-backed; the 64-bit domains are always
BigInt-backed. Operations preserve the exact domain, check overflow and invalid
division/remainder before mutation, and reject mixed-domain arithmetic and
comparisons. Other integer widths are expected compile failures when used in
client arithmetic. Boundary codecs may still carry other integer-typed fields;
that transport capability is not arithmetic support.

Typed signal hydration recursively validates integer leaves inside `Option`,
`Vec`, and structural objects before observation. Shared-field integer division
requires `div_int::<T>` unless both operands independently prove the same
supported domain, and lowers to the same checked operator as local arithmetic.
The semantic corpus contains 28 cases: 26 parity guarantees and two exact
finite-formatting divergences, with three separate unsupported wire-shape
exclusions.

Two source-safe DevX gaps remain. Ordinary shared arithmetic outside explicit
typed boundaries can specialize domains in traversal order, and an outer
`div_int::<i64>` does not yet contextualize a nested bare `div_int(...)`.
Generated Rust witnesses reject wrong explicit widths, but deterministic
diagnostics need scoped specialization or a whole-island proof prepass.
Type-erased initial values may also require the generic on the signal itself,
for example `signal::<Option<i64>>(None)`.

## Top-level scoreboard

| Area | Status | Notes |
|---|---|---|
| RSJS sync components | Done | Authors normally implement `WebRenderSync<L>` directly, then add `#[rsjs(component)]` for analyzed participation or `#[rsjs(component, client)]` for local browser origins. Advanced custom writers may additionally specify `B`. |
| RSJS async components | Done | Authors normally implement the canonical `WebRender<L>` signature and return `RenderOutcome::pending(async move { ... })`; `async fn render` sugar is removed, and advanced custom writers may additionally specify `B`. |
| Impl generics | Done | Lifetime, type, const generics, renamed buffer/loader params, HRTB-style where clauses. |
| Reactive text/attribute bindings | Done | Signal reads, projections, string helpers, value-level conditionals. |
| Template conditional sections | Done | Multiple sibling conditionals per parent element and loop-repeated conditionals are supported. |
| Template `for` loops | Done | Repeated text/attr/event/input/conditional bindings, document/window handlers, and loop payloads work for the rsjs-supported binding surface; node refs in template loops remain an expected compile fail. |
| Handler bodies | Partial | Blocks, handler-local `let`/`let mut` reassignment, signal/event ops, callback calls, browser effects, `if`/`else`, supported `match` shapes, and static/numeric-range loops work. General Rust does not. |
| Component props and callbacks | Partial | Direct static sites now carry rustc-witnessed value/callback fields through descriptor compilation, canonical linking, owner partitioning, and final fragment codegen. `LiveRead`, `SeedOnce`, `QueryKeyDependency`, invocation-time current callbacks, and multi-layer presentational pass-through retain distinct timing without serialization; imported/renamed/re-exported components work in the real two-compilation fixture. Browser runtime coverage proves that `SeedOnce` preserves child-owned edits across parent writes and resamples only for a fresh occurrence. Remaining gaps are broader explicit-island crossing rules, client handles/slots, and dynamic/generic sites. `EffectDependency` is reserved but intentionally fail-closed until a separate effect-semantics amendment defines authoring, scheduling, dependency tracking, cleanup, and failure behavior. |
| Tolerant server-rendered values | Done | Ordinary prelude values and dependency-free opaque renderables remain SSR with unknown `0..N` DOM cardinality; `server_only(...)` is a compatibility escape hatch, `server_attr(...)` handles static attributes, and `rsjs::island(...)` is an explicit owner boundary. |
| Browser reads/effects | Partial | Many safe helpers are implemented, including encoding/base64 helpers, WebAuthn credential JSON helpers, route/storage/viewport window events, and indexed event collection reads. Arbitrary browser APIs remain unsupported; use application-owned JavaScript or add an explicit typed RSJS capability rather than hiding effects in an origin-free client helper. |
| Generated typed-web facade | Partial | `rsjs_web`, re-exported as `rsjs::web`, generates the shared DOM marker/capability hierarchy, event-interface vocabulary, Rust binding obligations, capability-gated handle methods, and reviewed DOM operation registry. Typed event/current-target and local node-ref/tag mismatches fail in rustc. Local handlers expose exact element current targets and may use the broad `dom::Any` default; document/window handlers require exact non-element `Document`/`Window` targets while retaining their source elements only for compiler-owned payload lookup. First-class current targets lower supported scalar reads to direct JS and cannot escape the handler. Complete Webref ingestion, checked raw-target casts, dynamic `EventTarget`, and broader browser-registry migration remain planned. |
| Node refs | Done | Element/form/dialog/popover methods, attribute reads/writes, style/class/text/value operations, validation helpers, and geometry reads. |
| Client helpers | Partial | One `#[rsjs::client]` definition grants eligibility. Reachable ordinary Rust call paths emit rustc-resolved symbolic edges without a component allowlist; imported, renamed, and re-exported paths converge on the defining helper. `#[rsjs::client_module]` is optional grouping. Helper metadata carries v3 `BODY_IR_JSON` plus a generated server evaluator, and SSR text bindings can render helper values. Nested helper calls, unary operators, `let` / `let mut` locals with assignment, value-level `if` / `else`, and if-guarded early returns evaluate on the server and in linked JS. Helper-local named structs lower to nontransportable plain-object values with field reads, field/compound assignment, and SSR evaluator parity. `#[rsjs::client]` on a closed inline module supports direct constructors and borrowed read/mutation methods with evaluation-once scalar arguments, scalar and lexically scoped branch locals, local/field mutation, and assignment-oriented branches. Constructors and receiver methods are private transitive helper definitions with nominal internal-only object signatures; method bodies emit once and nested argument-bearing reads work. Deployable roots reject object parameters/returns, ordinary-call object escape and direct object equality fail before JS emission, and SSR uses shared reference-shaped carriers. `#[rsjs::client(class)]` exports public structs as registered ESM classes with private persistent state, `new`/`instanceof`, public receiver methods, closed nested calls, manifest JSON, deterministic deployment filenames, and Node acceptance. JavaScript owns exported instance lifetime; automatic hydration-scoped construction/disposal remains future work. |
| Query IR/runtime | Partial | IR, JS emitter, runtime asset plumbing, sync/async macro authoring, `FetchRequest` descriptors, structural `QueryKey::new((...))` cache keys, browser fetch execution, retry/progress/streaming, manual `query.refetch()` / `query.invalidate()` handlers, typed query-data field projection (`price.data().value`), per-instance query key locators, page-level query seed table collection/consumption, deterministic observer scheduling, shared-request deduplication, generation teardown/abort fencing, and compiler-emitted `rebind_query(...)` watchers are in place. Linked query reachability now excludes dead declarations, canonicalizes observations/refetch/invalidation, preserves inbound `QueryKeyDependency`, unions the query-runtime requirement across fused fragments, and reads adjacent query metadata through each fragment's `seedRoot`; the real two-compilation fixture covers a live parent key feeding an async query child. `query_with_initial(...)` accepts an already-available `Serialize` value in sync render or one loaded through `cx`/`DataLoader` in async render. Typed field access is canonical; JSON-pointer `.at(...)` remains an escape hatch. Remaining work is broader collection/object convenience and currently gated nested cross-kind query operations. |
| Mutation IR/runtime | Partial | IR, JS emitter, runtime asset plumbing, async macro authoring, `FetchRequest` runners, browser fetch execution, retry/invalidation, runtime conformance, and runtime consumption of per-instance `data-rsjs-island-mutations` metadata are in place. Mutation keys and invalidation keys can use server locals by serializing the effective keys into that metadata at render time. Descriptor-backed async parents can inline mutation child components, including child mutation metadata with server-local keys/invalidations. Signal-backed mutation keys/invalidations still use generated JS fallbacks. |
| Typed page-scoped shared state | Partial | `#[derive(rsjs::SharedState)]`, namespaced schema identity/hash metadata, pre-activation conflict checks, typed field handles, synchronous writes, batching, page hydration, late-hydration preservation, disposal, and page teardown are implemented. Static `SharedStateProvider` topology now carries a compiler `ProviderSiteId`, rustc-witnessed schema, exact direct-child edge, and `ContextRequired` root disposition. Final lowering rewrites only provider `children`; linked sync/async acquisition resolves the exact `(plan, host candidate, provider site)` with no nearest-`type_key` fallback. Nested/sibling occurrences stay distinct, and latent seeds follow artifact commit/rollback. Conditional, repeated, keyed, and deferred provider creation plus hydrated cross-response cache replay remain unsupported. Shared state remains synchronous client-owned UI state; queries/mutations retain server-owned async cache semantics. |
| Shared component fragment chunks | Partial | Descriptor-backed fragment assets, parent import/mount calls, callback adapters, repeated scopes, loop payload preservation, and manifest registration are in place. Schema-v4 hydrated fragments are parent-mounted and still require parent JS, even when self-contained; independent descriptor-child boundaries remain descriptor-v5 work. Catalog/framework asset paths hash, serve, preload, and smoke-test route-used fragments. Bundle-size tooling now separates runtime, island, client-module, and fragment bytes with optional rsjs budget gates; remaining work is broader component migration. |
| Client IR v3 / HIR | Partial | The syn-free Client IR v3 data model, JS emitter, ES-module emitter, shared JSON reader/writer, unresolved helper function refs, centralized handler-to-v3 conversion API, cached v3 handler/callback-adapter blocks, default island/fragment/callback-adapter handler emission, macro-side handler HIR that now directly produces DOM-handler and parent callback-adapter `ClientBlock`s, real handler-local `let` locals, exact fixed-width domains on range induction variables, direct handler-expression payload spans, ClientModule-driven client-helper bundle emission, statement-level helper blocks, v3 dependency collection, handler dedup by v3 block equivalence, dead-signal handler analysis over v3 blocks, runtime-helper import scans over v3 blocks, v3-only component-descriptor handler serialization, direct `body_ir` parsing/remapping through component inlining with required descriptor-level `client_ir_version`, old flat handler-emitter removal, and initial control-flow coverage are in place; broader direct-v3 lowering/module migration is next. |
| Machine-readable conformance matrix | Partial | Seed manifest exists, is validated against trybuild fixtures, covers nested handler/callback branch support, helper early returns, and hydration strategy fixtures, validates claimed snapshot files, has full HIR/IR/JS/runtime coverage for current pass features, enforces descriptor snapshots for compile-pass descriptor-relevant source positions, and has a report script. |

## Implemented surface

### Macro and render shape

| Feature | Status | Coverage |
|---|---|---|
| RSJS sync authoring | Done | `#[rsjs(component)]` and `#[rsjs(component, client)]` analyze an authored `impl WebRenderSync<L>` by default; inherent impl shorthand is removed, and the advanced custom-writer form may specify `B`. |
| RSJS async authoring | Done | The same component attributes analyze an authored canonical `impl WebRender<L>` by default; `async fn render` sugar is removed, and the advanced custom-writer form may specify `B`. |
| Impl-level lifetime generics | Done | `parser_generics.rs`. |
| Impl-level type generics | Done | `parser_generics.rs`. |
| Impl-level const generics | Done | `parser_generics.rs`. |
| Renamed buffer/loader generic params | Done | `parser_generics.rs`. |
| Impl-level where clauses | Done | `parser_generics.rs`. |
| Additional async render method generics | Expected fail | The canonical render method has no named method generics; put application generics on the `impl` header. |
| Additional async render method bounds | Done | Authored method where clauses are preserved; see `21_async_render_method_where_clause.rs`. |
| Tail `html_sync!` / `html!` validation | Done | `03_wrong_tail_macro`. |
| Source-ordered prelude binding | Done | One lexical walk binds declarations after their initializers, retains exact binding IDs through shadows, captures named-handler environments at declaration, and lowers queries/mutations/intervals in authored order. |
| Arbitrary server prelude statements | Done | The authored method body, control flow, `?`/`return`, evaluation/drop order, `.render(cx)` chain, and async no-work-before-poll behavior are preserved; only compiler-owned declarations and the analyzed template are rewritten. |

### Reactive template expressions

| Feature | Status | Notes |
|---|---|---|
| Literal values | Done | Integer, float, string, bool. |
| Signal reads via `.get()` | Done | Text and attributes subscribe. |
| Signal projections | Done | `is_non_empty`, string lowering paths, projection chain tests. |
| Binary and boolean expressions | Done | Used in bindings and handler conditions. |
| Unary `!` over supported bools | Done | Used in reactive attrs. |
| Value-level `if { ... } else { ... }` | Done | Text, attributes, and browser-effect args. |
| Value-level `if` without `else` | Expected fail | Conformance fail `24_value_if_missing_else`; use `if { ... } else { ... }` so the client expression has a value in both cases. |
| Conditional DOM sections | Done | `if` / `else` sections and multiple sibling conditionals per parent. |
| Multiple conditional sections on same parent | Done | Branch markers are cumulatively indexed per parent. |
| Dynamic `href` in islands | Expected fail | Conformance fail `13_dynamic_url_attr`; use a static value or `rsjs::server_attr(...)` for vetted server-rendered URLs. |
| `rsjs::server_only(...)` | Compatibility | Explicit opaque SSR boundary retained for syntax tolerant inference cannot yet classify; routine server values should render directly. |
| `rsjs::server_attr(...)` | Done | Server-rendered dynamic attributes outside client IR. |
| Signal reads inside `rsjs::server_only(...)` | Expected fail | Conformance fail `36_server_only_signal_capture`; move the signal read into a normal reactive slot or pass plain server data. |
| Signal reads inside `rsjs::server_attr(...)` | Expected fail | Conformance fail `37_server_attr_signal_capture`; use a normal dynamic attribute for client-updated values. |
| Parent signal captured by `rsjs::island(...)` | Expected fail | Conformance fail `41_island_signal_capture`; explicit child islands need plain server data or independent child state. |
| State declarations inside opaque boundaries | Expected fail | Conformance fails `38_server_only_state_declaration`, `39_server_attr_state_declaration`, and `40_island_state_declaration`; declare state in the parent or independent child island scope. |
| Client primitives inside opaque server boundaries | Expected fail | UI fail `51_server_only_client_primitive`; declare primitives before the template or in an independent child island scope. |
| Browser helpers inside opaque server boundaries | Expected fail | UI fail `52_server_only_browser_helper`; move browser work into reachable Client IR or use server-derived SSR data. |
| Browser read helpers | Done | `location::*()`, URL query-string reads, plus local/session storage `get_item`/`get_item_or`. |
| Encoding helpers | Partial | `utf8_bytes`, `utf8_string`, base64, and unpadded base64url lower through typed IR. |
| Direct inbound `self.field` reads | Done | Direct text, attribute, branch, handler, and presentational pass-through uses retain `LiveRead` timing when linked to a live parent value. Without a live producer they remain SSR without forcing JavaScript. Signal initializers are `SeedOnce`, query-key uses are `QueryKeyDependency`, and explicit server boundaries remain per-render snapshots. |
| Inline closure/lambda in reactive expression | Expected fail | Conformance fail `03_reactive_closure`. |
| Unmarked helper call in reactive expression | Link-stage fail | Rust-resolved fixture `tests/ui/linker/fail/unmarked_handler_helper.rs`; the descriptor linker must report the missing `#[rsjs::client]` definition. |

### Signals

| Feature | Status | Notes |
|---|---|---|
| `let count = signal(initial)` | Done | Initializers support literals and supported prop-derived values. |
| `signal.get()` | Done | Reactive read. |
| `signal.set(value)` | Done | Handler write. |
| `signal.update(|x| expr)` | Done | Single-arg closure with constrained expression body. |
| `signal.update` body calling `.set(...)` | Expected fail | Conformance fail `28_update_with_set_in_body`; use `signal.set(value)` as a handler statement. |
| Dead signal elimination | Done | `rsjs_ir` optimization tests. |
| Handler dedup | Done | `rsjs_ir` optimization tests. |

### Event handlers

| Feature | Status | Notes |
|---|---|---|
| Inline `on_*={|event| ...}` closures | Done | Naked closures with zero or one event param. |
| Typed event params | Done | `Event`, `MouseEvent`, `PointerEvent`, `KeyboardEvent`, `InputEvent`, `SubmitEvent`, `ChangeEvent`, `ClipboardEvent`, `TouchEvent`, `WheelEvent`, `CompositionEvent`, `DragEvent`, `ToggleEvent`, `AnimationEvent`, `TransitionEvent`, `PopStateEvent`, `HashChangeEvent`, `StorageEvent`. |
| Multi-arg handlers | Expected fail | Conformance fail `15_handler_ref_arity`; named handler refs must match the declared payload arity. |
| Handler blocks | Done | Statement blocks are supported. |
| Handler-local `let` | Done | Locals for supported values lower to real JS `let` declarations and local reads. |
| Handler-local `let mut` reassignment | Done | Plain local assignments and arithmetic compound assignments such as `next = next + 1` and `next += 1` lower to JS assignment statements. |
| Handler-local `let ... else` | Expected fail | Conformance fail `11_handler_let_else`; rewrite with supported `if` / `else` for now. |
| Handler `if` / `else` | Done | Nested `else if` supported. |
| Handler `match` | Partial | Boolean, literal/range, and `Option` presence/payload matches with supported guards lower to structured `if` / `else`; path patterns and nested destructuring remain expected-fail. |
| Handler `for` loops | Partial | Static arrays/ranges are unrolled and dynamic numeric ranges lower to JS `for`; non-range dynamic iterators remain expected-fail. |
| Iterator `.map(...)` | Expected fail | Conformance fail `05_iterator_map`. |
| `event.prevent_default()` | Done | Statement only. |
| `event.stop_propagation()` | Done | Statement only. |
| Unknown event methods | Expected fail | Conformance fail `16_event_unknown_method`; event params expose the typed rsjs event API, not raw DOM methods. |

Supported event attributes:

```rust
on_click
on_input
on_change
on_paste
on_focus
on_blur
on_keydown
on_keyup
on_mousedown
on_mouseup
on_mousemove
on_mouseover
on_mouseout
on_scroll
on_wheel
on_beforeinput
on_compositionstart
on_compositionupdate
on_compositionend
on_dragstart
on_dragenter
on_dragover
on_dragleave
on_drop
on_dragend
on_toggle
on_beforetoggle
on_pointerdown
on_pointerup
on_pointermove
on_pointerover
on_pointerout
on_pointerenter
on_pointerleave
on_gotpointercapture
on_lostpointercapture
on_touchstart
on_touchmove
on_touchend
on_touchcancel
on_document_click
on_document_pointerdown
on_document_keydown
on_window_popstate
on_window_hashchange
on_window_storage
on_window_resize
on_window_scroll
on_submit
```

Supported event value accessors:

```rust
event.type_()
event.value()
event.checked()
event.target_value()
event.target_checked()
event.target_date()
event.target_id()
event.current_target_id()
event.current_target().id()
event.current_target().value()
event.current_target().checked()
event.target_attr("data-id")
event.closest_attr("data-id")
event.current_target_attr("data-id")
event.key()
event.code()
event.repeat()
event.shift_key()
event.meta_key()
event.ctrl_key()
event.alt_key()
event.client_x()
event.client_y()
event.page_x()
event.page_y()
event.screen_x()
event.screen_y()
event.offset_x()
event.offset_y()
event.movement_x()
event.movement_y()
event.button()
event.buttons()
event.detail()
event.detail_u32()
event.pointer_id()
event.pointer_type()
event.is_primary()
event.pressure()
event.tangential_pressure()
event.tilt_x()
event.tilt_y()
event.twist()
event.pointer_width()
event.pointer_height()
event.delta_x()
event.delta_y()
event.delta_z()
event.delta_mode()
event.current_target_scroll_top()
event.current_target_scroll_left()
event.input_type()
event.data()
event.old_state()
event.new_state()
event.old_url()
event.new_url()
event.storage_key()
event.storage_old_value()
event.storage_new_value()
event.storage_url()
event.storage_area()
event.clipboard_text()
event.data_transfer_text()
event.data_transfer_file_count()
event.data_transfer_first_file_name()
event.data_transfer_first_file_type()
event.data_transfer_first_file_size()
event.data_transfer_file_name_at(index)
event.data_transfer_file_type_at(index)
event.data_transfer_file_size_at(index)
event.data_transfer_item_count()
event.data_transfer_item_kind_at(index)
event.data_transfer_item_type_at(index)
event.target_file_count()
event.target_first_file_name()
event.target_first_file_type()
event.target_first_file_size()
event.target_file_name_at(index)
event.target_file_type_at(index)
event.target_file_size_at(index)
event.changed_touch_count()
event.changed_touch_identifier()
event.changed_touch_client_x()
event.changed_touch_client_y()
event.changed_touch_page_x()
event.changed_touch_page_y()
event.changed_touch_screen_x()
event.changed_touch_screen_y()
event.changed_touch_identifier_at(index)
event.changed_touch_client_x_at(index)
event.changed_touch_client_y_at(index)
event.changed_touch_page_x_at(index)
event.changed_touch_page_y_at(index)
event.changed_touch_screen_x_at(index)
event.changed_touch_screen_y_at(index)
event.target_touch_count()
event.target_touch_identifier_at(index)
event.target_touch_client_x_at(index)
event.target_touch_client_y_at(index)
event.target_touch_page_x_at(index)
event.target_touch_page_y_at(index)
event.target_touch_screen_x_at(index)
event.target_touch_screen_y_at(index)
event.touch_count()
event.touch_identifier_at(index)
event.touch_client_x_at(index)
event.touch_client_y_at(index)
event.touch_page_x_at(index)
event.touch_page_y_at(index)
event.touch_screen_x_at(index)
event.touch_screen_y_at(index)
event.target_inside(root)
event.target_outside(root)
```

Supported event statement methods:

```rust
event.prevent_default()
event.stop_propagation()
event.target_first_file_text_into(&signal)
event.target_first_file_text_into_or(&signal, fallback)
event.data_transfer_first_file_text_into(&signal)
event.data_transfer_first_file_text_into_or(&signal, fallback)
```

Static attribute-name rules:

| Shape | Status |
|---|---|
| `event.target_attr("data-id")` | Done |
| `event.closest_attr("data-id")` | Done |
| `event.current_target_attr("data-id")` | Done |
| Dynamic event attribute name | Expected fail | Conformance fail `22_event_attr_dynamic`; static string literal attribute names keep generated selectors explicit. |

### Browser read/effect helpers

| Helper | Status |
|---|---|
| `rsjs::location::href()` | Done |
| `rsjs::location::origin()` | Done |
| `rsjs::location::protocol()` | Done |
| `rsjs::location::host()` | Done |
| `rsjs::location::hostname()` | Done |
| `rsjs::location::port()` | Done |
| `rsjs::location::pathname()` | Done |
| `rsjs::location::search()` | Done |
| `rsjs::location::hash()` | Done |
| `rsjs::window::scroll_to(x, y)` | Done |
| `rsjs::window::scroll_by(x, y)` | Done |
| `rsjs::window::print()` | Done |
| `rsjs::location::set_hash(value)` | Done |
| `rsjs::location::assign(url)` | Done |
| `rsjs::location::replace(url)` | Done |
| `rsjs::location::reload()` | Done |
| `rsjs::history::push_state(url)` | Done |
| `rsjs::history::replace_state(url)` | Done |
| `rsjs::history::back()` | Done |
| `rsjs::history::forward()` | Done |
| `rsjs::history::go(delta)` | Done |
| `rsjs::document::dispatch_event(name)` | Done |
| `rsjs::document::dispatch_event_with_detail(name, detail)` | Done |
| `rsjs::document::set_title(title)` | Done |
| `rsjs::clipboard::write_text(text)` | Done |
| `rsjs::clipboard::read_text_into(&signal)` | Done |
| `rsjs::clipboard::read_text_into_or(&signal, fallback)` | Done |
| `rsjs::webauthn::create_json_into(options_json, &signal)` | Done |
| `rsjs::webauthn::create_json_into_or(options_json, &signal, fallback)` | Done |
| `rsjs::webauthn::get_json_into(options_json, &signal)` | Done |
| `rsjs::webauthn::get_json_into_or(options_json, &signal, fallback)` | Done |
| `rsjs::local_storage::get_item(key)` | Done |
| `rsjs::local_storage::get_item_or(key, fallback)` | Done |
| `rsjs::local_storage::get_bool_or(key, fallback)` | Done |
| `rsjs::local_storage::get_i64_or(key, fallback)` | Done |
| `rsjs::local_storage::get_f64_or(key, fallback)` | Done |
| `rsjs::local_storage::set_item(key, value)` | Done |
| `rsjs::local_storage::remove_item(key)` | Done |
| `rsjs::session_storage::get_item(key)` | Done |
| `rsjs::session_storage::get_item_or(key, fallback)` | Done |
| `rsjs::session_storage::get_bool_or(key, fallback)` | Done |
| `rsjs::session_storage::get_i64_or(key, fallback)` | Done |
| `rsjs::session_storage::get_f64_or(key, fallback)` | Done |
| `rsjs::session_storage::set_item(key, value)` | Done |
| `rsjs::session_storage::remove_item(key)` | Done |
| `rsjs::url::search_param(key)` | Done |
| `rsjs::url::search_param_or(key, fallback)` | Done |
| `rsjs::url::set_search_param(key, value)` | Done |
| `rsjs::url::push_search_param(key, value)` | Done |
| `rsjs::url::delete_search_param(key)` | Done |
| `rsjs::url::push_delete_search_param(key)` | Done |
| `rsjs::encoding::utf8_bytes(text)` | Done |
| `rsjs::encoding::utf8_string(bytes)` | Done |
| `rsjs::encoding::base64_encode(bytes)` | Done |
| `rsjs::encoding::base64_decode(text)` | Done |
| `rsjs::encoding::base64url_encode(bytes)` | Done |
| `rsjs::encoding::base64url_decode(text)` | Done |
| `console::{log,info,debug,warn,error,table,time,time_end,group,group_end}` | Done |
| Arbitrary `window` / `document` calls | Expected fail | Conformance fail `06_browser_global`; use typed RSJS browser helpers or an application-owned controller. |
| `fetch` / WebCrypto / raw DOM traversal | Expected fail | Use query/mutation for fetch. Other capabilities need an explicit typed RSJS surface or application-owned JavaScript; origin-free client helpers cannot provide them. |

### Node refs

Supported marker types:

```rust
rsjs::Element
rsjs::HtmlElement
rsjs::HtmlInputElement
rsjs::HtmlTextAreaElement
rsjs::HtmlSelectElement
rsjs::HtmlFormElement
rsjs::HtmlDialogElement
```

Supported value accessors:

```rust
node.id()
node.text_content()
input.value()
input.checked()
input.check_validity()
input.validation_message()
input.validity_valid()
input.will_validate()
input.selection_start()
input.selection_end()
node.scroll_top()
node.scroll_left()
node.client_width()
node.client_height()
node.scroll_width()
node.scroll_height()
node.offset_width()
node.offset_height()
node.rect_x()
node.rect_y()
node.rect_top()
node.rect_right()
node.rect_bottom()
node.rect_left()
node.rect_width()
node.rect_height()
input.file_count()
input.first_file_name()
input.first_file_type()
input.first_file_size()
node.get_attribute("data-id")
node.has_attribute("hidden")
```

Supported statement methods:

```rust
node.focus()
node.blur()
node.scroll_into_view()
node.click()
node.scroll_to(x, y)
node.scroll_by(x, y)
node.set_pointer_capture(pointer_id)
node.release_pointer_capture(pointer_id)
node.set_text_content(value)
node.add_class(class_name)
node.remove_class(class_name)
node.toggle_class(class_name)
node.set_attribute("aria-expanded", value)
node.remove_attribute("hidden")
node.set_style_property("background", value)
input.select()
input.select_all()
input.set_value(value)
input.set_checked(value)
input.set_custom_validity(message)
input.report_validity()
input.set_selection_range(start, end)
form.request_submit()
form.reset()
form.report_validity()
dialog.show_modal()
dialog.close()
popover.show_popover()
popover.hide_popover()
popover.toggle_popover()
```

Known node-ref boundaries:

| Shape | Status | Notes |
|---|---|---|
| `ref={input}` on stable element | Done |
| Required `NodeRef<T>` field on a static direct composed child | In progress | One direct parent/child edge shares an authenticated browser object and owner through rustc-resolved witnesses. Pure participant-only pass-through chains, delegated browser-event acceptance, and observed teardown ordering remain before this slice is complete. |
| Standalone, optional, explicit-island, conditional, repeated, keyed, or multiply-bound cross-component ref | Expected fail | The first handle ABI intentionally admits only one required alias chain with one stable terminal binding. |
| `ref={...}` inside template loop | Expected fail | Conformance fail `10_ref_in_template_loop`; node refs target a single stable DOM handle. |
| Unknown node-ref method | Expected fail | Conformance fail `21_node_ref_unknown_method`; node refs expose a closed typed DOM API. |
| Dynamic attribute name for read/write | Expected fail | Conformance fails `25_node_ref_dynamic_attribute_name` and `26_node_ref_dynamic_attribute_read`; static names keep emitted DOM operations explicit. |

### Component composition and callbacks

| Feature | Status | Notes |
|---|---|---|
| Required callback prop `rsjs::Handler<T>` | Done | Child calls `self.on_change.call(payload)`. |
| Optional callback prop `Option<rsjs::Handler<T>>` | Done | Child calls `.call_if_some(payload)`. |
| Local handler ref `let h = rsjs::handler(...)` | Done | Parent passes to child. |
| `rsjs::noop_handler()` | Done | Useful at explicit island boundaries. |
| Parent inlines child rsjs IR | Done | Descriptor-backed inlining. |
| Explicit child island via `rsjs::island(...)` | Done | Prevents inlining. |
| Inbound child value props | Done | Direct child `self.field` / field-path uses carry typed linked provenance and `LiveRead` timing instead of being classified as unconditional server props. Use `server_attr` / `server_only` for an intentional snapshot and construct arbitrary opaque SSR renderers in the prelude. |
| Reactive child props seeded into signals | Done | Calendar/controlled component pattern. |
| Static direct child `NodeRef<T>` handle | In progress | The direct parent/child path works without serialization, another owner, or another ref allocation. The linker is gaining deferred inbound-projection resolution so a pure middle component can forward the same handle without syntax-order inference. |
| Template loops in child components | Partial | Repeated text, attrs, form controls, conditionals, and event handlers, including submit handlers. |
| Handler prop without inlining | Expected fail | Conformance fail `19_component_handler_prop_without_inlining`; needs descriptor-backed inlining or server-rendered content. |
| Handler prop crossing island boundary | Expected fail | Conformance fail `20_island_boundary_handler_prop`; render directly, give the child independent state, or pass `rsjs::noop_handler()`. |
| Arbitrary `self.method(...)` in handler | Expected fail | Conformance fail `17_self_non_callback_method`; only `self.on_*.call(payload)` callback-slot calls lower into client handlers. |
| Server-only value captured by handler | Expected fail | Conformance fail `18_server_only_handler_capture`; handler props cannot cross opaque `server_only` boundaries. |
| Async child components | Partial | Sync parents still reject async child components with guided diagnostics. Async parents can inline descriptor-backed async children, including awaited server prelude values expanded into server-only descriptor slots. |
| Query/mutation child components | Partial | Descriptor-backed async parents can inline query/mutation child declarations, reads, `run(...)`, `refetch()`, and `invalidate()` handlers. Child query initializers, including `query_with_initial(...)` values loaded through `cx`/`DataLoader`, are remapped into the parent and dehydrated under the parent query id. Child mutation metadata with server-local keys/invalidations is remapped into the parent mutation metadata blob. Sync parents remain an expected fail (`35_query_mutation_child_component`). |
| Shared component fragment chunks | Partial | Descriptor-backed child fragments can emit shared chunks and parent islands can import/mount them. Server-only child descriptors inline as SSR; hydrated descriptor fragments still require parent JS in schema v4, even when self-contained. Query/mutation children force full inlining today so parent-owned state and metadata share one id space. |

### Client helpers

| Feature | Status | Notes |
|---|---|---|
| `#[rsjs::client]` pure helpers | Partial | Constrained origin-free bodies, including typed `let` / `let mut` locals and checked assignment, are carried through the semantic plan and final-linked into deterministic private functions; unresolved symbolic edges fail final emission. |
| `#[rsjs::client_module]` bundles | Partial | Optional bundling/namespace metadata; it neither grants helper eligibility nor needs a consumer declaration. |
| Helper calls from reactive slots | Partial | The function itself must carry `#[rsjs::client]`; reachable Rust-resolved edges are final-linked with the exact transitive helper closure, without a component allowlist. |
| Helper calls from handler bodies | Partial | Same constrained IR vocabulary and final-link contract. |
| Standalone unmarked helper call | Link-stage fail | Rust-resolved fixture `tests/ui/linker/fail/unmarked_reactive_helper.rs`; `client_module` is not required, but the function definition must carry `#[rsjs::client]`. |
| General Rust helper body | Planned via Client IR v3/HIR, not current IR. |

### Query and mutation system

| Feature | Status | Notes |
|---|---|---|
| Query IR side table | Partial | `rsjs_ir` supports declarations and accessors. |
| Query JS emitter | Partial | Unit tests cover registration and reads. |
| Query runtime asset split | Partial | Manifest/runtime asset plumbing exists. |
| Query macro authoring | Partial | Sync or async islands declare queries through `rsjs::query_with_initial(...)`; use `WebRenderSync` when the `Serialize` initial value is already available, or load it through ordinary async render code such as `cx.load_json(...).await?`. Query-data field access like `price.data().currency`, explicit projection chains, and handlers that call `query.refetch()` / `query.invalidate()` as standalone statements lower through the normal macro path. The older `rsjs::query(...)` and `query_with_options(...)` static-JSON forms have been removed. Query and mutation declarations accept legacy single-expression keys or structural `rsjs::QueryKey::new((...))` tuple/array keys. Runtime server initializers seed the framework-level query seed table and initial SSR text/attribute/conditional output; adjacent per-instance query blobs carry effective key locators so `query_with_initial(...)` can use server locals such as `symbol.as_str()` in `QueryKey::new(...)`. Client fetchers accept literal URLs or static `rsjs::FetchRequest` descriptors with method, headers, credentials, body, timeout, response decoding, opt-in download progress, and opt-in streaming text for text responses. FetchRequest/options, progress, text-stream authoring, structural keys, manual refetch/invalidate, typed query-data field access, DataLoader-backed query initials, per-instance runtime key location, and page-level query seed-table hydration are covered by tests; stream-text response validation is covered by conformance fail `32_query_stream_text_requires_text_response`. |
| Query in sync `WebRenderSync` island | Supported | UI pass `28_query_in_sync_island`; the initial value must already be available because sync render cannot `.await`. |
| Inline `rsjs::server_fetcher(...)` query initializer | Expected fail | Conformance fail `42_query_server_fetcher`; load through the render `cx` and pass the value to `query_with_initial(...)`. |
| Mutation IR side table | Partial | `rsjs_ir` supports declarations and accessors. |
| Mutation JS emitter | Partial | Unit tests cover `run(...)`, state reads, invalidation metadata. |
| Mutation runtime asset split | Partial | Shared with query runtime assets. |
| Mutation macro authoring | Partial | Async islands can declare `rsjs::mutation(...)` / `mutation_with_options(...)`; handlers can call `mutation.run(arg)` and bindings can read mutation state. Client runners accept literal URLs, JSON config strings, or static `rsjs::FetchRequest` descriptors with method, headers, credentials, timeout, fixed bodies, and response decoding. Mutation keys and invalidation keys can use server locals such as `symbol.as_str()` by serializing the effective keys into per-instance mutation metadata. FetchRequest/options authoring and query invalidation are covered by conformance pass `23_query_mutation_authoring` plus runtime coverage. |
| Mutation in sync `WebRenderSync` island | Expected fail | Conformance fail `12_mutation_in_sync_island`; use async `WebRender` and `html!` for mutation declarations. |

### Runtime and build pipeline

| Feature | Status | Notes |
|---|---|---|
| Per-island JS source registration | Done |
| Manifest JSON serialization | Done |
| Component descriptor JSON | Done |
| Content-hashed asset URLs | Done |
| Route asset graph | Done |
| Event delegation | Done | One listener per owner/event type; marker lookup is scoped by exact component/explicit-island ownership, so colliding local handler IDs in nested owners cannot dispatch through the parent. |
| Microtask batching | Done |
| Index-walk hydration | Done |
| Browser effect conformance | Done |
| Runtime Trusted-Types-clean style | Done | No `innerHTML`, `eval`, or `Function` in current runtime path. |
| CSSOM/UI event surface | Partial | `on_scroll`, `on_wheel`, `on_beforeinput`, composition events, drag/drop events, toggle/beforetoggle events, animation/transition events, pointer enter/leave and capture events, pointer field/capture helpers, wheel delta accessors, current-target scroll reads, file metadata/text helpers, element scroll/size/geometry accessors, window resize events, and window viewport/scroll reads are supported. |
| Production minification/precompression | Planned | See `OPTIMIZATIONS.md`. |
| Hydration strategies | Partial | Runtime entry points exist for immediate, visible, idle, first-interaction, and media-query hydration with cancellable pending work. `#[rsjs(component, client, strategy = "...")]` threads strategy metadata through the macro, IR, JS emitter, rsjs manifest JSON, conformance strategy fixtures/snapshots, and the generic framework bootstrap. RSJS asset helpers plus the catalog, playground, and lean benchmark adapters defer visible/idle/media island module fetch/parse until the strategy trigger; interaction remains eagerly loaded so the first event can hydrate during capture. |
| Disposer/unsubscribe lifecycle | Done | Runtime-owned resources and returned initializer cleanup are additive, reverse-ordered, exception-isolated, and once-only. Nested hydrated/pending owners dispose deepest-first; structural/keyed removal disposes nested owners before item resources and DOM removal; route/page teardown also drains query and SharedState subscriptions. |
| Conditional component lifecycle | Partial | One authenticated conditional may govern a direct child plus its static descendants. The linked owner mounts only the active resource group, disposes child-first while retaining SSR DOM, samples `SeedOnce` on each mount, preserves `LiveRead` aliases/current callbacks, and rolls back failed mounts. Repeated, keyed, nested-conditional, and conditionally lazy module loading remain open. |
| Seed-state page-level tables | Partial | Framework finalizers append `id="rsjs-query-seed-table"` for query seeds and `id="rsjs-shared-state-seed-table"` for typed provider scopes. Shared-state consumers carry exact scope/schema bindings in their adjacent island blob; existing live cells win over repeated installation so late hydration cannot restore stale SSR values. Island-local signal seed-table consolidation is still planned. |

## Known gaps and expected failures

### Language/control-flow gaps

| Gap | Status | Current guidance |
|---|---|---|
| Path-pattern or nested-destructuring `match` in handler bodies | Expected fail | Use supported boolean/literal/range/`Option` matches, rewrite as `if` / `else`, or move to a marked client helper. |
| Non-range dynamic loops in handlers | Expected fail | Use template loops, supported numeric ranges, split handlers, or a marked client helper. |
| Iterator helpers like `.map(...)` | Expected fail | Move collection logic into a marked client helper. |
| Inline closures/lambdas in reactive slots | Expected fail | Use a marked client helper. |
| General function calls | Expected fail unless marked | Mark portable client computation with `#[rsjs::client]`; keep server-only computation in the prelude. |
| `let ... else` in handlers | Expected fail | Rewrite with supported control flow. |
| `?` / general `Result` propagation in client DSL | Planned | Needs Client IR v3/HIR before committing. |
| `while` / arbitrary loops | Planned | Needs Client IR v3/HIR. |
| General Rust borrowing/moves in client DSL | Out of scope | Client IR is not a Rust compiler. |

### DOM/template gaps

| Gap | Status | Notes |
|---|---|---|
| Multiple conditional sections under one parent | Done | Branch markers are cumulatively indexed per parent. |
| `ref={...}` in template loops | Expected fail | One ref currently maps to one stable DOM node. |
| Repeated input bindings | Done | Loop-rendered controls bind every matching input marker to the declared signal. |
| Repeated submit handlers | Done | Loop-rendered forms bind every matching handler marker with `bind_submit`. |
| Dynamic URL attributes in islands | Expected fail | Use static URLs or server-only vetted content. |
| Dynamic attribute names for event/ref APIs | Expected fail | Static string literal names only; conformance covers event reads plus node-ref reads/writes. |

### Browser/API gaps

| Gap | Status | Notes |
|---|---|---|
| Arbitrary `document.querySelector(...)` | Expected fail | Conformance fail `06_browser_global`; use node refs or an application-owned controller. Pure `#[rsjs::client]` helpers cannot access raw DOM globals. |
| Raw `fetch` | Expected fail | Conformance fail `07_raw_fetch`; use `rsjs::query_with_initial(...)` / `rsjs::mutation(...)`. Pure `#[rsjs::client]` helpers are not a network escape hatch. |
| WebAuthn / Credential Management | Partial | Client-side registration/authentication ceremonies work through `rsjs::webauthn::*_json_into*` helpers that accept server JSON options and write credential JSON into a signal. Target validation is covered by conformance fail `33_webauthn_target`; server challenge/response helpers, compatibility fallbacks for browsers without WebAuthn Level 3 JSON helpers, and policy diagnostics remain open. |
| WebCrypto | Planned | Needs browser API escape hatch or typed helper. |
| Rich ArrayBuffer/DataView helpers | Planned | The first `Bytes`/`Uint8Array` shape exists for encoding; broader binary APIs should land with WebAuthn/WebCrypto pressure. |
| Arbitrary `window` / `document` assignment | Expected fail | Add a narrow helper when the API is common and safe. |
| Typed storage parsing | Partial | Boolean, canonical full-domain `i64` decimal, and finite-f64 reads have explicit fallback semantics; `i64` becomes BigInt on the client. Richer JSON/object parsing can wait for concrete pressure. |
| Full TouchList traversal | Planned | Basic touch events plus indexed changed/target/all touch scalar reads are implemented; multi-touch list iteration should wait for Client IR v3 loop lowering. |

### Component/chunking gaps

| Gap | Status | Notes |
|---|---|---|
| Shared component fragment chunk system | Partial | Core manifest/codegen/mount path exists; Calendar uses the shared fragment path; playground/catalog/framework asset paths hash, serve, preload, and smoke-test route-used fragments. Bundle-size tooling accounts for static import graphs and splits runtime/island/client-module/fragment bytes; remaining work is broader component migration. |
| Component-level tree shaking/minification integration | Partial | Playground minifies fragment chunks and rewrites parent imports to content-hashed fragment URLs; catalog/framework hash and preload route-used fragments; the bundle-size harness can enforce optional rsjs total/fragment min-gzip budgets. Generalized build-pipeline tree shaking is still planned. |
| Cross-route fragment dedupe | Partial | Parent modules share registered fragment assets by ID, and catalog smoke verifies one shared Calendar fragment fetch across multiple island parents. Broader route accounting and budget gates remain. |
| Source maps from Rust DSL to generated JS | Planned | Needs HIR/source span model. |

### Conformance gaps

| Gap | Status | Notes |
|---|---|---|
| Machine-readable feature matrix | Partial | Seed manifest in `rsjs_conformance/features.json`; validator keeps it in sync with trybuild fixtures, validates compile-fail stderr snapshots plus claimed, supporting, and unique snapshot files, enforces coverage gates, requires compile-pass features to carry JS snapshots plus runtime coverage, requires descriptor snapshots for compile-pass descriptor-relevant source positions, syntax-checks JS snapshots/runtime scripts, validates body-level Client IR snapshots, validates descriptor v3 handler/body references, and validates ClientModule IR snapshot structure/reachability. CI runs the manifest check, trybuild fixtures, macro HIR/IR snapshots, rsjs emitted artifact snapshots, and the runtime conformance suite. |
| Coverage percentages | Partial | `node rsjs_conformance/report.mjs` prints status/category/coverage/snapshot counts, and `--check` validates manifest status/coverage/snapshot consistency, snapshot path/JSON/source-span shape, JS snapshot/runtime-script syntax, all snapshot-file ownership, minimum coverage gates, and compile-pass JS snapshot coverage for CI. Richer percentage gates are still planned. |
| HIR snapshots | Partial | Initial macro-side handler HIR exists; basic event-to-signal handlers, named event-handler refs, component callbacks, server-boundary callbacks, node refs, template repetition payload/global-event handlers, query/mutation run handlers, and nested handler control flow have normalized HIR snapshots. Real macro-expanded islands now also emit `target/rsjs/hir/*.json` sidecars, with conformance gates for basic, named, control-flow, browser-effect, node-ref, query/mutation, server-boundary, component-callback, and template-repetition handler HIR plus component-fragment callback adapter HIR. Handler HIR snapshots and client-helper BODY_HIR snapshots now carry concrete expression source spans; direct handler statement payloads and browser/node-ref effect operands preserve their own expression spans instead of inheriting statement spans, and the manifest gates expression-span presence for `.hir.json` / `.body-hir.json` snapshots. Remaining work is richer, per-subexpression span precision where handler HIR still lowers through compatibility expressions. |
| Typed Client IR v3 snapshots | Partial | Basic event-to-signal handlers, named event-handler refs, component callbacks, server-boundary callbacks, node refs, template repetition payload/global-event handlers, query/mutation run handlers, client-helper early returns, and handler control flow now check typed Client IR; real macro-expanded islands also emit `target/rsjs/client_ir/*.json` sidecars, with conformance gates for basic, named, control-flow, browser-effect, node-ref, query/mutation, and server-boundary handler bodies plus component-fragment callback adapters. Basic/named/control-flow handler JS, browser-effect JS, node-ref JS, query/mutation JS, server-boundary parent JS, template repetition fragment/global-event JS, component callback parent JS, helper JS, and component/server-boundary/template-repetition descriptor snapshots cover emitted artifacts. Body IR snapshots and descriptor handler bodies are now parsed/gated as Client IR v3 blocks; descriptor snapshots also structurally gate event binding references. Client-helper early-return HIR/IR/bundle snapshots now compare against the actual helper lowering and module emitter, and the generated optimized client-module graph is pinned by a module-level Client IR snapshot with structure/export/reachability validation. Module-level dead function pruning now runs before client-module JS emission; broader module optimization coverage remains. |
| Runtime conformance | Partial | `rsjs_conformance/runtime/*.mjs` executes generated basic, named event-handler, control-flow handler, query/mutation, component-callback, server-boundary, node-ref, browser-effect, template-repetition, repeated global-event, and client-helper JS against the runtime and a tiny DOM/browser shim where needed. Broader browser/runtime scenarios remain. |
| Pass/fail fixture pairing by feature ID | Partial | Every conformance pass/fail fixture carries a stable `case_id` and the JS/Rust validators require each fixture to be mapped exactly once under a feature ID; richer status-transition tooling is still planned. |

## Recommended next tracks

1. **Client IR v3 HIR and descriptor migration.** Handler and callback adapter
   lowering now produce Client IR v3 directly, helpers carry v3 metadata, and
   descriptors serialize and require v3 bodies. Next compiler-core work is
   broader handler/module coverage and nested source-span precision.
2. **Conformance snapshots and CI gating.** Keep broadening fixture-level
   HIR/IR, descriptor, emitted JS, runtime, and sidecar gates so supported
   features cannot silently regress.
3. **Hydration/disposer lifecycle.** Needed for visible/idle/interaction
   hydration strategies. Runtime cleanup now tracks hydrated roots, disposes
   stale duplicate hydrations, exposes targeted route-transition cleanup, and
   tears down query interval/focus policies after the last observer unsubscribes.
   Runtime visible/idle/interaction/media hydration now has cancellable pending
   work, and `#[rsjs(component, client, strategy = "...")]` reaches emitted island JS, manifest
   metadata, conformance strategy fixtures/snapshots, and the generic framework
   strategy descriptor. Catalog/playground/lean asset bootstraps now defer
   visible/idle/media island module fetch/parse until the strategy trigger while
   keeping interaction islands eager for first-event correctness.
4. **Broaden shared component fragment chunks.** Compiler-side codegen,
   Calendar migration, hashed asset serving, modulepreloads, browser
   verification, size accounting, and optional budget gates exist; next is
   migrating another heavy component once the first three tracks stay green.
5. **Query/mutation macro authoring.** Promote the existing IR/runtime scaffold
   into the normal authoring surface once the core DSL stabilizes.
6. **Production asset polish.** Minification, precompressed siblings, and
   budget gates belong after the emitted module graph is stable.
7. **WebAuthn server ceremony demo.** Add an end-to-end registration/login
   sample with server challenge generation and response verification once a
   concrete auth crate is chosen.

## Out of scope

rsjs should not promise to compile arbitrary Rust to JavaScript.

Out of scope for the client DSL:

- arbitrary traits / dynamic dispatch;
- arbitrary crates;
- unsafe Rust;
- borrow-sensitive Rust semantics;
- arbitrary async Rust in handlers;
- hidden server calls from client code;
- generic browser-global access without a typed rsjs helper;
- a cross-framework global signal store (typed `SharedStateProvider` scopes are
  page-local rsjs UI state, not a public foreign-framework store).

The goal is a small, explicit UI compiler surface with clear errors when a user
steps outside it.

## Update rules

When a feature lands:

1. Add or update an executable pass fixture when the feature is supported.
2. Add or update an executable fail fixture when a nearby shape is intentionally
   unsupported.
3. Update this file in the same commit.
4. If the feature changes user-facing authoring, update
   `docs/agent/api-reference/slot-grammar.md`.
5. If the feature closes a known real-world limitation, update
   `KNOWN_LIMITATIONS.md`.
