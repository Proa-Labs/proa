// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression tests for corrected gaps, plus explicit documented API boundaries.
use proa_core::{
    head_dispatch::{self, ComponentRole, HeadOperation},
    managed_head::{HeadError, ManagedHead},
    HeadContext, RenderBuffer, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteBuf,
    WriteError,
};
use std::{
    future::{Future, Ready},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Waker},
};
type Wire = Arc<Mutex<Vec<u8>>>;
fn setup(limit: usize) -> (WebContext, ManagedHead, Wire) {
    let wire: Wire = Default::default();
    let sink = wire.clone();
    let doc = ManagedHead::new(
        limit,
        Arc::new(move |_, parts| {
            for p in parts {
                sink.lock().unwrap().extend_from_slice(&p);
            }
            Ok(())
        }),
    );
    let mut cx = WebContext::new();
    cx.head_mut().manage(doc.clone());
    (cx, doc, wire)
}
struct Page;
impl WebRenderSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, _: &mut WebContext) -> Result<(), WriteError> {
        Ok(())
    }
}
struct Provider(Arc<AtomicUsize>);
impl WebRender for Provider {
    const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;
    fn render_head(&self, cx: &mut WebContext) -> impl HeadOperation {
        self.0.fetch_add(1, Ordering::SeqCst);
        async move { cx.head_mut().title("child") }
    }
    fn render(
        self,
        _: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}
#[test]
fn documented_raw_render_call_can_silently_skip_child_hook() {
    let (mut cx, doc, _) = setup(4096);
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(matches!(
        Provider(calls.clone()).render(&mut cx),
        RenderOutcome::Done(Ok(()))
    ));
    WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
    head_dispatch::sync(Page, &mut cx).unwrap();
    doc.final_check().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
fn lifecycle(error: WriteError) -> HeadError {
    match error {
        WriteError::HeadLifecycle(e) => e,
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn captures_cannot_publish_the_document() {
    let (mut cx, doc, wire) = setup(4096);
    cx.output_mut().extend(b"OUTER_PREFIX").unwrap();
    let result = cx.with_output(RenderBuffer::new(), |capture| {
        capture.output_mut().extend(b"CAPTURED_FRAGMENT")?;
        WebRenderSync::render(doc.outlet(), capture)?;
        head_dispatch::sync(Page, capture)
    });
    assert_eq!(lifecycle(result.unwrap_err()), HeadError::Excluded);
    assert!(doc.final_check().is_err());
    assert!(wire.lock().unwrap().is_empty());
    assert_eq!(cx.output().len(), b"OUTER_PREFIX".len());
}
#[test]
fn capture_outlet_offset_cannot_reach_root_buffer() {
    let (mut cx, doc, wire) = setup(4096);
    cx.output_mut().extend(b"ROOT_PREFIX").unwrap();
    let result = cx.with_output(RenderBuffer::new(), |capture| {
        capture.output_mut().extend(b"xx")?;
        WebRenderSync::render(doc.outlet(), capture)
    });
    assert_eq!(lifecycle(result.unwrap_err()), HeadError::Excluded);
    assert!(head_dispatch::sync(Page, &mut cx).is_err());
    assert!(wire.lock().unwrap().is_empty());
}
#[test]
fn legacy_state_replacement_is_rejected_and_sticky() {
    let (mut cx, doc, wire) = setup(4096);
    cx.output_mut().extend(b"PREFIX").unwrap();
    WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
    head_dispatch::sync(Page, &mut cx).unwrap();
    cx.set_head(HeadContext::new());
    assert_eq!(
        lifecycle(cx.head_mut().title("lost").unwrap_err()),
        HeadError::StateReplaced
    );
    assert_eq!(
        lifecycle(doc.final_check().unwrap_err()),
        HeadError::StateReplaced
    );
    assert_eq!(&*wire.lock().unwrap(), b"PREFIX");
}
#[test]
fn context_parts_cannot_replace_managed_authority() {
    let (mut cx, doc, _) = setup(4096);
    cx.set_context_parts(proa_core::WebContextParts::new());
    assert_eq!(
        lifecycle(doc.final_check().unwrap_err()),
        HeadError::StateReplaced
    );
    assert!(cx.head_mut().title("lost").is_err());
}
#[test]
fn reinstalling_document_cannot_reset_authority() {
    let (mut cx, doc, _) = setup(4096);
    let (_, replacement, _) = setup(4096);
    cx.head_mut().manage(replacement);
    assert_eq!(
        lifecycle(doc.final_check().unwrap_err()),
        HeadError::StateReplaced
    );
}
#[test]
fn legacy_raw_and_unchecked_setters_cannot_bypass_permission() {
    let (mut cx, doc, wire) = setup(4096);
    cx.head_mut().push_raw_html("<script>untrusted()</script>");
    assert_eq!(
        lifecycle(doc.final_check().unwrap_err()),
        HeadError::LegacyMutation
    );
    assert!(wire.lock().unwrap().is_empty());
    let mut cx = proa_core::SendWebContext::new();
    let (_, doc, _) = setup(4096);
    cx.head_mut().manage(doc.clone());
    cx.head_mut().set_title("unchecked");
    assert_eq!(
        lifecycle(doc.final_check().unwrap_err()),
        HeadError::LegacyMutation
    );
}
#[test]
fn body_only_capture_is_allowed_and_restores_permission() {
    let (mut cx, doc, _) = setup(4096);
    let hook = doc.begin_hook().unwrap();
    let scope = hook.finish(Ok(())).unwrap();
    cx.with_output(RenderBuffer::new(), |capture| {
        capture.output_mut().extend(b"BODY")
    })
    .unwrap();
    WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
    head_dispatch::sync(Page, &mut cx).unwrap();
    scope.finish(Ok(())).unwrap();
    doc.final_check().unwrap();
}
#[test]
fn capture_exclusion_restores_on_unwind() {
    let (mut cx, doc, _) = setup(4096);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.with_output(RenderBuffer::new(), |_| panic!("capture abort"));
    }));
    WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
    head_dispatch::sync(Page, &mut cx).unwrap();
    doc.final_check().unwrap();
}
struct InheritedAsyncPage(Arc<AtomicUsize>);
impl WebRender for InheritedAsyncPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        RenderOutcome::<Ready<Result<(), WriteError>>>::Done(cx.output_mut().extend(b"BODY"))
    }
}

#[test]
fn render_child_with_inherited_head_yields_only_for_streaming_publication() {
    for mode in [
        proa_core::RenderMode::Static,
        proa_core::RenderMode::Agent,
        proa_core::RenderMode::ChunkedInOrder,
        proa_core::RenderMode::ChunkedOutOfOrder,
    ] {
        let (mut cx, doc, wire) = setup(4096);
        cx.set_render_mode(mode);
        cx.output_mut().extend(b"PREFIX").unwrap();
        WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let mut render = std::pin::pin!(proa_core::render_child(
                &mut cx,
                InheritedAsyncPage(calls.clone())
            ));
            let mut task = Context::from_waker(Waker::noop());
            let first = render.as_mut().poll(&mut task);
            if matches!(
                mode,
                proa_core::RenderMode::ChunkedInOrder | proa_core::RenderMode::ChunkedOutOfOrder
            ) {
                assert!(first.is_pending());
                assert!(doc.is_published());
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                assert!(matches!(
                    render.as_mut().poll(&mut task),
                    std::task::Poll::Ready(Ok(()))
                ));
            } else {
                assert!(matches!(first, std::task::Poll::Ready(Ok(()))));
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(&*wire.lock().unwrap(), b"PREFIX");
        assert_eq!(cx.output().to_vec(), b"BODY");
        doc.final_check().unwrap();
    }
}

struct SlowBeforePage;
impl WebRender for SlowBeforePage {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend(b"BODY_BEFORE_PAGE")?;
            std::future::pending::<()>().await;
            head_dispatch::sync(Page, cx)
        })
    }
}
#[test]
fn boundary_body_only_work_before_page_still_delays_managed_head() {
    let (mut cx, doc, wire) = setup(4096);
    cx.output_mut().extend(b"<head>").unwrap();
    WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
    let mut render = std::pin::pin!(proa_core::render_child(&mut cx, SlowBeforePage));
    assert!(render
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert!(!doc.is_published());
    assert!(wire.lock().unwrap().is_empty());
}
struct Overshoot;
impl WebRender for Overshoot {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend(&[b'x'; 4096])?;
            std::future::pending::<()>().await;
            Ok(())
        })
    }
}
#[test]
fn boundary_sampled_budget_can_be_exceeded_while_discovery_waits() {
    let (mut cx, doc, wire) = setup(32);
    WebRenderSync::render(doc.outlet(), &mut cx).unwrap();
    {
        let mut render = std::pin::pin!(proa_core::render_child(&mut cx, Overshoot));
        assert!(render
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    assert_eq!(cx.output().len(), 4096);
    assert!(!doc.is_published());
    assert!(wire.lock().unwrap().is_empty());
}
