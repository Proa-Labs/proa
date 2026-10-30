use futures::executor::block_on;
use proa_core::{
    head_dispatch::{self, ComponentRole, HeadOperation},
    managed_head::{HeadError, ManagedHead},
    *,
};
use proa_macros::html;
use proa_stream::Suspense;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Waker},
};

type Output = Arc<Mutex<Vec<bytes::Bytes>>>;
fn setup() -> (WebContext, ManagedHead, Output) {
    let output: Output = Arc::new(Mutex::new(Vec::new()));
    let out = output.clone();
    let d = ManagedHead::new(
        1024 * 1024,
        Arc::new(move |_, bytes| {
            out.lock().unwrap().extend(bytes);
            Ok(())
        }),
    );
    let mut cx = WebContext::new();
    cx.head_mut().manage(d.clone());
    (cx, d, output)
}
fn wire(out: &Output) -> String {
    String::from_utf8(
        out.lock()
            .unwrap()
            .iter()
            .flat_map(|b| b.iter().copied())
            .collect(),
    )
    .unwrap()
}
fn err(e: WriteError) -> HeadError {
    if let WriteError::HeadLifecycle(e) = e {
        e
    } else if let WriteError::HeadFailure(failure) = e {
        failure.kind
    } else {
        panic!("unexpected: {e:?}")
    }
}
struct PageWithTitle {
    title: &'static str,
    wait: bool,
}
impl WebRender for PageWithTitle {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext) -> impl HeadOperation {
        async move {
            cx.head_mut().title(self.title)?;
            Ok(())
        }
    }
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            if self.wait {
                std::future::pending::<()>().await;
            }
            cx.output_mut().extend(b"PAGE_BODY")
        })
    }
}
struct Document<C>(C);
impl<C: WebRender + Send + 'static> WebRender for Document<C> {
    fn render_head(&self, cx: &mut WebContext) -> impl HeadOperation {
        async move {
            cx.head_mut().title("Parent default")?;
            Ok(())
        }
    }
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let head = cx.head();
        html! {
            <!DOCTYPE html>
            <html>
                <head>{head}</head>
                <body>{self.0}</body>
            </html>
        }
        .render(cx)
    }
}
#[test]
fn page_overrides_parent_and_splices_before_pending_body() {
    let (mut cx, d, out) = setup();
    let mut fut = std::pin::pin!(render_child(
        &mut cx,
        Document(PageWithTitle {
            title: "a</title>&\"é",
            wait: true
        })
    ));
    assert!(fut
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert!(d.is_published());
    assert_eq!(wire(&out),"<!DOCTYPE html><!-- Rendered with Proa --><html><head><title>a&lt;/title&gt;&amp;\"é</title></head><body>");
}
#[test]
fn page_before_outlet_is_valid_and_stored_handle_uses_final_values() {
    let (mut cx, d, out) = setup();
    let head = d.outlet();
    block_on(render_child(
        &mut cx,
        PageWithTitle {
            title: "final",
            wait: false,
        },
    ))
    .unwrap();
    assert!(!d.is_published());
    cx.output_mut().extend(b"<head>").unwrap();
    WebRenderSync::render(head, &mut cx).unwrap();
    d.final_check().unwrap();
    assert_eq!(wire(&out), "PAGE_BODY<head><title>final</title>");
}
#[test]
fn body_mutation_is_sticky_even_if_error_is_ignored() {
    let (mut cx, d, _) = setup();
    let _ = cx.head_mut().title("not in hook");
    assert_eq!(err(d.final_check().unwrap_err()), HeadError::OutsideHook);
    assert!(block_on(render_child(
        &mut cx,
        PageWithTitle {
            title: "page",
            wait: false
        }
    ))
    .is_err());
}
#[test]
fn duplicate_foreign_and_missing_outlets_are_observable() {
    let (mut cx, d, _) = setup();
    WebRenderSync::render(d.outlet(), &mut cx).unwrap();
    assert_eq!(
        err(WebRenderSync::render(d.outlet(), &mut cx).unwrap_err()),
        HeadError::DuplicateOutlet
    );
    let (mut cx, d, _) = setup();
    let (_, foreign, _) = setup();
    assert_eq!(
        err(WebRenderSync::render(foreign.outlet(), &mut cx).unwrap_err()),
        HeadError::ForeignOutlet
    );
    assert!(d.final_check().is_err());
    let (mut cx, d, _) = setup();
    block_on(render_child(
        &mut cx,
        PageWithTitle {
            title: "page",
            wait: false,
        },
    ))
    .unwrap();
    assert_eq!(err(d.final_check().unwrap_err()), HeadError::MissingOutlet);
}
#[test]
fn page_siblings_and_postpage_hooks_fail_without_reopening() {
    let (mut cx, d, _) = setup();
    block_on(render_child(
        &mut cx,
        Document(PageWithTitle {
            title: "page",
            wait: false,
        }),
    ))
    .unwrap();
    assert_eq!(
        err(block_on(render_child(
            &mut cx,
            PageWithTitle {
                title: "second",
                wait: false
            }
        ))
        .unwrap_err()),
        HeadError::Closed
    );
    assert!(d.is_published());
    assert!(d.final_check().is_err());
}
#[test]
fn provider_without_selected_page_is_not_a_valid_layout() {
    let (mut cx, d, _) = setup();
    assert_eq!(
        err(block_on(render_child(&mut cx, Document(()))).unwrap_err()),
        HeadError::OrphanProvider
    );
    assert!(d.final_check().is_err());
}
struct Cancelled;
impl WebRender for Cancelled {
    fn render_head(&self, cx: &mut WebContext) -> impl HeadOperation {
        async move {
            cx.head_mut().title("provisional")?;
            std::future::pending::<()>().await;
            Ok(())
        }
    }
    fn render(
        self,
        _cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}
#[test]
fn cancelled_hook_closes_permission_and_poison_does_not_leak_to_next_request() {
    let (mut cx, d, _) = setup();
    {
        let mut f = std::pin::pin!(render_child(&mut cx, Cancelled));
        assert!(f
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    assert_eq!(err(d.final_check().unwrap_err()), HeadError::Cancelled);
    assert_eq!(
        err(cx.head_mut().title("after cancel").unwrap_err()),
        HeadError::Cancelled
    );
    let (mut fresh, d, _) = setup();
    block_on(render_child(
        &mut fresh,
        Document(PageWithTitle {
            title: "fresh",
            wait: false,
        }),
    ))
    .unwrap();
    d.final_check().unwrap();
}
struct CountHook(Arc<AtomicUsize>);
impl WebRender for CountHook {
    fn render_head(&self, _cx: &mut WebContext) -> impl HeadOperation {
        self.0.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    }
    fn render(
        self,
        _cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}
#[test]
fn actual_inline_suspense_excludes_ready_hooks_in_every_delivery_mode() {
    for mode in [
        RenderMode::Static,
        RenderMode::ChunkedInOrder,
        RenderMode::ChunkedOutOfOrder,
    ] {
        let (mut cx, d, _) = setup();
        cx.set_render_mode(mode);
        let calls = Arc::new(AtomicUsize::new(0));
        let e = block_on(render_child(
            &mut cx,
            Suspense::new(CountHook(calls.clone()), ()),
        ))
        .unwrap_err();
        assert_eq!(err(e), HeadError::Excluded);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(d.final_check().is_err());
    }
}
#[test]
fn detached_context_stays_excluded_after_parent_scope_exits() {
    let (mut parent, d, _) = setup();
    let parts = parent.context_parts();
    let mut child = WebContext::with_loader_buffer_and_context_parts(
        RenderBuffer::new(),
        Arc::new(NoLoader),
        parts,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        err(block_on(render_child(&mut child, CountHook(calls.clone()))).unwrap_err()),
        HeadError::Excluded
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(d.final_check().is_err());
}
#[test]
fn lifecycle_error_cannot_be_caught_by_actual_error_boundary() {
    let (mut cx, d, _) = setup();
    let child = Suspense::new(CountHook(Arc::new(AtomicUsize::new(0))), ());
    let result = block_on(render_child(&mut cx, ErrorBoundary::new(child, "fallback")));
    let e = result.unwrap_err();
    assert!(!e.is_boundary_catchable());
    assert!(d.final_check().is_err());
    assert!(cx.output().is_empty());
}
struct EmptyPage;
impl WebRenderSync for EmptyPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, _cx: &mut WebContext) -> Result<(), WriteError> {
        Ok(())
    }
}
#[test]
fn page_without_hook_seals_and_publishes_in_sync_dispatch() {
    let (mut cx, d, _) = setup();
    cx.output_mut().extend(b"<head>").unwrap();
    WebRenderSync::render(d.outlet(), &mut cx).unwrap();
    head_dispatch::sync(EmptyPage, &mut cx).unwrap();
    d.final_check().unwrap();
    assert!(d.is_published());
}
#[test]
fn custom_writer_must_explicitly_support_owned_pages() {
    let (_, d, _) = setup();
    let mut cx = WebContext::with_buffer(Vec::<u8>::new());
    cx.head_mut().manage(d.clone());
    WebRenderSync::render(d.outlet(), &mut cx).unwrap();
    assert_eq!(
        err(cx.managed_seal().unwrap_err()),
        HeadError::UnsupportedWriter
    );
    assert!(d.final_check().is_err());
}
#[test]
fn retained_bytes_limit_fails_instead_of_waiting_for_itself() {
    let d = ManagedHead::new(3, Arc::new(|_, _| panic!("must not publish")));
    let mut cx = WebContext::new();
    cx.head_mut().manage(d.clone());
    cx.output_mut().extend(b"four").unwrap();
    assert_eq!(
        err(WebRenderSync::render(d.outlet(), &mut cx).unwrap_err()),
        HeadError::RetainedLimit
    );
    assert!(d.final_check().is_err());
}

struct BodyError;
impl WebRender for BodyError {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.output_mut().extend(b"discard")?;
            Err(WriteError::render("body error"))
        })
    }
}
#[test]
fn body_recovery_after_seal_keeps_metadata_and_rewinds_only_body() {
    let (mut cx, d, out) = setup();
    block_on(render_child(
        &mut cx,
        Document(PageWithTitle {
            title: "kept",
            wait: false,
        }),
    ))
    .unwrap();
    cx.output_mut().clear();
    block_on(render_child(
        &mut cx,
        ErrorBoundary::new(BodyError, "fallback"),
    ))
    .unwrap();
    assert_eq!(cx.into_output().into_string(), "fallback");
    assert!(wire(&out).contains("<title>kept</title>"));
    d.final_check().unwrap();
}
#[test]
fn recovery_cannot_rewind_over_a_reserved_head_outlet() {
    struct ReserveThenFail;
    impl WebRender for ReserveThenFail {
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                let head = cx.head();
                WebRenderSync::render(head, cx)?;
                Err(WriteError::render("after outlet"))
            })
        }
    }
    let (mut cx, d, _) = setup();
    assert_eq!(
        err(block_on(render_child(
            &mut cx,
            ErrorBoundary::new(ReserveThenFail, "fallback")
        ))
        .unwrap_err()),
        HeadError::RecoveryCrossedLifecycle
    );
    assert!(d.final_check().is_err());
}
#[test]
fn chained_pages_are_transferred_and_invalid_utf8_is_rejected() {
    let (mut cx, d, out) = setup();
    let content = "é".repeat(3000);
    cx.output_mut().extend(content.as_bytes()).unwrap();
    let original = cx
        .output()
        .slices()
        .find(|p| !p.is_empty())
        .unwrap()
        .as_ptr();
    WebRenderSync::render(d.outlet(), &mut cx).unwrap();
    head_dispatch::sync(EmptyPage, &mut cx).unwrap();
    assert_eq!(out.lock().unwrap()[0].as_ptr(), original);
    assert_eq!(wire(&out), content);
    let (mut cx, d, out) = setup();
    cx.output_mut().extend(&[255]).unwrap();
    WebRenderSync::render(d.outlet(), &mut cx).unwrap();
    assert_eq!(
        err(head_dispatch::sync(EmptyPage, &mut cx).unwrap_err()),
        HeadError::InvalidOutput
    );
    assert!(out.lock().unwrap().is_empty());
}
