// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Launch-facing fixture for an rsjs island using the canonical render trait.

use proa_core::{render_to_string, NoLoader, WebRenderSync};
use rsjs::{event_handler, rsjs, signal};

struct LaunchCounter {
    initial: i32,
}

struct GenericLaunchCounter<'a, T, const N: usize> {
    label: &'a T,
    initial: i32,
    #[allow(dead_code)]
    marker: [(); N],
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: proa_core::WriteBuf> WebRenderSync<L, B> for LaunchCounter {
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let count = signal::<i32>(self.initial);
        let increment = event_handler(|_: rsjs::Event| count.update(|value| value + 1));

        proa_macros::html_sync! {
            <button
                type="button"
                data-launch-counter
                on_click={increment()}
            >
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<'a, T, const N: usize, L, B> WebRenderSync<L, B> for GenericLaunchCounter<'a, T, N>
where
    T: AsRef<str>,
    B: proa_core::WriteBuf,
    L: proa_core::DataLoader,
{
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let label = self.label.as_ref();
        let count = signal::<i32>(self.initial);
        let increment = event_handler(|_: rsjs::Event| count.update(|value| value + 1));

        proa_macros::html_sync! {
            <button
                type="button"
                aria-label={label}
                on_click={increment()}
            >
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

#[test]
fn explicit_trait_is_the_normal_static_dispatch_render_path() {
    fn assert_sync<R: WebRenderSync<NoLoader, Vec<u8>>>(_: R) {}

    assert_sync(LaunchCounter { initial: 41 });

    let html = render_to_string(LaunchCounter { initial: 41 });
    assert!(
        html.starts_with("<rsjs-island data-rsjs-island-root=\"launchcounter_"),
        "reachable browser behavior should emit the island wrapper: {html}"
    );
    assert!(
        html.contains("data-launch-counter")
            && html.contains(r#"data-rsjs-bind="0""#)
            && html.contains(r#"data-rsjs-handler="0">41</button>"#),
        "the dynamic input and click marker should survive analyzed explicit-trait expansion: {html}"
    );
    assert!(
        html.contains(r#"{"signals":[41]}"#),
        "the dynamic initial value should be the hydration seed: {html}"
    );
}

#[test]
fn explicit_trait_registers_the_real_client_module() {
    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "LaunchCounter")
        .expect("LaunchCounter should register a hydrated island");

    assert!(!entry.is_async);
    assert!(!entry.uses_queries);
    assert!(!entry.uses_mutations);
    assert!(
        entry.js_source.contains("__sig_0.update(")
            && entry
                .js_source
                .contains("checked_int_add(__sig_0.get(), 1, \"i32\")"),
        "the emitted client module should preserve the increment: {}",
        entry.js_source
    );
}

#[test]
fn explicit_trait_preserves_lifetime_type_generics_and_where_bounds() {
    fn assert_sync<R: WebRenderSync<NoLoader, Vec<u8>>>(_: R) {}

    let label = String::from("Increase total");
    assert_sync(GenericLaunchCounter {
        label: &label,
        initial: 8,
        marker: [(); 2],
    });

    let html = render_to_string(GenericLaunchCounter {
        label: &label,
        initial: 8,
        marker: [(); 2],
    });
    assert!(
        html.contains(r#"aria-label="Increase total""#)
            && html.contains(r#"data-rsjs-bind="0""#)
            && html.contains(r#"data-rsjs-handler="0">8</button>"#)
            && html.contains(r#"{"signals":[8]}"#),
        "generic trait impl should retain its borrowed server prop and dynamic seed: {html}"
    );
}
