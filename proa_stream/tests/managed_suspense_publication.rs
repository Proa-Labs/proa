// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression for a real in-order flusher retaining the managed document prefix.
use futures::{executor::block_on, FutureExt};
use proa_core::{head_dispatch::ComponentRole, managed_head::ManagedHead, *};
use proa_macros::{html, html_send};
use proa_stream::Suspense;
use std::{
    future::{pending, Future},
    panic::AssertUnwindSafe,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Waker},
};

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl Sink {
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}
impl StreamChunkFlusher for Sink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.0.lock().unwrap().extend_from_slice(&chunk);
        Ok(())
    }
}
impl SendStreamChunkFlusher for Sink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        StreamChunkFlusher::flush_chunk(self, chunk)
    }
}
fn document(sink: &Sink) -> ManagedHead {
    let sink = sink.clone();
    ManagedHead::new(
        1024 * 1024,
        Arc::new(move |_, chunks| {
            for chunk in chunks {
                sink.0.lock().unwrap().extend_from_slice(&chunk);
            }
            Ok(())
        }),
    )
}
fn setup() -> (WebContext, ManagedHead, Sink) {
    let sink = Sink::default();
    let d = document(&sink);
    let mut cx = WebContext::new()
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(Rc::new(sink.clone()));
    cx.head_mut().manage(d.clone());
    (cx, d, sink)
}
fn setup_send() -> (SendWebContext, ManagedHead, Sink) {
    let sink = Sink::default();
    let d = document(&sink);
    let mut cx = SendWebContext::new().with_render_mode(RenderMode::ChunkedInOrder);
    cx.set_stream_chunk_flusher(Arc::new(sink.clone()));
    cx.head_mut().manage(d.clone());
    (cx, d, sink)
}

struct Page;
impl WebRender for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Selected page")
    }
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<_>>::Done(cx.output_mut().extend(b"PAGE"))
    }
}
impl WebRenderSend for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head_send(&self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Selected page")
    }
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<_>>::Done(cx.output_mut().extend(b"PAGE"))
    }
}

#[derive(Clone, Copy)]
enum Finish {
    Ok,
    Error,
    Pending,
    Panic,
}
struct Child(Finish);
async fn finish(how: Finish) -> Result<(), WriteError> {
    match how {
        Finish::Ok => Ok(()),
        Finish::Error => Err(WriteError::render("child failed")),
        Finish::Pending => pending().await,
        Finish::Panic => panic!("child panicked"),
    }
}
impl WebRender for Child {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend(b"CHILD")?;
            finish(self.0).await
        })
    }
}
impl WebRenderSend for Child {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend(b"CHILD")?;
            finish(self.0).await
        })
    }
}
impl MdRender for Child {
    fn render_md(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend(b"CHILD")?;
            finish(self.0).await
        })
    }
}

struct AssertHeld(Sink);
impl WebRender for AssertHeld {
    fn render(
        self,
        _cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        assert!(
            self.0.bytes().is_empty(),
            "nothing may escape before publication"
        );
        RenderOutcome::<std::future::Ready<_>>::Done(Ok(()))
    }
}

#[test]
fn failed_suspense_rewinds_only_its_boundary_then_reaches_page_and_publishes() {
    // One fallback is shorter than the saved outlet offset; the other is longer.
    // Before the fix these caused an invalid offset or a head inside fallback text.
    for fallback in [
        "recovered",
        concat!(
            "a long fallback must not receive the old document head offset; ",
            "the original document prefix must survive and contain the title"
        ),
    ] {
        let (mut cx, d, sink) = setup();
        let head = d.outlet();
        let recovered = ErrorBoundary::new(Suspense::new(Child(Finish::Error), ()), fallback);
        let held = AssertHeld(sink.clone());
        block_on(render_child(
            &mut cx,
            html! {
                <!DOCTYPE html>
                <html>
                    <head>{head}</head>
                    <body>{recovered}{held}{Page}</body>
                </html>
            },
        ))
        .unwrap();
        d.final_check().unwrap();
        cx.flush_stream_chunk().unwrap();
        assert_eq!(String::from_utf8(sink.bytes()).unwrap(), format!(
            "<!DOCTYPE html><!-- Rendered with Proa --><html><head><title>Selected page</title></head><body>{fallback}PAGE</body></html>"
        ));
    }
}

#[test]
fn nested_suspense_failure_keeps_document_prefix() {
    let (mut cx, d, sink) = setup();
    let head = d.outlet();
    let recovered = ErrorBoundary::new(
        Suspense::new(Suspense::new(Child(Finish::Error), ()), ()),
        "recovered",
    );
    block_on(render_child(
        &mut cx,
        html! {
            <head>{head}</head>
            <body>{recovered}{Page}</body>
        },
    ))
    .unwrap();
    d.final_check().unwrap();
    cx.flush_stream_chunk().unwrap();
    assert_eq!(
        sink.bytes(),
        b"<head><title>Selected page</title></head><body>recoveredPAGE</body>"
    );
}

#[test]
fn successful_suspense_is_held_before_publication_and_streams_after_it() {
    let (mut cx, d, sink) = setup();
    let head = d.outlet();
    let held = AssertHeld(sink.clone());
    block_on(render_child(
        &mut cx,
        html! {
            <head>{head}</head>
            {Suspense::new(Child(Finish::Ok), ())}
            {held}
            {Page}
        },
    ))
    .unwrap();
    assert!(d.is_published());
    assert_eq!(
        sink.bytes(),
        b"<head><title>Selected page</title></head>CHILD"
    );
    block_on(render_child(&mut cx, Suspense::new(Child(Finish::Ok), ()))).unwrap();
    assert_eq!(
        sink.bytes(),
        b"<head><title>Selected page</title></head>CHILDPAGECHILD"
    );
    assert!(cx.output().is_empty());
}

#[test]
fn sealed_head_without_outlet_still_uses_buffered_suspense() {
    let (mut cx, d, sink) = setup();
    block_on(render_child(&mut cx, Page)).unwrap();
    assert!(!d.is_published());
    block_on(render_child(
        &mut cx,
        ErrorBoundary::new(Suspense::new(Child(Finish::Error), ()), "recovered"),
    ))
    .unwrap();
    assert_eq!(cx.output().to_vec(), b"PAGErecovered");
    assert!(sink.bytes().is_empty());
    let head = d.outlet();
    block_on(render_child(
        &mut cx,
        html! {
            <head>{head}</head>
        },
    ))
    .unwrap();
    d.final_check().unwrap();
    cx.flush_stream_chunk().unwrap();
    assert_eq!(
        sink.bytes(),
        b"PAGErecovered<head><title>Selected page</title></head>"
    );
}

#[test]
fn local_unhandled_error_cancellation_and_unwind_preserve_held_prefix() {
    for how in [Finish::Error, Finish::Pending, Finish::Panic] {
        let (mut cx, d, sink) = setup();
        let head = d.outlet();
        block_on(render_child(
            &mut cx,
            html! {
                <head>{head}</head>
            },
        ))
        .unwrap();
        {
            let future = render_child(&mut cx, Suspense::new(Child(how), ()));
            match how {
                Finish::Error => assert!(block_on(future).is_err()),
                Finish::Pending => {
                    let mut future = std::pin::pin!(future);
                    assert!(future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending());
                }
                Finish::Panic => {
                    assert!(block_on(AssertUnwindSafe(future).catch_unwind()).is_err())
                }
                Finish::Ok => unreachable!(),
            }
        }
        // Buffered cancellation is not a transaction. The request/outer boundary
        // owns disposal or recovery of the partial child, never the whole prefix.
        assert_eq!(cx.output().to_vec(), b"<head></head>CHILD");
        assert!(!d.is_published());
        assert!(sink.bytes().is_empty());
    }
}

#[test]
fn send_error_cancellation_and_unwind_preserve_held_prefix() {
    for how in [Finish::Error, Finish::Pending, Finish::Panic] {
        let (mut cx, d, sink) = setup_send();
        let head = d.outlet();
        block_on(render_send_child(&mut cx, html_send! {<head>{head}</head>})).unwrap();
        {
            let future = render_send_child(&mut cx, Suspense::new(Child(how), ()));
            fn require_send<T: Send>(value: T) -> T {
                value
            }
            let future = require_send(future);
            match how {
                Finish::Error => assert!(block_on(future).is_err()),
                Finish::Pending => {
                    let mut future = std::pin::pin!(future);
                    assert!(future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending());
                }
                Finish::Panic => {
                    assert!(block_on(AssertUnwindSafe(future).catch_unwind()).is_err())
                }
                Finish::Ok => unreachable!(),
            }
        }
        assert_eq!(cx.output().to_vec(), b"<head></head>CHILD");
        assert!(!d.is_published());
        assert!(sink.bytes().is_empty());
    }
}

#[test]
fn send_seal_without_outlet_retains_prefix_and_published_suspense_flushes() {
    let (mut cx, d, sink) = setup_send();
    block_on(render_send_child(&mut cx, Page)).unwrap();
    assert!(!d.is_published());
    assert!(block_on(render_send_child(
        &mut cx,
        Suspense::new(Child(Finish::Error), ())
    ))
    .is_err());
    assert_eq!(cx.output().to_vec(), b"PAGECHILD");
    assert!(sink.bytes().is_empty());

    // Use a fresh request for success; a failed request is not republished.
    let (mut cx, d, sink) = setup_send();
    let head = d.outlet();
    block_on(render_send_child(&mut cx, html_send! {<head>{head}</head>})).unwrap();
    block_on(render_send_child(
        &mut cx,
        Suspense::new(Child(Finish::Ok), ()),
    ))
    .unwrap();
    assert!(sink.bytes().is_empty());
    block_on(render_send_child(&mut cx, Page)).unwrap();
    assert!(d.is_published());
    block_on(render_send_child(
        &mut cx,
        Suspense::new(Child(Finish::Ok), ()),
    ))
    .unwrap();
    d.final_check().unwrap();
    assert_eq!(
        sink.bytes(),
        b"<head><title>Selected page</title></head>CHILDPAGECHILD"
    );
    assert!(cx.output().is_empty());
}

#[test]
fn markdown_suspense_sharing_managed_context_cannot_clear_held_prefix() {
    // Markdown has no metadata lifecycle of its own, but can use the same
    // WebContext/writer. Its cleanup must respect the same held-flush contract.
    for how in [Finish::Error, Finish::Pending, Finish::Panic] {
        let (mut cx, d, sink) = setup();
        let head = d.outlet();
        block_on(render_child(
            &mut cx,
            html! {
                <head>{head}</head>
            },
        ))
        .unwrap();
        {
            let future = render_md_child(&mut cx, Suspense::new(Child(how), ()));
            match how {
                Finish::Error => assert!(block_on(future).is_err()),
                Finish::Pending => {
                    let mut future = std::pin::pin!(future);
                    assert!(future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending());
                }
                Finish::Panic => {
                    assert!(block_on(AssertUnwindSafe(future).catch_unwind()).is_err())
                }
                Finish::Ok => unreachable!(),
            }
        }
        assert_eq!(cx.output().to_vec(), b"<head></head>CHILD");
        assert!(!d.is_published());
        assert!(sink.bytes().is_empty());
    }
}

#[test]
fn recovery_preserves_prefix_and_outlet_across_chained_buffer_pages() {
    let (mut cx, d, sink) = setup();
    let head = d.outlet();
    let padding = "x".repeat(9_000);
    let expected = format!(
        "<head><meta name=\"padding\" content=\"{padding}\"><title>Selected page</title></head><body>recoveredPAGE</body>"
    );
    let recovered = ErrorBoundary::new(Suspense::new(Child(Finish::Error), ()), "recovered");
    block_on(render_child(
        &mut cx,
        html! {
            <head>
                <meta name="padding" content={padding} />
                {head}
            </head>
            <body>{recovered}{Page}</body>
        },
    ))
    .unwrap();
    d.final_check().unwrap();
    cx.flush_stream_chunk().unwrap();
    assert_eq!(String::from_utf8(sink.bytes()).unwrap(), expected);
}
