# Cookbook

Short copy-paste recipes for common patterns. Each recipe is self-contained
and references a real fixture in the workspace.

If you don't see your pattern here, check
[`rsjs-islands.md`](rsjs-islands.md) (the four canonical island shapes) or
[`components.md`](components.md) (the end-to-end component walkthrough).

## Index

| Recipe | When to use |
|---|---|
| [Toggle visibility on click (1 signal)](#1-toggle-visibility-on-click-1-signal) | Watch heart, drawer, expandable section |
| [Multi-toggle toolbar (N signals, 1 island)](#2-multi-toggle-toolbar-n-signals-1-island) | Filter chips, tab bar, multi-select |
| [Form with reactive validation](#3-form-with-reactive-validation) | Login, signup, contact form |
| [Conditional class on root via signal](#4-conditional-class-on-root-via-signal) | "Selected" / "expanded" / "active" state |
| [Embed an SVG sprite inside an island](#5-embed-an-svg-sprite-inside-an-island) | Icons that aren't lone Rust idents |
| [Render N items from a slice](#6-render-n-items-from-a-slice) | Lists, grids, repeated cards |
| [Generic-children component](#7-generic-children-component) | Card-shaped wrapper, layout primitive |
| [Async island with dataloader](#8-async-island-with-dataloader) | Fetch user data inside an interactive widget |
| [Compose an opaque server component inside an analyzed component](#9-compose-an-opaque-server-component-inside-an-island) | Use an ordinary render component inside an RSJS-analyzed body |
| [Ship the rsjs runtime + per-island JS in the page](#10-ship-the-rsjs-runtime--per-island-js-in-the-page) | Wiring up a new framework adapter |
| [Generic island with lifetime / type / const params](#11-generic-island-with-lifetime--type--const-params) | Island borrows lifetime-bound data or takes type/const params |

---

## 1. Toggle visibility on click (1 signal)

Boolean signal, two conditional branches, one click handler.

```rust
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::signal;
use rsjs_macros::rsjs;

pub struct Drawer;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Drawer {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let open = signal(false);
        html_sync! {
            <div>
                <button on_click={|_| open.set(!open.get())}>"Toggle"</button>
                <div>
                    {if open.get() {
                        html_sync! { <div class="drawer-content">"Hello"</div> }
                    }}
                </div>
                <div>
                    {if !open.get() {
                        html_sync! { <span class="drawer-collapsed">"(closed)"</span> }
                    }}
                </div>
            </div>
        }
        .render(cx)
    }
}
```

Reference: [`proa_demo/src/ebay_srp_islands.rs`](../../proa_demo/src/ebay_srp_islands.rs)
(`WatchHeart`).

---

## 2. Multi-toggle toolbar (N signals, 1 island)

Three independent signals in one island, each with its own click handler. The
island gets one `<rsjs-island>` wrapper, one seed script `{"signals":[false,false,false]}`,
three handler markers.

```rust
pub struct FilterChips;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for FilterChips {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let buy_now = signal(false);
        let auction = signal(false);
        let free_shipping = signal(false);
        html_sync! {
            <div class="filter-toolbar">
                <button on_click={|_| buy_now.update(|v| !v)}>
                    <span>{if buy_now.get()  { html_sync! { <span class="active">"Buy Now"</span> } }}</span>
                    <span>{if !buy_now.get() { html_sync! { <span>"Buy Now"</span> } }}</span>
                </button>
                <button on_click={|_| auction.update(|v| !v)}>
                    <span>{if auction.get()  { html_sync! { <span class="active">"Auction"</span> } }}</span>
                    <span>{if !auction.get() { html_sync! { <span>"Auction"</span> } }}</span>
                </button>
                <button on_click={|_| free_shipping.update(|v| !v)}>
                    <span>{if free_shipping.get()  { html_sync! { <span class="active">"Free shipping"</span> } }}</span>
                    <span>{if !free_shipping.get() { html_sync! { <span>"Free shipping"</span> } }}</span>
                </button>
            </div>
        }
        .render(cx)
    }
}
```

Reference: [`proa_demo/src/ebay_srp_islands.rs`](../../proa_demo/src/ebay_srp_islands.rs)
(`FilterChips`).

---

## 3. Form with reactive validation

Two `bind_value` inputs, an `on_submit` handler, two conditional error
sections (each in its own wrapper to satisfy "one if per element").

```rust
pub struct LoginForm;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for LoginForm {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let username = signal(String::new());
        let email = signal(String::new());
        let submitting = signal(false);
        html_sync! {
            <form on_submit={|_| submitting.set(true)}>
                <input bind_value={username}/>
                <div>
                    {if !username.is_non_empty() {
                        html_sync! { <p class="error">"Username required"</p> }
                    }}
                </div>
                <input bind_value={email}/>
                <div>
                    {if email.get() == String::new() {
                        html_sync! { <p class="error">"Email required"</p> }
                    }}
                </div>
                <button type="submit">"Submit"</button>
            </form>
        }
        .render(cx)
    }
}
```

Reference: [`rsjs_macros/tests/parser_user_form.rs`](../../rsjs_macros/tests/parser_user_form.rs).

---

## 4. Conditional class on root via signal

When you want the same DOM element with a different class based on state.
Uses two sibling `if`s wrapping the entire component.

```rust
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Toggle {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let active = signal(false);
        html_sync! {
            <div>
                {if active.get() {
                    html_sync! {
                        <button class="btn btn--active" on_click={|_| active.set(false)}>"On"</button>
                    }
                }}
                {if !active.get() {
                    html_sync! {
                        <button class="btn" on_click={|_| active.set(true)}>"Off"</button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}
```

The two branches share the `on_click` semantics but render different markup.
The runtime swaps display via `style="display:none"` toggling, no DOM
mutation.

---

## 5. Embed an SVG sprite inside an island

Modern (post-`fix(rsjs_macros)` and `feat(proa_dsl)`): keyword tag names like
`<use>` parse natively.

```rust
html_sync! {
    <svg class="icon" focusable="false" aria-hidden="true">
        <use href="#icon-heart-filled-16"/>
    </svg>
}
```

Fallback for SVG syntax the template parser does not support: construct an
explicit raw renderable in the prelude:

```rust
let icon =
    proa_core::UnsafeRaw(r##"<use href="#icon-heart-16"></use>"##);

html_sync! {
    <svg class="icon">
        {icon}
    </svg>
}
```

The first form is preferred. The fallback is only useful if you're vendoring
arbitrary SVG markup that has constructs the html_sync parser doesn't handle
yet.

---

## 6. Render N items from a slice

Plain, non-analyzed component pattern. Keep the loop inside the final template
and author the render contract explicitly:

```rust
pub struct ListingsList<'a> {
    pub listings: &'a [Listing],
}

impl<L: DataLoader> WebRenderSync<L> for ListingsList<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <ul>
                {for listing in self.listings {
                    html_sync! { <li>{Card { listing }}</li> }
                }}
            </ul>
        }
        .render(cx)
    }
}
```

For islands, consult the current rsjs repetition support and its conformance
fixture. A server-only parent can always use the component pattern above and
render one island instance per item.

Reference: [`proa_demo/src/ebay_srp.rs`](../../proa_demo/src/ebay_srp.rs)
(`InteractiveListingsList` renders 60 `WatchHeart` islands).

---

## 7. Generic-children component

```rust
pub struct Card<C = ()> {
    pub class: Option<&'static str>,
    pub children: Option<C>,
}

impl<L, C> WebRenderSync<L> for Card<C>
where
    L: DataLoader,
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let classes = card_classes(self.class);
        html_sync! {
            <div data-component="card" class={classes}>
                {self.children}
            </div>
        }
        .render(cx)
    }
}

// Caller side:
Card {
    children: Some(html_sync! { <h2>"Hello"</h2> }),
    ..Default::default()
}.render(cx)?;
```

Reference: [`../../proa_core/tests/landing_page_example.rs`](../../proa_core/tests/landing_page_example.rs).

---

## 8. Async island with dataloader

Use `WebRender` (not `WebRenderSync`) when the body needs `.await`.

```rust
pub struct UserCard {
    pub user_id: u64,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for UserCard {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let user = cx
                .load::<User>(LoadKey::typed("user", self.user_id))
                .await?;
            let following = signal(user.following);
            html! {
                <div>
                    <span>{user.name}</span>
                    <button on_click={|_| following.set(!following.get())}>
                        {if following.get()  { html! { <span>"Unfollow"</span> } }}
                    </button>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
```

Reference: [`rsjs_macros/tests/parser_counter.rs`](../../rsjs_macros/tests/parser_counter.rs)
(`AsyncServerPreludeChild` and the query/mutation fixtures).

---

## 9. Compose an opaque server component inside an island

Plain SSR locals and field/path projections render directly inside an
RSJS-analyzed body. Construct an arbitrary non-component renderer in the
prelude, because a direct struct literal in an analyzed slot deliberately means
"compiler-composable component edge."

```rust
use crate::components::ActionButton;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for ConfirmModal {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let action = ActionButton {
            class: Some("btn-primary"),
            label: "Confirm",
        };
        let open = signal(false);
        html_sync! {
            <div>
                {if open.get() {
                    html_sync! {
                        <div class="modal">
                            {action}
                        </div>
                    }
                }}
                <button on_click={|_| open.set(true)}>"Open"</button>
            </div>
        }
        .render(cx)
    }
}
```

The `ActionButton` local renders once on the server and is opaque to the parent
client graph. A direct `{ActionButton { ... }}` expression instead requires the
generated component ABI. For independently interactive behavior, use an
explicit island boundary.

---

## 10. Ship rsjs assets for rendered hydrated islands

`proa_framework_axum` response finalizers do this automatically. Render the page
through the ordinary sync, async, or streaming response path; do not
read the recorded island set or emit runtime tags from a page or component.
The finalizer snapshots the response's `RsjsRenderArtifacts`, derives one
`rsjs::PageRuntimePlan`, and emits only the runtime, query runtime, preloads,
eager modules, and deferred instructions the rendered islands require.

```rust
let response = proa_framework_core::RouteResponse::ssr(MyPage);
```

If you are implementing a framework adapter rather than using
`proa_framework_axum`, the response owner—not a component—uses the same shared
planner:

```rust
let artifacts = cx.rsjs_artifacts_snapshot();
let plan = rsjs::manifest()
    .page_runtime_plan(artifacts.rendered_islands())?;
plan.write_html(response_tail, cx.csp_nonce())?;
```

Call this after the render has completed and place the resulting tail before
`</body>`. An empty artifact snapshot produces no tags, while an unknown
recorded island is an error. `PageRuntimePlan` keeps interaction islands eager,
defers visible/idle/media islands, scopes the query runtime correctly, and
deduplicates client modules, component fragments, and module preloads.
If `artifacts.query_seed_table_json()` returns a value, the adapter must place
it in an inert `application/json` script with id `rsjs-query-seed-table`
before the runtime plan. Apply the request nonce to that script too. The
framework finalizers already do this; it matters only for custom adapters.

Production adapters with hashed or CDN URLs should implement
`rsjs::RsjsAssetUrlResolver` and call
`Manifest::page_runtime_plan_with_urls(...)`; do not copy the planning rules.
The resolver changes plan/bootstrap URLs only. The asset pipeline must also
rewrite static ESM imports embedded in the generated island, client-module,
client-class, and component-fragment sources to those resolved URLs (or serve stable
canonical aliases); otherwise an entry module can load while its runtime or
relative dependencies still 404.
For streamed responses, retain one `PageRuntimeEmissionState` and call
`write_html_delta(...)` for the shell and each committed boundary. Custom
out-of-order boundary producers return `RenderedBoundary`, which keeps the
committed HTML and its artifact delta together. When using
`proa_stream::drain_pending_boundaries_with_nonce_and_artifacts`, write the
boundary query-seed fragment and runtime-plan delta immediately before
`chunk.bytes`. The swap script inside those bytes dispatches
`proa:boundary-ready`, so metadata appended after the frame is too late.

Cached rendered fragments must preserve their artifact snapshot alongside
their HTML. Replay it with `cx.replay_rsjs_artifacts(&cached.artifacts)` when
the fragment is reused, so its islands and query seeds participate in the
outer response plan.

You also need three Axum routes:

```rust
.route("/rsjs-runtime.js", get(serve_rsjs_runtime))
.route("/rsjs-runtime-query.js", get(serve_rsjs_runtime_query))
.route("/rsjs/island/:asset_id", get(serve_rsjs_island))
```

Where `serve_rsjs_island` looks up the asset id in `rsjs::manifest().islands`
and returns its `js_source`.

---

## 11. Generic island with lifetime / type / const params

The macro preserves the user's impl header, so any mix of lifetime, type, and
const generics works. HRTBs in the where clause are fine. Buffer and loader
generics can be renamed.

```rust
pub trait LabelRef {
    fn label_ref(&self) -> &'static str;
}

pub struct GenericIsland<'a, T, const N: usize> {
    pub label: &'a T,
}

#[rsjs(component, client)]
impl<'a, T, const N: usize, Buf, Loader> WebRenderSync<Loader, Buf> for GenericIsland<'a, T, N>
where
    for<'b> &'b T: LabelRef,
    Buf: WriteBuf,
    Loader: proa_core::DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let label = self.label.label_ref();
        let count = signal(0_i64);
        html_sync! {
            <div data-kind="generic">
                <span>{count.get()}</span>
                {label}
            </div>
        }
        .render(cx)
    }
}
```

Async generics use the same canonical `WebRender` signature as every other
async component. Put application type and const generics on the impl header;
the method itself carries only the trait's render/output lifetimes. See
[`api-reference/render-traits.md`](api-reference/render-traits.md).

Reference: [`rsjs_macros/tests/parser_generics.rs`](../../rsjs_macros/tests/parser_generics.rs)
(both sync and async generic island fixtures).
