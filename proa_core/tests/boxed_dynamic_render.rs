// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use proa_core::{
    boxed_render, boxed_sync_render, render_in_context_async, render_to_string_async,
    render_to_vec, BoxedWebRender, BoxedWebRenderSync, DataLoader, NoLoader, RenderOutcome,
    WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html, html_sync};

struct Alpha;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Alpha {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend(b"alpha")
    }
}

struct Beta;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Beta {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend(b"beta")
    }
}

struct DynamicSync {
    alpha: bool,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for DynamicSync {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // Both branches have one erased type even though their concrete leaf
        // types differ. B and L are inferred from the surrounding render.
        let child = if self.alpha {
            boxed_sync_render(Alpha)
        } else {
            boxed_sync_render(Beta)
        };
        WebRenderSync::render(
            html_sync! {
                <section>{child}</section>
            },
            cx,
        )
    }
}

struct AsyncAlpha;

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for AsyncAlpha {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            std::future::ready(()).await;
            cx.out.extend(b"async-alpha")
        })
    }
}

struct AsyncBeta;

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for AsyncBeta {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            std::future::ready(()).await;
            cx.out.extend(b"async-beta")
        })
    }
}

struct DynamicAsync {
    alpha: bool,
}

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for DynamicAsync {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let child = if self.alpha {
            boxed_render(AsyncAlpha)
        } else {
            boxed_render(AsyncBeta)
        };
        WebRender::render(
            html! {
                <section>{child}</section>
            },
            cx,
        )
    }
}

struct LocalFuture;

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for LocalFuture {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        // Capturing Rc makes this future !Send. Dynamic rendering only
        // requires the component itself to be Send; it does not invent a
        // stronger future bound than WebRender has.
        let local = Rc::new("local-future");
        RenderOutcome::Pending(async move {
            std::future::ready(()).await;
            cx.out.extend(local.as_bytes())
        })
    }
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct NeverReady {
    dropped: Arc<AtomicBool>,
}

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for NeverReady {
    fn render(
        self,
        _cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let guard = DropFlag(self.dropped);
        RenderOutcome::Pending(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
            Ok(())
        })
    }
}

fn assert_send<T: Send>() {}

#[test]
fn heterogeneous_sync_children_compose_in_html_sync() {
    let alpha = render_to_vec(DynamicSync { alpha: true });
    let beta = render_to_vec(DynamicSync { alpha: false });

    assert_eq!(alpha, b"<section>alpha</section>");
    assert_eq!(beta, b"<section>beta</section>");
}

#[tokio::test]
async fn heterogeneous_async_children_compose_in_html() {
    let alpha = render_to_string_async(DynamicAsync { alpha: true }).await;
    let beta = render_to_string_async(DynamicAsync { alpha: false }).await;

    assert_eq!(alpha, "<section>async-alpha</section>");
    assert_eq!(beta, "<section>async-beta</section>");
}

#[tokio::test]
async fn sync_erasure_can_cross_an_explicit_async_boundary() {
    let rendered = render_to_string_async(boxed_sync_render(Alpha)).await;
    assert_eq!(rendered, "alpha");
}

#[tokio::test]
async fn async_erasure_accepts_a_non_send_render_future() {
    let rendered = render_to_string_async(boxed_render(LocalFuture)).await;
    assert_eq!(rendered, "local-future");
}

#[test]
fn erased_component_wrappers_are_send() {
    assert_send::<BoxedWebRenderSync<NoLoader, Vec<u8>>>();
    assert_send::<BoxedWebRender<NoLoader, Vec<u8>>>();
}

#[tokio::test]
async fn from_sync_uses_the_immediate_async_path() {
    let child = BoxedWebRender::<NoLoader, Vec<u8>>::from_sync(Beta);
    let mut cx = WebContext::with_buffer(Vec::new());
    let outcome = WebRender::render(child, &mut cx);
    assert!(matches!(&outcome, RenderOutcome::Done(Ok(()))));
    drop(outcome);
    assert_eq!(cx.output(), b"beta");
}

#[test]
fn dropping_an_erased_pending_render_drops_its_future() {
    let dropped = Arc::new(AtomicBool::new(false));
    let child: BoxedWebRender<NoLoader, Vec<u8>> = boxed_render(NeverReady {
        dropped: Arc::clone(&dropped),
    });
    let mut cx = WebContext::with_buffer(Vec::new());

    let outcome = WebRender::render(child, &mut cx);
    assert!(matches!(&outcome, RenderOutcome::Pending(_)));
    assert!(!dropped.load(Ordering::SeqCst));

    drop(outcome);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn erased_async_child_works_with_the_reusable_context_driver() {
    let mut cx = WebContext::with_buffer(Vec::new());
    render_in_context_async(&mut cx, boxed_render(AsyncBeta))
        .await
        .unwrap();
    assert_eq!(cx.output(), b"async-beta");
}
