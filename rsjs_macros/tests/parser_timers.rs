// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Timer primitives end-to-end through the proc-macro pipeline.
//!
//! - A prelude `rsjs::interval(ms, || ...);` statement must emit
//!   `__rsjs_disposers.push(interval(<ms>, () => { ... }));` with the
//!   handler-statement grammar inside the tick closure, gate the
//!   `interval` runtime import behind use, and stay out of the SSR
//!   replay (nothing ticks on the server).
//! - An `on_input={rsjs::debounce(|event| ..., delay_ms)}` attribute must
//!   emit `const __handler_<id> = debounce((event) => { ... }, <delay_ms>);`
//!   plus a `__rsjs_disposers.push(__handler_<id>._dispose);` line,
//!   with the delegation table referencing the wrapped function.

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

pub struct UptimeTicker;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for UptimeTicker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let tick = signal(0_i64);
        rsjs::interval(1000, || tick.update(|t| t + 1_i64));
        proa_macros::html_sync! {
            <div>
                <span>{tick.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct DebouncedSearch;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DebouncedSearch {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal("");
        proa_macros::html_sync! {
            <div>
                <input on_input={rsjs::debounce(|event| query.set(event.value()), 300)} />
                <span>{query.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn interval_island_emits_disposer_pushed_runtime_interval() {
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("uptimeticker_"))
        .expect("UptimeTicker island should be registered")
        .js_source;
    assert!(
        js.contains("__rsjs_disposers.push(interval(1000, () => {"),
        "the interval prelude statement should lower to a disposer-pushed runtime interval, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.update((__c0) => checked_int_add(__sig_0.get(), 1n, \"i64\"));"),
        "the tick closure should run the lowered handler statements, got:\n{js}"
    );
    assert!(
        js.contains("interval") && js.contains("} from \"/rsjs-runtime.js\";"),
        "the runtime import line should pull in the interval helper, got:\n{js}"
    );
    assert!(
        !js.contains("debounce"),
        "an interval-only island must not import the debounce helper, got:\n{js}"
    );
}

#[test]
fn interval_island_ssr_renders_initial_value_without_ticking() {
    let html = render_to_string(UptimeTicker);
    assert!(
        html.contains(">0</span>"),
        "SSR should show the signal initial; the interval must not run on the server, got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":["0"]}"#),
        "the seed blob should hold the untouched signal initial, got:\n{html}"
    );
}

#[test]
fn debounced_handler_emits_wrapped_const_with_disposer() {
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("debouncedsearch_"))
        .expect("DebouncedSearch island should be registered")
        .js_source;
    assert!(
        js.contains("const __handler_0 = debounce((event) => {"),
        "the debounced attribute value should declare the runtime debounce wrapper, got:\n{js}"
    );
    assert!(
        js.contains("    }, 300);"),
        "the delay must be the second argument of the runtime wrapper, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set((event.target?.value ?? \"\"));"),
        "the wrapped handler body should run the lowered handler statements, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(__handler_0._dispose);"),
        "the debounce wrapper's _dispose must join the island disposer list, got:\n{js}"
    );
    assert!(
        js.contains("__handler_0]"),
        "the delegation table should reference the wrapped handler by name, got:\n{js}"
    );
    assert!(
        js.contains("debounce") && js.contains("} from \"/rsjs-runtime.js\";"),
        "the runtime import line should pull in the debounce helper, got:\n{js}"
    );
    assert!(
        !js.contains("interval("),
        "a debounce-only island must not start an interval, got:\n{js}"
    );
}
