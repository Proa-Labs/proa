// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for generic `#[rsjs(component, client)]` impl headers.
//!
//! The macro used to reject generic self types and impl-level where
//! clauses because the Rust emitter rebuilt a fresh `impl<L, B> ...`
//! header. This test exercises lifetimes, type parameters, const
//! parameters, renamed buffer/loader generics, and a where clause.

#[allow(unused_imports)]
use proa_core::{render_to_string, WebContext, WebRenderSync, WriteBuf, WriteError};
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

    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}

pub trait LabelSource {
    fn label(&self) -> &'static str;
}

pub trait LabelRef {
    fn label_ref(self) -> &'static str;
}

struct FixtureLabel;

impl LabelSource for FixtureLabel {
    fn label(&self) -> &'static str {
        "generic label"
    }
}

#[allow(clippy::needless_lifetimes)] // Explicit lifetime is parser coverage.
impl<'a> LabelRef for &'a FixtureLabel {
    fn label_ref(self) -> &'static str {
        self.label()
    }
}

pub struct GenericIsland<'a, T, const N: usize> {
    pub label: &'a T,
}

pub struct GenericServerOnlyIsland<'a, T> {
    pub label: &'a T,
}

pub struct GenericUnusedPropSignalIsland<'a, T> {
    pub label: &'a T,
}

pub struct GenericClientHelperPropIsland {
    pub cents: i64,
}

pub struct GenericClientHelperServerPreludeIsland;

pub struct SignalAliasIsland {
    pub initial: i64,
}

#[rsjs::client_module(path = "crate::price_math")]
pub mod price_math {
    #[rsjs::client]
    pub fn double(value: i64) -> i64 {
        value * 2
    }
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
        proa_macros::html_sync! {
            <div data-kind="generic">
                <span>{count.get()}</span>
                {rsjs::server_only(label)}
            </div>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<'a, T, Buf, Loader> WebRenderSync<Loader, Buf> for GenericServerOnlyIsland<'a, T>
where
    for<'b> &'b T: LabelRef,
    Buf: WriteBuf,
    Loader: proa_core::DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let label = self.label.label_ref();
        proa_macros::html_sync! {
            <div data-kind="generic-server-only" aria_label={label}>{label}</div>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<'a, T, Buf, Loader> WebRenderSync<Loader, Buf> for GenericUnusedPropSignalIsland<'a, T>
where
    for<'b> &'b T: LabelRef,
    Buf: WriteBuf,
    Loader: proa_core::DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let label = self.label.label_ref();
        let _draft = signal(self.label);
        proa_macros::html_sync! {
            <div data-kind="generic-unused-prop-signal">{label}</div>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<Buf, Loader> WebRenderSync<Loader, Buf> for GenericClientHelperPropIsland
where
    Buf: WriteBuf,
    Loader: proa_core::DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let cents = signal(self.cents);
        proa_macros::html_sync! {
            <div data-kind="generic-client-helper-prop">
                <span>{price_math::double(cents.get())}</span>
            </div>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<Buf, Loader> WebRenderSync<Loader, Buf> for GenericClientHelperServerPreludeIsland
where
    Buf: WriteBuf,
    Loader: proa_core::DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let doubled = price_math::double(21_i64);
        proa_macros::html_sync! {
            <div data-kind="generic-client-helper-server-prelude">
                <span>{doubled}</span>
            </div>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<Buf, Loader> WebRenderSync<Loader, Buf> for SignalAliasIsland
where
    Buf: WriteBuf,
    Loader: proa_core::DataLoader,
{
    fn render(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
        let count = signal(self.initial);
        let display = count.get();
        proa_macros::html_sync! {
            <div data-kind="signal-alias">
                <span>{display}</span>
            </div>
        }
        .render(cx)
    }
}

mod async_case {
    use super::*;
    use proa_core::WebRender;

    pub struct AsyncGenericIsland<'a, T, const N: usize> {
        pub label: &'a T,
    }

    #[rsjs(component, client)]
    impl<'a, T, const N: usize, Buf> WebRender<::proa_core::NoLoader, Buf>
        for AsyncGenericIsland<'a, T, N>
    where
        T: LabelSource,
        Buf: WriteBuf,
    {
        fn render(
            self,
            cx: &mut WebContext<::proa_core::NoLoader, Buf>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                let label = self.label.label();
                let status = signal("");
                proa_macros::html! {
                    <div data-kind="async-generic">
                        <span>{status.get()}</span>
                        {rsjs::server_only(label)}
                    </div>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }
}

#[test]
fn signal_alias_hydrates_without_replaying_client_alias_let() {
    let html = render_to_string(SignalAliasIsland { initial: 5 });
    assert!(
        html.contains(r#"data-kind="signal-alias""#),
        "expected server HTML to render, got:\n{html}"
    );
    assert!(
        html.contains(">5<") && html.contains("<rsjs-island") && html.contains("data-rsjs-bind"),
        "signal-derived prelude aliases should hydrate as bindings:\n{html}"
    );
}

#[test]
fn generic_impl_header_is_preserved() {
    let label = FixtureLabel;
    let html = render_to_string(GenericIsland::<FixtureLabel, 7> { label: &label });
    assert!(
        html.contains(r#"data-kind="generic""#),
        "expected static attributes to render, got:\n{html}"
    );
    assert!(
        html.contains("generic label"),
        "expected generic prelude/server-only content to render, got:\n{html}"
    );
    assert!(
        html.contains("data-rsjs-bind"),
        "expected the reactive binding to remain present, got:\n{html}"
    );
}

#[test]
fn generic_ssr_prop_stays_server_only() {
    let label = FixtureLabel;
    let html = render_to_string(GenericServerOnlyIsland { label: &label });
    assert!(
        html.contains(r#"data-kind="generic-server-only""#),
        "expected server-only markup to render, got:\n{html}"
    );
    assert!(
        html.contains("generic label"),
        "expected generic SSR prop text to render, got:\n{html}"
    );
    assert!(
        html.contains(r#"aria-label="generic label""#),
        "expected generic SSR attr prop to render, got:\n{html}"
    );
    assert!(
        !html.contains("rsjs-island") && !html.contains("data-rsjs"),
        "generic SSR props must not force hydration:\n{html}"
    );
    assert!(
        !html.contains("<script"),
        "server-only strategy must not emit seed scripts:\n{html}"
    );
}

#[test]
fn unused_prop_seeded_signal_stays_server_only() {
    let label = FixtureLabel;
    let html = render_to_string(GenericUnusedPropSignalIsland { label: &label });
    assert!(
        html.contains(r#"data-kind="generic-unused-prop-signal""#),
        "expected server-only markup to render, got:\n{html}"
    );
    assert!(
        html.contains("generic label"),
        "expected generic prop text to render through SSR, got:\n{html}"
    );
    assert!(
        !html.contains("rsjs-island") && !html.contains("data-rsjs"),
        "unused prop-seeded signals must not force hydration or serialize the prop:\n{html}"
    );
    assert!(
        !html.contains("<script"),
        "unused prop-seeded signals must not emit seed scripts:\n{html}"
    );
}

#[test]
fn reachable_client_helper_prop_seed_hydrates() {
    let html = render_to_string(GenericClientHelperPropIsland { cents: 21 });
    assert!(
        html.contains(r#"data-kind="generic-client-helper-prop""#),
        "expected server HTML to render, got:\n{html}"
    );
    assert!(
        html.contains("<rsjs-island") && html.contains("data-rsjs-bind"),
        "reachable client helper call should require hydration:\n{html}"
    );
    assert!(
        html.contains(r#""signals":["21"]"#),
        "prop-seeded signal should serialize only because the helper call is reachable:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("genericclienthelperpropisland_"))
        .expect("GenericClientHelperPropIsland should be registered")
        .js_source;
    assert!(
        js.contains("__rsjs_unlinked_helper_edge_0(") && !js.contains("__rsjs_client_module_"),
        "descriptor-stage JS should retain the reachable helper as a symbolic edge without \
         a component allowlist; got:\n{js}"
    );
}

#[test]
fn pure_server_prelude_client_helper_result_stays_server_only() {
    let html = render_to_string(GenericClientHelperServerPreludeIsland);
    assert!(
        html.contains(r#"data-kind="generic-client-helper-server-prelude""#)
            && html.contains(">42<"),
        "expected server helper output to render as SSR HTML, got:\n{html}"
    );
    assert!(
        !html.contains("<rsjs-island") && !html.contains("data-rsjs") && !html.contains("<script"),
        "pure server prelude helper result must not force hydration:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.islands.iter().all(|entry| {
            !entry
                .asset_id
                .starts_with("genericclienthelperserverpreludeisland_")
        }),
        "pure server prelude helper result should not register a runtime island"
    );
}

#[test]
fn async_generic_impl_header_typechecks() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}

    let label = FixtureLabel;
    assert_web_render(async_case::AsyncGenericIsland::<FixtureLabel, 7> { label: &label });
}
