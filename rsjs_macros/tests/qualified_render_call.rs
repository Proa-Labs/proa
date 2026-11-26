// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A module that imports both render traits cannot call `.render(cx)` on a
//! template (the method is ambiguous), so analyzed components must also accept
//! the qualified `WebRenderSync::render(..)` / `WebRender::render(..)` tails.
//! Each fixture compiles through the real attribute macro and is rendered, so
//! the lossless tail rewrite is proven for every accepted spelling.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use proa_core::{
    render_to_string, render_to_string_async, DataLoader, NoLoader, RenderOutcome, WebContext,
    WebRender, WebRenderSync, WriteBuf, WriteError,
};
use rsjs::{event_handler, rsjs, signal};

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct BareSync {
    initial: i32,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BareSync {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal::<i32>(self.initial);
        let increment = event_handler(|_: rsjs::Event| count.update(|value| value + 1));

        WebRenderSync::render(
            proa_macros::html_sync! {
                <button type="button" data-bare-sync on_click={increment()}>{count.get()}</button>
            },
            cx,
        )
    }
}

struct CratePathSync {
    label: &'static str,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CratePathSync {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let label = self.label;
        ::proa_core::WebRenderSync::render(
            proa_macros::html_sync! {
                <span data-crate-path>{label}</span>
            },
            cx,
        )
    }
}

struct TurbofishSync {
    total: u32,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for TurbofishSync {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let total = self.total;
        WebRenderSync::<L, B>::render(
            proa_macros::html_sync! {
                <output data-turbofish>{total}</output>
            },
            cx,
        )
    }
}

struct QualifiedSelfSync {
    total: u32,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for QualifiedSelfSync {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let total = self.total;
        <_ as WebRenderSync<L, B>>::render(
            proa_macros::html_sync! {
                <output data-qualified-self>{total}</output>
            },
            cx,
        )
    }
}

struct ReturningSync {
    visible: bool,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ReturningSync {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let visible = self.visible;
        return WebRenderSync::render(
            proa_macros::html_sync! {
                <i data-returning>{visible}</i>
            },
            cx,
        );
    }
}

struct BareAsync {
    initial: i32,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for BareAsync {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let count = signal::<i32>(self.initial);
            WebRender::render(
                proa_macros::html! {
                    <button data-bare-async on_click={|_| count.update(|value| value + 1)}>
                        {count.get()}
                    </button>
                },
                cx,
            )
            .resolve()
            .await
        })
    }
}

struct CratePathAsync {
    label: &'static str,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for CratePathAsync {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let label = self.label;
            proa_core::WebRender::render(
                proa_macros::html! {
                    <span data-crate-path-async>{label}</span>
                },
                cx,
            )
            .resolve()
            .await
        })
    }
}

/// The authored element (identified by its data attribute) must carry the
/// escaped dynamic value, whatever wrapper the local renderer adds around it.
fn assert_rendered(html: &str, element_marker: &str, closing: &str) {
    assert!(
        html.contains(element_marker) && html.contains(closing),
        "expected `{element_marker}` ending in `{closing}`: {html}"
    );
}

#[test]
fn every_qualified_sync_spelling_compiles_and_renders_its_dynamic_value() {
    fn assert_sync<R: WebRenderSync<NoLoader, Vec<u8>>>(_: &R) {}

    let crate_path = CratePathSync { label: "a<b" };
    assert_sync(&crate_path);
    assert_rendered(
        &render_to_string(crate_path),
        "<span data-crate-path",
        ">a&lt;b</span>",
    );
    assert_rendered(
        &render_to_string(TurbofishSync { total: 7 }),
        "<output data-turbofish",
        ">7</output>",
    );
    assert_rendered(
        &render_to_string(QualifiedSelfSync { total: 9 }),
        "<output data-qualified-self",
        ">9</output>",
    );
    assert_rendered(
        &render_to_string(ReturningSync { visible: true }),
        "<i data-returning",
        ">true</i>",
    );
}

#[test]
fn qualified_sync_tail_keeps_the_hydrated_island_rewrite() {
    let html = render_to_string(BareSync { initial: 41 });
    assert!(
        html.starts_with("<rsjs-island data-rsjs-island-root=\"baresync_"),
        "the qualified tail must still receive the island wrapper: {html}"
    );
    assert!(
        html.contains("data-bare-sync")
            && html.contains(r#"data-rsjs-handler="0">41</button>"#)
            && html.contains(r#"{"signals":[41]}"#),
        "handler marker and hydration seed must survive the qualified tail: {html}"
    );
    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "BareSync")
        .expect("BareSync should register a hydrated island");
    assert!(!entry.is_async);
}

#[test]
fn qualified_async_tails_render_and_hydrate() {
    assert_rendered(
        &block_on(render_to_string_async(CratePathAsync { label: "a&b" })),
        "<span data-crate-path-async",
        ">a&amp;b</span>",
    );

    let html = block_on(render_to_string_async(BareAsync { initial: 5 }));
    assert!(
        html.starts_with("<rsjs-island data-rsjs-island-root=\"bareasync_"),
        "the qualified async tail must still receive the island wrapper: {html}"
    );
    assert!(
        html.contains("data-bare-async")
            && html.contains(r#"data-rsjs-handler="0">5</button>"#)
            && html.contains(r#"{"signals":[5]}"#),
        "handler marker and hydration seed must survive the qualified async tail: {html}"
    );
    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "BareAsync")
        .expect("BareAsync should register a hydrated island");
    assert!(entry.is_async);
}
