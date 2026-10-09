// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Memo (derived) signals end-to-end through the proc-macro
//! pipeline: a `#[rsjs(component, client)]` island with two leaf signals and one
//! `rsjs::memo(...)` cell must
//!
//! - render SSR HTML showing the derived value evaluated over the
//!   signals' initial values,
//! - seed only the leaf signals in the dehydrated state blob (with an
//!   aligned `null` placeholder at the derived index), and
//! - emit JS that declares the derived cell via the runtime's
//!   `memo(fn, deps)` and ties its `_dispose` to island teardown.

#[allow(unused_imports)]
use proa_core::render_to_string;
#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

// Local stub mirroring parser_counter.rs: the macro discards the
// original render body, but the type-checker still resolves it.
#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

#[allow(dead_code)]
impl<T: Clone + std::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }

    pub fn set(&self, _value: T) {}

    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}

pub struct CartTotal;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CartTotal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price = signal(10_i64);
        let qty = signal(2_i64);
        let total = rsjs::memo(|| price.get() * qty.get());
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| qty.update(|q| q + 1)}>"+"</button>
                <span>{total.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct MemoChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for MemoChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price = signal(3_i64);
        let qty = signal(4_i64);
        let total = rsjs::memo(|| price.get() * qty.get());
        proa_macros::html_sync! {
            <article>
                <button on_click={|_| qty.update(|q| q + 1)}>"+"</button>
                <span>{total.get()}</span>
            </article>
        }
        .render(cx)
    }
}

pub struct ParentWithMemoChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithMemoChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>{MemoChild {}}</section>
        }
        .render(cx)
    }
}

#[test]
fn memo_survives_descriptor_stage_component_boundary() {
    let html = render_to_string(ParentWithMemoChild);
    assert!(
        html.contains(">12</span>"),
        "parent SSR should show the inlined child's derived value (3 * 4), got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":["3","4"]}"#),
        "parent seed blob should keep reachable leaf initials and trim trailing derived slots, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("parentwithmemochild_")),
        "descriptor compilation must not invent a macro-stage parent owner"
    );
    let child = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.asset_id.starts_with("memochild_"))
        .expect("MemoChild fragment should be registered for final linking");
    assert!(
        child.js_source.contains("memo(() => "),
        "the child fragment must retain its memo cell for the linker, got:\n{}",
        child.js_source
    );
}

#[test]
fn memo_island_renders_derived_initial_value() {
    let html = render_to_string(CartTotal);
    assert!(
        html.contains(">20</span>"),
        "SSR should show the memo value over signal initials (10 * 2), got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":["10","2"]}"#),
        "seed blob should hold reachable leaf initials and trim trailing derived slots, got:\n{html}"
    );
}

#[test]
fn memo_island_emits_runtime_memo_init() {
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("carttotal_"))
        .expect("CartTotal island should be registered")
        .js_source;
    assert!(
        js.contains(
            "const __sig_2 = memo(() => checked_int_mul(__sig_0.get(), __sig_1.get(), \"i64\"), [__sig_0, __sig_1]);"
        ),
        "memo declaration should lower to the runtime memo with both deps, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(__sig_2._dispose);"),
        "memo disposer should join the island disposer list, got:\n{js}"
    );
    assert!(
        js.contains("memo") && js.contains("} from \"/rsjs-runtime.js\";"),
        "runtime import line should pull in the memo helper, got:\n{js}"
    );
    assert!(
        js.contains("bind_text") && js.contains("__sig_2.get()") && js.contains("[__sig_2]"),
        "text bindings reading the derived cell should subscribe to it, got:\n{js}"
    );
    assert!(
        !js.contains("signal_initial(__rsjs_signal_initials, 2,"),
        "derived cells must not read the signal seed blob, got:\n{js}"
    );
}
