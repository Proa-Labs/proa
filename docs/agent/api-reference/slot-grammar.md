# `html_sync!` slot grammar

What's allowed inside `{ ... }` slots in `html_sync!` and `html!`. Different
rules apply depending on whether the surrounding render-trait impl is marked
with `#[rsjs(component)]` for analysis or is an opaque Proa component.

## Two grammars

| Slot context | Allowed in `{ ... }` | Where the rules live |
|---|---|---|
| Opaque Proa component (`WebRenderSync` / `WebRender`, no RSJS attribute) | Any Rust expression. The macro emits `<expr>.render(cx)` if the value implements the right render trait, otherwise it emits the value as text via `Display`. | [`proa_macros/src/lib.rs`](../../../proa_macros/src/lib.rs) (html_sync impl) |
| Inside an RSJS-analyzed render body (`#[rsjs(component)]` or `#[rsjs(component, client)]`) | A *constrained subset* of expressions; everything else is rejected at compile time | [`rsjs_macros/src/analyze/mod.rs`](../../../rsjs_macros/src/analyze/mod.rs) |

This page documents the constrained RSJS-analyzed grammar. For the unconstrained
opaque-component case, just write Rust — anything that type-checks and
implements `WebRenderSync<L>` / `Display` works. The advanced custom-writer
form may additionally specify `B`.

## What's allowed inside an RSJS-analyzed slot

### Literals

```rust
{42}             // contextual integer literal
{1.5}            // f64
{"hello"}        // &'static str
{true}           // bool
```

Compile to JS literals on the client, render as text on the server.

### Signal reads

```rust
{count.get()}                        // Signal<i64> → BigInt in JS
{username.get()}                     // String → string in JS
{username.is_non_empty()}            // bool projection
{label.to_lowercase().contains(query.get().to_lowercase())}
{if selected.get() { "Selected" } else { "Choose" }}
```

Auto-tracked: when the signal updates, the surrounding effect re-runs.

### Value-level conditionals

```rust
{if active.get() { "Active" } else { "Idle" }}
class={if active.get() { "active" } else { "idle" }}
on_click={|_| rsjs::history::replace_state(
    if active.get() { "/active" } else { "/idle" }
)}
```

These lower to JavaScript ternaries. Branches must be single supported reactive
expressions and an `else` branch is required so the expression always has a
value. Use template conditional sections when the branch should render DOM
nodes instead of return a value.

### Browser read helpers

```rust
{rsjs::location::href()}
{rsjs::location::pathname()}
{rsjs::location::search()}
{rsjs::location::hash()}
{rsjs::window::inner_width()}
{rsjs::url::search_param_or("filters", "all")}
on_click={|_| filter.set(rsjs::local_storage::get_item_or("filters", "all"))}
on_click={|_| draft.set(rsjs::session_storage::get_item("draft"))}
```

Supported location reads:

```rust
rsjs::location::href()
rsjs::location::origin()
rsjs::location::protocol()
rsjs::location::host()
rsjs::location::hostname()
rsjs::location::port()
rsjs::location::pathname()
rsjs::location::search()
rsjs::location::hash()
```

Supported window reads:

```rust
rsjs::window::inner_width()
rsjs::window::inner_height()
rsjs::window::scroll_x()
rsjs::window::scroll_y()
```

Supported storage reads:

```rust
rsjs::local_storage::get_item(key)
rsjs::local_storage::get_item_or(key, fallback)
rsjs::local_storage::get_bool_or(key, fallback)
rsjs::local_storage::get_i64_or(key, fallback)
rsjs::local_storage::get_f64_or(key, fallback)
rsjs::session_storage::get_item(key)
rsjs::session_storage::get_item_or(key, fallback)
rsjs::session_storage::get_bool_or(key, fallback)
rsjs::session_storage::get_i64_or(key, fallback)
rsjs::session_storage::get_f64_or(key, fallback)
```

Supported URL query-string reads:

```rust
rsjs::url::search_param(key)
rsjs::url::search_param_or(key, fallback)
```

Supported document-root reads:

```rust
rsjs::root::has_class("dark")
```

Reads `document.documentElement.classList`. The root element is page-wide
state that no island renders, so a `node_ref` cannot reach it; this is the
supported way to see a class the document shell or a pre-paint script set.
Renders `false` during SSR, so seed island state from it only where a
server-side `false` is the correct first paint.

Supported encoding helpers:

```rust
rsjs::encoding::utf8_bytes(text)
rsjs::encoding::utf8_string(bytes)
rsjs::encoding::base64_encode(bytes)
rsjs::encoding::base64_decode(text)
rsjs::encoding::base64url_encode(bytes)
rsjs::encoding::base64url_decode(text)
```

These lower to `window.location`, `window`, `Storage.getItem(...)`, and
`URLSearchParams` reads in the generated island JS. During SSR, location reads
render as `""`, window reads render as `0`, and storage/query-string reads
render as `""` or the explicit fallback. Browser storage and query-string reads
are wrapped in `try`/`catch` and return the fallback when the key is missing or
the browser API is unavailable. Typed storage reads parse stored JSON booleans
or numbers and return the fallback on parse/type mismatch. Encoding helpers lower to `TextEncoder`,
`TextDecoder`, `btoa`, and `atob`; byte values are `rsjs::Bytes` on the Rust
side and `Uint8Array` in generated JS.

### Conditional sections

```rust
{if predicate {
    html_sync! { ...content... }
}}
```

`predicate` must use the recognised reactive grammar (see "Predicates" below).
Each parent element may host **at most one** `{if ...}` child; wrap multiple
conditionals in their own containers (the user_form pattern).

### Closures in `on_*` handler slots

```rust
use rsjs::{elem, web::dom};

on_click={|_| count.update(|c| c + 1)}
on_submit={|_| submitting.set(true)}
on_change={|_| dropdown.set(false)}
on_paste={|event: rsjs::ClipboardEvent| pasted.set(event.clipboard_text())}
on_keydown={|event: rsjs::KeyboardEvent| key.set(event.key())}
on_pointerdown={|event: rsjs::PointerEvent| {
    console::log(event.client_x(), event.client_y());
}}
on_document_pointerdown={|event: rsjs::PointerEvent<dom::Document>| {
    if event.target_outside(root) {
        open.set(false);
    }
}}
on_click={|event: rsjs::MouseEvent| {
    console::log(event.client_x(), event.client_y());
}}
on_input={|event: rsjs::InputEvent<elem::Input>| {
    value.set(event.current_value());
}}
on_touchstart={|event: rsjs::TouchEvent| {
    console::log(event.changed_touch_client_x(), event.changed_touch_client_y());
}}
on_toggle={|event: rsjs::ToggleEvent| {
    console::log(event.old_state(), event.new_state());
}}
on_window_storage={|event: rsjs::StorageEvent<dom::Window>| {
    console::log(event.storage_key(), event.storage_new_value());
}}
on_window_hashchange={|event: rsjs::HashChangeEvent<dom::Window>| {
    console::log(event.old_url(), event.new_url());
}}
on_window_scroll={|_: rsjs::Event<dom::Window>| {
    scrolled.set(rsjs::window::scroll_y() > 0);
}}
```

Naked `|event|`-style closures only, with at most one parameter (the event,
which may be typed as `rsjs::Event`, `rsjs::MouseEvent`, `rsjs::PointerEvent`,
`rsjs::KeyboardEvent`, `rsjs::InputEvent`, `rsjs::SubmitEvent`,
`rsjs::ChangeEvent`, `rsjs::ClipboardEvent`, `rsjs::TouchEvent`,
`rsjs::WheelEvent`, `rsjs::CompositionEvent`, `rsjs::DragEvent`, or
`rsjs::ToggleEvent`, `rsjs::PopStateEvent`, `rsjs::HashChangeEvent`, or
`rsjs::StorageEvent` for IDE completion). These event types may carry a DOM
target marker generic. Local bindings use the bound handler element, e.g.
`rsjs::MouseEvent<rsjs::web::dom::HtmlButtonElement>` or
`rsjs::InputEvent<rsjs::web::dom::HtmlInputElement>`; global bindings use
`rsjs::web::dom::Document` or `rsjs::web::dom::Window`. The older
`rsjs::elem::{Button, Input}` spellings are compatibility aliases. The event
interface and supported target marker are rustc-checked at the `on_*` binding.
For element-local bindings, `rsjs::Event` and `rsjs::web::dom::Any` are the
intentional broad forms. Global bindings require an explicit `dom::Document`
or `dom::Window` marker. Multi-parameter closures, `match`,
and handler-local loops are rejected.
`if` / `else` inside handler bodies is supported.

For non-trivial handlers, prefer a named `rsjs::event_handler(...)` local before
the `html_sync!` block. The handler body is normal Rust source, so IDE
autocomplete works while typing `self.` and event methods. The first parameter
is the DOM event; the optional second parameter is a template payload.

```rust
let on_pick = rsjs::event_handler(|event: rsjs::Event, day: u8| {
    event.prevent_default();
    console::log(day);
    self.on_change.call(day);
});

html_sync! {
    <button on_click={on_pick(day)}>{day}</button>
}
```

Supported event attributes are:

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
on_animationstart
on_animationiteration
on_animationend
on_animationcancel
on_transitionrun
on_transitionstart
on_transitionend
on_transitioncancel
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

`on_focus`, `on_blur`, and `on_scroll` are delegated through capture because
native `focus`, `blur`, and element `scroll` do not bubble.
`on_document_*` handlers attach to the owner document for the source element,
expose `CurrentTarget<dom::Document>`, and are intended for outside-click and
Escape-key overlay behavior.
`on_window_*` handlers attach to the source element's owner window, and are
typed as `CurrentTarget<dom::Window>`. They are intended for route, hash,
storage-sync, viewport, and page-scroll state. The source element remains an
internal payload owner for repeated handlers, not the public current target.

Supported event statement methods:

```rust
event.prevent_default();
event.stop_propagation();
event.target_first_file_text_into(&selected_text);
event.target_first_file_text_into_or(&selected_text, "unreadable");
event.data_transfer_first_file_text_into(&dropped_text);
event.data_transfer_first_file_text_into_or(&dropped_text, "unreadable");
```

Supported browser side-effect helpers inside handler bodies:

```rust
console::log(value);
console::info(value);
console::debug(value);
console::warn(value);
console::error(value);
console::table(value);
console::time("label");
console::time_end("label");
console::group("label");
console::group_end();
rsjs::window::scroll_to(0, 100);
rsjs::window::scroll_by(0, 20);
rsjs::window::print();
rsjs::location::set_hash(filter_hash.get());
rsjs::location::assign("/catalog/filters");
rsjs::location::replace("/login");
rsjs::location::reload();
rsjs::history::push_state("/catalog#filters");
rsjs::history::replace_state("/catalog#filters=open");
rsjs::history::back();
rsjs::history::forward();
rsjs::history::go(1);
rsjs::document::dispatch_event("filters:changed");
rsjs::document::dispatch_event_with_detail("filters:changed", filter_hash.get());
rsjs::document::set_title("Filters open");
rsjs::clipboard::write_text(filter_hash.get());
rsjs::clipboard::read_text_into(&clipboard_text);
rsjs::clipboard::read_text_into_or(&clipboard_text, "permission denied");
rsjs::webauthn::create_json_into(registration_options_json.get(), &credential_json);
rsjs::webauthn::create_json_into_or(registration_options_json.get(), &credential_json, "failed");
rsjs::webauthn::get_json_into(authentication_options_json.get(), &credential_json);
rsjs::webauthn::get_json_into_or(authentication_options_json.get(), &credential_json, "failed");
rsjs::root::set_class("dark", true);
rsjs::local_storage::set_item("filters", filter_hash.get());
rsjs::local_storage::remove_item("filters:stale");
rsjs::session_storage::set_item("filters", filter_hash.get());
rsjs::session_storage::remove_item("filters:stale");
rsjs::url::set_search_param("filters", filter_hash.get());
rsjs::url::delete_search_param("stale");
rsjs::url::push_search_param("page", "2");
rsjs::url::push_delete_search_param("modal");
```

`rsjs::root::set_class(class, on)` mirrors
`document.documentElement.classList.toggle(class, on)`: it is the effect
paired with `rsjs::root::has_class`, for theme classes and anything else keyed
off the root element. Prefer keying CSS off that one class over setting inline
styles from a handler.

These lower to direct browser API calls in the generated island JS. They are
statement-only helpers; they do not return values and they are not reactive
subscriptions. Storage writes/removals are wrapped in `try`/`catch` because
browsers can throw when storage is unavailable. URL query-string writes/removals
update the current `URLSearchParams`; `set_` / `delete_` variants call
`history.replaceState(...)`, while `push_` variants call
`history.pushState(...)`.
Clipboard reads are async commands: they write into the target signal when
`navigator.clipboard.readText()` resolves, or write `""` / the explicit
fallback when the API is unavailable or permission is denied.
WebAuthn helpers are also async commands: they expect server-provided JSON
strings for `PublicKeyCredential` options, call the browser credential API,
and write the resulting credential JSON string or fallback into the target
signal.
File text reads are also async commands: they read the first selected/dropped
file with `File.text()` and write the result or fallback into the target signal.

Supported event value accessors include:

```rust
event.type_()
event.value()
event.checked()
event.target_value()
event.target_checked()
event.target_date()
event.target_id()
event.current_target_id()
event.current_value()
event.current_target_value()
event.current_checked()
event.current_target_checked()
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
event.animation_name()
event.property_name()
event.elapsed_time()
event.pseudo_element()
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

On local bindings, `event.current_target_id()`, `event.current_value()`, and `event.current_checked()` read from the matched rsjs handler element. The generated typed handle supports direct reads and a handler-local form such as `let input = event.current_target(); input.value()`. Common reads include `id`, `text_content`, static `get_attribute` / `has_attribute`, scroll/size/geometry accessors, and element-specific value, checked, validation, selection, and file-summary accessors. Rustc restricts those methods by the event's marker. The handle may only receive generated scalar reads inside that handler; it cannot cross into a signal, callback, helper, or serialized payload, and it does not expose DOM effects. For delegated handlers, this is intentionally different from native DOM `event.currentTarget`, which points at the delegated listener root. Global bindings instead return `CurrentTarget<dom::Document>` or `CurrentTarget<dom::Window>` and do not expose element methods.

`target_attr`, `closest_attr`, and `current_target_attr` require a static string literal attribute name. `closest_attr` walks from `event.target` to the nearest ancestor with that attribute, which is the general form of `event.target_date()`.

`target_inside(ref)` and `target_outside(ref)` require a `node_ref` declared in
the render prelude. They lower to an `Element.contains(event.target)` check and
are the safe path for outside-click handling.

### Node Refs

Declare a node ref before the template and bind it to a stable element with `ref={...}`:

```rust
let input = rsjs::node_ref::<rsjs::HtmlInputElement>();

html_sync! {
    <input ref={input}/>
    <button on_click={|_| {
        input.focus();
        input.set_value("");
    }}>
        "Clear"
    </button>
}
```

Supported value accessors are:

```rust
input.id()
input.text_content()
input.value()
input.checked()
input.selection_start()
input.selection_end()
input.get_attribute("data-id")
input.has_attribute("hidden")
input.check_validity()
input.validation_message()
input.validity_valid()
input.will_validate()
input.scroll_top()
input.scroll_left()
input.client_width()
input.client_height()
input.scroll_width()
input.scroll_height()
input.offset_width()
input.offset_height()
input.rect_x()
input.rect_y()
input.rect_top()
input.rect_right()
input.rect_bottom()
input.rect_left()
input.rect_width()
input.rect_height()
input.file_count()
input.first_file_name()
input.first_file_type()
input.first_file_size()
```

Supported node-ref validation statements inside handler bodies:

```rust
input.report_validity();
input.set_custom_validity("Pick a date in the future");
```

Supported statement methods are:

```rust
input.focus();
input.blur();
input.scroll_into_view();
input.click();
form.request_submit();
form.reset();
dialog.show_modal();
dialog.close();
popover.show_popover();
popover.hide_popover();
popover.toggle_popover();
input.scroll_to(0, 32);
input.scroll_by(0, 8);
input.set_pointer_capture(event.pointer_id());
input.release_pointer_capture(event.pointer_id());
input.select();
input.select_all();
input.set_value(value);
input.set_checked(value);
input.set_selection_range(start, end);
input.set_text_content(value);
input.add_class(class_name);
input.remove_class(class_name);
input.toggle_class(class_name);
input.set_attribute("aria-expanded", value);
input.remove_attribute("hidden");
input.set_style_property("background", value);
```

`ref={...}` inside template `for` loops is rejected for now because one ref handle would otherwise point at many repeated DOM nodes.

A parent may pass one required typed ref through a statically composed direct
child. The child participates in the parent's owner and binds the exact same
browser object; it does not need `client` permission solely to consume the
handle:

```rust
pub struct FocusInput {
    pub input_ref: rsjs::NodeRef<rsjs::HtmlInputElement>,
}

#[rsjs::rsjs(component)]
impl<L: proa_core::DataLoader>
    proa_core::WebRenderSync<L> for FocusInput
{
    fn render(
        self,
        cx: &mut proa_core::WebContext<L>,
    ) -> Result<(), proa_core::WriteError> {
        proa_macros::html_sync! {
            <input ref={self.input_ref}/>
        }
        .render(cx)
    }
}

pub struct FocusPanel;

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader>
    proa_core::WebRenderSync<L> for FocusPanel
{
    fn render(
        self,
        cx: &mut proa_core::WebContext<L>,
    ) -> Result<(), proa_core::WriteError> {
        let input_ref = rsjs::node_ref::<rsjs::HtmlInputElement>();

        proa_macros::html_sync! {
            <section>
                {FocusInput { input_ref }}
                <button on_click={|_| input_ref.focus()}>"Focus"</button>
            </section>
        }
        .render(cx)
    }
}
```

This first cross-component handle shape requires a non-optional field, a
`StaticSingle` direct composed edge, and exactly one compatible terminal
binding. It is rejected across `rsjs::island(...)` and in conditional,
repeated, keyed, optional, multiply-bound, or standalone-child forms.

### Marked client functions

```rust
// Pure value→value. Callable in any reactive position or handler body.
#[rsjs::client]
pub fn subtotal(price: i64, qty: i64) -> i64 {
    price * qty
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Cart {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> Result<(), WriteError> {
        let qty = signal(1_i64);
        html_sync! {
            <span>{subtotal(self.price, qty.get())}</span>
        }
        .render(cx)
    }
}
```

The function annotation is the eligibility declaration. Reachable calls create
symbolic helper edges automatically; normal Rust imports, aliases, and
qualified paths are valid. `#[rsjs::client_module]` is optional grouping for
bundling or namespace validation and is never required on the consuming
component. Prefer server-side prelude helpers for static labels, and keep
browser helper groups small and named. Current `rsjs explain` reports which
reachable helper groups shipped.

Standalone helpers are fixed-signature, synchronous, safe Rust-ABI functions.
Use plain parameter identifiers and the supported JSON-shaped types. `&str` is
parameter-only; return `String`. Use `f64` for floating-point helpers: `f32` is
rejected until Client IR represents Rust's per-operation rounding. Explicit
helper-call turbofish/path arguments are also rejected because the transport
does not carry monomorphization identity.

Linked builds resolve standalone helper definitions and calls through normal
Rust identity. Component-level `clients(...)` allowlists have been removed; the
helper definition's one `#[rsjs::client]` annotation is the eligibility
declaration.

### Server-rendered values and compatibility escape hatch

```rust
{ rsjs::server_only(some_expression) }
```

In the v2 strategy model, ordinary server values do not need this escape
hatch. A prelude local whose dependency closure contains no inbound or local
client value renders once on the server and does not enter Client IR:

```rust
let label = format_label(self.item);

html_sync! {
    <button aria-label={label}>
        {label}
        <span>{count.get()}</span>
    </button>
}
```

Here `label` is SSR-only only if `format_label(self.item)` depends on static
server data at this occurrence. If `self.item` is supplied by a live composed
parent, an opaque server helper cannot silently freeze it: linking rejects that
path unless the helper has supported client semantics. `count.get()` is
reactive client state. `ClientEncode` is required only for values that flow
through a reachable client crossing.

Direct inbound reads have stronger semantics:

```rust
html_sync! {
    <h1>{self.title}</h1>
    <div class={self.class}>{self.body}</div>
}
```

Inside an analyzed component these are latent `LiveRead` uses. They emit only
server HTML when the final occurrence is server-only, but stay current when a
linked parent supplies a live value. An exact alias such as
`let title = self.title;` preserves the same provenance.

Construct arbitrary non-component renderers and opaque server computations in
the prelude, then render the local normally. A direct struct literal inside an
analyzed slot is reserved for a compiler-composable component edge. An unbound
direct path such as `{Badge}` or `{crate::ui::Badge}` has the same meaning for
a unit component; imported, renamed, and re-exported paths work through
rustc-resolved identity. A single-segment path naming a tracked prelude or loop
local remains an ordinary value. Keep
`rsjs::server_attr(...)` for static-per-render dynamic attributes.
`rsjs::server_only(...)` remains a compatibility escape hatch for syntax the
tolerant frontend cannot yet classify; it is not the routine authoring path.

Compiler-backed v2 lint may report `rsjs/unnecessary-server-only` when the
wrapper surrounds a proven ordinary server local. It must not suggest removing
an intentional boundary around an inbound field such as `self.class`, because
the wrapper changes that field from latent `LiveRead` participation to a
per-render snapshot. The lint should not fire for opaque components,
`UnsafeRaw`, helper calls, or opaque child descriptor server-prop expressions.

### Descriptor-backed rsjs components

```rust
{SearchInput {
    value: query.get(),
    on_change,
}}
```

Allowed only when `SearchInput` has an rsjs component descriptor. Handler fields
must receive local `rsjs::handler(...)` values or `rsjs::noop_handler()`.
`signal(self.value)` records `SeedOnce`: the child samples the current parent
value when its occurrence mounts and then owns the local cell. Direct field
reads such as `{self.label}` and `class={self.class}` record `LiveRead`: they
remain current when a composed parent supplies a live value. The parent value
must fit the supported reactive expression grammar, such as `query.get()` or a
literal.

Use `rsjs::server_only(...)` or `rsjs::server_attr(...)` when a prop is
intentionally static for that render. Parent signals and handler references
cannot cross such a snapshot boundary.

Opaque server-only dynamic attributes use `rsjs::server_attr(...)`:

```rust
<input class={rsjs::server_attr(crate::components::input_classes(self.size))} />
```

The wrapped expression renders in the generated server `html_sync!` body and
does not produce a client binding. During semantic linking, `self.field` and
`self.field.path` projections become occurrence-specific prop edges while
preserving the trailing path. Child-local server computations in attributes
remain part of the compiled fragment rather than being macro-copied into the
parent.
The wrapped expression must not capture or read a `signal(...)` local; use a
normal dynamic attribute when the value should update after hydration.
It also must not declare/read `signal(...)`, `query_with_initial(...)`, or `mutation(...)`
inside the wrapped expression.

### Explicit island boundaries

```rust
{rsjs::island(SearchInput {
    value: String::new(),
    on_change: rsjs::noop_handler(),
})}
```

Inside a parent RSJS-analyzed component, descriptor-backed child occurrences
participate in the enclosing linked graph by default. Live props, callbacks,
or lifecycle edges fuse connected occurrences into one final owner; the linker
imports the child's compiled fragment rather than copying its graph.
`rsjs::island(...)` forces an independent nested owner boundary.

Constraints:
- Cannot capture a parent `rsjs::handler(...)` local; handler props do not cross
  island boundaries.
- Cannot capture parent signals, declare/read client primitives, or call rsjs
  browser helpers inside the boundary expression.
- If the child should update parent state, render it directly so it composes
  into the parent island. If it should be independent, pass independent state or
  `rsjs::noop_handler()`.

For opaque server-rendered content that should not be analysed (an
`UnsafeRaw` SVG sprite, a regular Proa component, etc.). The expression is
spliced verbatim into the generated `html_sync!` body and never appears in
the per-island JS.

Constraints:
- Cannot capture or read a `signal(...)` local (compile error).
- Cannot declare/read client primitives such as `signal(...)`,
  `memo(...)`, `query_with_initial(...)`, `mutation(...)`,
  `node_ref(...)`, `handler(...)`, `event_handler(...)`, or
  `interval(...)`.
- Cannot call rsjs browser helpers such as `rsjs::window::inner_width()`,
  `rsjs::location::pathname()`, or `rsjs::navigator::language()`.

## Client expressions that are not lowerable

These produce a compile error when they depend on browser-reachable state:

```rust
{ format!("count is {}", count.get()) }     // ✗ opaque call captures a signal
{ self.helper(count.get()) }                // ✗ opaque method captures a signal
{ if count.get() > 0 { let x = 1; x } else { 0 } } // ✗ reactive branch-local statements
{ |evt| { count.set(evt); count.get() } }    // ✗ reactive closure as a rendered child
{ -count.get() }                            // ✗ unary minus
```

Ordinary SSR values and calls are allowed directly:

```rust
{label}                               // ordinary server local, SSR once
{self.title}                          // inbound LiveRead in an analyzed component
{render_markdown(self.document)}      // opaque server call, SSR once
{proa_core::UnsafeRaw(self.icon_svg)} // opaque server value, SSR once
```

The opaque calls above are valid only while no browser-reachable dependency
crosses them. If `self.document` or `self.icon_svg` comes from a live parent,
RSJS rejects the path instead of shipping a stale snapshot; move the operation
to supported client code or make the snapshot boundary explicit.

Use these shapes when a boundary needs more structure:

| Want to do | How |
|---|---|
| Embed unknown-cardinality server output before a hydrated sibling | Render it directly. The runtime verifies the fast positional path and falls back to an owner-scoped marker lookup: `{render_markdown(doc)}`. |
| Embed an ordinary Proa component without creating a direct analyzed-component edge | Construct it in the prelude (`let rendered = MyComponent { ... };`) and render `{rendered}` |
| Run a non-reactive helper at render time | Call it directly: `{helper(server_value)}` |
| Chain on a signal-derived value | Define a `#[rsjs::client]` pure helper: `#[rsjs::client] fn double(n: i64) -> i64 { n * 2 }` then `{double(count.get())}` |
| DOM-shaped `if/else` branches | Use a conditional section: `{if pred { html_sync! { ... } } else { html_sync! { ... } }}` |

In v2, unsupported template expressions should report
`rsjs/template-opaque-expression` or a more specific
`rsjs/template-*` diagnostic. These errors mean the expression must choose an
explicit SSR or client route; they do not cause rsjs to infer client-only
rendering. During rollout, current source lint may still report the legacy
`slot/invalid-expression` code; treat it as a compatibility alias for the more
specific v2 `rsjs/template-*` codes.

When strategy sidecars are enabled, `cargo proa rsjs explain` should expose the
same template decisions the compiler used: simple SSR slots/attrs, client
expressions, descriptor invocations, explicit island boundaries, and
`rsjs/template-*` diagnostics. Use that explanation to debug why a value stayed
server-rendered or became browser-reachable; do not infer it from whether a JS
asset happened to be emitted.

## Predicates

Conditional `{if pred { ... }}` predicates must lower to the v0.1 IR. Allowed:

```rust
if count.get()                       // bool signal read
if !username.is_non_empty()          // unary !
if count.get() == 0                  // BinOp::Eq vs literal
if count.get() != target             // BinOp::Neq
if count.get() < 10                  // BinOp::Lt
if a.get() && b.get()                // BinOp::And (short-circuit)
if a.get() || b.get()                // BinOp::Or
```

Not allowed:

```rust
if helper(count.get())               // ✗ free function call
if let Some(v) = ...                 // ✗ pattern matching
if matches!(...)                     // ✗ macros in predicate
```

## Attribute slots

Attribute values can be:

```rust
<div class="static">                                 // string literal
<div class={dynamic_class}>                          // identifier (prelude let)
<div class={if active.get() { "on" } else { "off" }}> // value-level conditional
<input bind_value={username}>                        // signal-bound input value
<button on_click={|_| ...}>                          // event handler
```

Static attribute values that are entirely `Ident`-shaped (no quotes) are
forwarded as-is to the HTML emitter.

## See also

- [`rsjs_macros/src/analyze/mod.rs`](../../../rsjs_macros/src/analyze/mod.rs) — the analyser that enforces this grammar.
- [`rsjs_ir/src/client/expr.rs`](../../../rsjs_ir/src/client/expr.rs) — the IR `Expr` enum (everything that *can* be lowered).
- [`rsjs_macros/tests/parser_*.rs`](../../../rsjs_macros/tests/) — working fixtures for each shape.
- [`rsjs_macros/tests/ui/fail/`](../../../rsjs_macros/tests/ui/fail/) — compile-fail fixtures for each rejection rule.
