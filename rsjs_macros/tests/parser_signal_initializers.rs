// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Runtime signal initializer coverage for `signal(self.field)`.

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

pub struct FilterChip {
    pub initial_checked: bool,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for FilterChip {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let checked = signal(self.initial_checked);
        proa_macros::html_sync! {
            <div>
                <input type="checkbox" bind_checked={checked} />
                {if checked.get() {
                    proa_macros::html_sync! {
                        <strong>"On"</strong>
                    }
                } else {
                    proa_macros::html_sync! {
                        <em>"Off"</em>
                    }
                }}
                <button on_click={|_| checked.update(|v| !v)}>"Toggle"</button>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn self_field_signal_initializer_seeds_ssr_html_and_json() {
    let html = render_to_string(FilterChip {
        initial_checked: true,
    });

    assert!(
        html.contains(r#"data-rsjs-input-checked="1" checked"#),
        "checked=true should render the checkbox as checked, got:\n{html}"
    );
    assert!(
        html.contains(r#"<strong data-rsjs-cond-then="0:0" style="">On</strong>"#),
        "checked=true should show the then branch on SSR, got:\n{html}"
    );
    assert!(
        html.contains(r#"<em data-rsjs-cond-else="0:0" style="display:none">Off</em>"#),
        "checked=true should hide the else branch on SSR, got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":[true]}"#),
        "checked=true should seed the hydration JSON from self.initial_checked, got:\n{html}"
    );

    let html = render_to_string(FilterChip {
        initial_checked: false,
    });

    assert!(
        !html.contains(r#"data-rsjs-input-checked="1" checked"#),
        "checked=false should not render the checked attribute, got:\n{html}"
    );
    assert!(
        html.contains(r#"<strong data-rsjs-cond-then="0:0" style="display:none">On</strong>"#),
        "checked=false should hide the then branch on SSR, got:\n{html}"
    );
    assert!(
        html.contains(r#"<em data-rsjs-cond-else="0:0" style="">Off</em>"#),
        "checked=false should show the else branch on SSR, got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":[false]}"#),
        "checked=false should seed the hydration JSON from self.initial_checked, got:\n{html}"
    );
}

pub struct InitialLabel<'a> {
    pub label: &'a str,
}

#[rsjs(component, client)]
impl<'a, L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for InitialLabel<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let label = signal(self.label);
        proa_macros::html_sync! {
            <div>
                <span>{label.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn borrowed_string_field_initializer_is_client_encoded() {
    let html = render_to_string(InitialLabel { label: "</script>" });

    assert!(
        html.contains(r#"<span data-rsjs-bind="1">&lt;/script&gt;</span>"#),
        "borrowed string field should render as escaped text, got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":["\u003c/script\u003e"]}"#),
        "borrowed string field should be safely encoded in hydration JSON, got:\n{html}"
    );
}

fn build_label(prefix: &str) -> String {
    format!("{prefix}-ready")
}

pub struct RuntimeExprLabel {
    pub prefix: &'static str,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RuntimeExprLabel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let label = signal(build_label(self.prefix));
        proa_macros::html_sync! {
            <div>
                <input type="text" bind_value={label} />
                <span>{label.get()}</span>
                <span>{label.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn runtime_expression_initializer_is_evaluated_during_ssr() {
    let html = render_to_string(RuntimeExprLabel { prefix: "url" });

    assert!(
        html.contains(r#"data-rsjs-input-value="1" value="url-ready""#),
        "runtime expression should render its SSR input value, got:\n{html}"
    );
    assert_eq!(
        html.matches(r#">url-ready</span>"#).count(),
        2,
        "runtime expression should be reusable across multiple SSR reads, got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":["url-ready"]}"#),
        "runtime expression should seed hydration JSON, got:\n{html}"
    );
}
