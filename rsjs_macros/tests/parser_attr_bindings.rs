// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration coverage for reactive HTML attributes inside `#[rsjs(component, client)]`.

use proa_core::render_to_string;
#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

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

pub struct ReactiveAttrs;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ReactiveAttrs {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let open = signal(false);
        let panel_class = signal("panel panel--closed");
        proa_macros::html_sync! {
            <div>
                <button
                    type="button"
                    aria-expanded={open.get()}
                    class={panel_class.get()}
                    on_click={|_| open.update(|v| !v)}
                >
                    "Toggle"
                </button>
                <section hidden={!open.get()} data-state={panel_class.get()}>"Panel"</section>
                <button type="button" disabled={open.get()}>"Submit"</button>
            </div>
        }
        .render(cx)
    }
}

pub struct OwnedAttr {
    pub label: String,
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for OwnedAttr {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let label = signal(self.label);
        proa_macros::html_sync! {
            <div data_state={label.get()}>{label.get()}</div>
        }
        .render(cx)
    }
}

#[test]
fn reactive_attrs_render_initial_values_and_markers() {
    let html = render_to_string(ReactiveAttrs);

    assert!(
        html.contains(r#"aria-expanded="false""#),
        "normal bool-valued attrs should SSR as string values, got:\n{html}"
    );
    assert!(
        html.contains(r#"class="panel panel--closed""#),
        "string-valued attrs should SSR their initial signal value, got:\n{html}"
    );
    assert!(
        html.contains(r#"data-state="panel panel--closed""#),
        "data attrs should SSR their initial signal value, got:\n{html}"
    );
    assert!(
        html.contains("hidden"),
        "HTML boolean attrs should SSR by presence/absence, got:\n{html}"
    );
    assert!(
        !html.contains("disabled"),
        "disabled={{false}} should SSR as absent, got:\n{html}"
    );
    assert!(
        html.matches(r#"data-rsjs-attr=""#).count() >= 3,
        "nodes with reactive attrs should carry attr markers, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("reactiveattrs_"))
        .expect("ReactiveAttrs island should be registered")
        .js_source;
    assert!(
        js.contains(r#""aria-expanded""#),
        "client attr binding should use normalized HTML names, got:\n{js}"
    );
    assert!(
        js.contains(r#""data-state""#),
        "client attr binding should use normalized data-* names, got:\n{js}"
    );
    assert!(
        !js.contains("aria_expanded") && !js.contains("data_state"),
        "client attr binding should not retain Rust identifier spelling, got:\n{js}"
    );
}

#[test]
fn owned_signal_initializer_can_seed_attr_and_text() {
    let html = render_to_string(OwnedAttr {
        label: String::from("owned <label>"),
    });

    assert!(
        html.contains(r#"data-state="owned &lt;label&gt;""#),
        "owned String initializers should SSR attr values without moving before seed/text, got:\n{html}"
    );
    assert!(
        html.contains(r#"owned &lt;label&gt;"#),
        "owned String initializers should still render text bindings, got:\n{html}"
    );
    assert!(
        html.contains(r#""signals":["owned \u003clabel\u003e"]"#),
        "owned String initializers should still seed hydration JSON, got:\n{html}"
    );
}
