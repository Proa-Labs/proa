// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Production in-order `Suspense` semantics for the same-context
//! `RenderBuffer` streaming path.

use std::cell::{Cell, RefCell};
use std::future::{pending, Future};
use std::rc::Rc;
use std::task::Poll;

use futures::future::poll_fn;
use futures::FutureExt;
use http::{HeaderName, HeaderValue, StatusCode};
use proa_core::{
    render_in_context_async, BoxedWebRender, DataLoader, ErrorBoundary, HeadEntry, NoLoader,
    RenderBuffer, RenderMode, RenderOutcome, StreamChunkFlusher, StreamChunkView, WebContext,
    WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_stream::Suspense;

const BEFORE_HEADER: HeaderName = HeaderName::from_static("x-before-boundary");
const LATE_HEADER: HeaderName = HeaderName::from_static("x-late-boundary");

#[derive(Default)]
struct CollectingSink {
    chunks: RefCell<Vec<Vec<u8>>>,
    flush_attempts: Cell<usize>,
    fail_at: Option<usize>,
}

impl CollectingSink {
    fn failing_at(flush: usize) -> Self {
        Self {
            fail_at: Some(flush),
            ..Self::default()
        }
    }

    fn record(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        if chunk.is_empty() {
            return Ok(());
        }
        let attempt = self.flush_attempts.get() + 1;
        self.flush_attempts.set(attempt);
        if self.fail_at == Some(attempt) {
            return Err(WriteError::StreamClosed);
        }
        self.chunks.borrow_mut().push(chunk);
        Ok(())
    }
}

impl StreamChunkFlusher for CollectingSink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.record(chunk)
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        let mut owned = Vec::with_capacity(chunk.total_len());
        chunk.for_each_slice(&mut |slice| {
            owned.extend_from_slice(slice);
            Ok(())
        })?;
        self.record(owned)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticSnapshot {
    result: Result<(), String>,
    chunks: Vec<Vec<u8>>,
    tail: Vec<u8>,
    flush_generation: u64,
    status: Option<StatusCode>,
    before_header: Option<String>,
    late_header: Option<String>,
    title: Option<String>,
    head_entries: Vec<HeadEntry>,
    islands: Vec<&'static str>,
    query_seed_keys: Vec<String>,
    shared_state_seed_table: Option<String>,
    fallback_stack_is_empty: bool,
    provider_stack_is_empty: bool,
}

impl SemanticSnapshot {
    fn combined_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for chunk in &self.chunks {
            bytes.extend_from_slice(chunk);
        }
        bytes.extend_from_slice(&self.tail);
        bytes
    }
}

fn capture<B: WriteBuf>(
    cx: &mut WebContext<NoLoader, B>,
    sink: &Rc<CollectingSink>,
    result: Result<(), WriteError>,
    provider_probe: (&'static str, &'static str, &'static str),
) -> SemanticSnapshot {
    let artifacts = cx.rsjs_artifacts_snapshot();
    let islands = artifacts.rendered_islands().collect::<Vec<_>>();
    let query_seed_keys = artifacts
        .query_seeds()
        .iter()
        .map(|seed| seed.key_json().to_owned())
        .collect::<Vec<_>>();
    let shared_state_seed_table = cx.rsjs_shared_state_seed_table_json();
    let provider_stack_is_empty = cx
        .rsjs_shared_state_acquire::<u32>(
            provider_probe.0,
            provider_probe.1,
            provider_probe.2,
            "PostRenderProbe",
        )
        .is_err();
    let headers = cx.response().headers();
    let before_header = headers
        .get(&BEFORE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let late_header = headers
        .get(&LATE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    SemanticSnapshot {
        result: result.map_err(|error| error.to_string()),
        chunks: sink.chunks.borrow().clone(),
        tail: cx.out.to_vec(),
        flush_generation: cx.response().stream_flush_generation(),
        status: cx.response().status(),
        before_header,
        late_header,
        title: cx.head().title(),
        head_entries: cx.head().entries(),
        islands,
        query_seed_keys,
        shared_state_seed_table,
        fallback_stack_is_empty: cx.current_boundary_error_fallback().is_none(),
        provider_stack_is_empty,
    }
}

fn in_order_context(
    out: RenderBuffer,
    sink: &Rc<CollectingSink>,
) -> WebContext<NoLoader, RenderBuffer> {
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    WebContext::with_buffer(out)
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher)
}

fn install_response_head<B: WriteBuf>(cx: &mut WebContext<NoLoader, B>) {
    assert!(cx.response_mut().set_status(StatusCode::ACCEPTED));
    assert!(cx
        .response_mut()
        .insert_header(BEFORE_HEADER.clone(), HeaderValue::from_static("before")));
}

async fn run_suspense<R>(
    root: R,
    sink: Rc<CollectingSink>,
    provider_probe: (&'static str, &'static str, &'static str),
) -> SemanticSnapshot
where
    R: WebRender<NoLoader, RenderBuffer>,
{
    let mut cx = in_order_context(RenderBuffer::new(), &sink);
    install_response_head(&mut cx);
    let result = render_in_context_async(&mut cx, root).await;
    capture(&mut cx, &sink, result, provider_probe)
}

#[derive(Clone, Copy)]
struct Marker {
    html: &'static [u8],
    island: Option<&'static str>,
}

impl Marker {
    const fn plain(html: &'static [u8]) -> Self {
        Self { html, island: None }
    }

    const fn island(html: &'static [u8], island: &'static str) -> Self {
        Self {
            html,
            island: Some(island),
        }
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Marker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(island) = self.island {
            cx.rsjs_island_used(island);
        }
        cx.out.extend_static(self.html)
    }
}

#[derive(Clone)]
struct FailingFallback {
    calls: Rc<Cell<usize>>,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for FailingFallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        self.calls.set(self.calls.get() + 1);
        cx.rsjs_island_used("failing-fallback-island");
        cx.out.extend_static(b"<broken-fallback>")?;
        Err(WriteError::render("fallback failed"))
    }
}

struct StatefulSuccess;

impl<L: DataLoader> WebRender<L, RenderBuffer> for StatefulSuccess {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            assert!(!cx.response_mut().set_status(StatusCode::CREATED));
            assert!(!cx
                .response_mut()
                .insert_header(LATE_HEADER.clone(), HeaderValue::from_static("late")));
            cx.head_mut().set_title("child title");
            cx.head_mut().push_meta("scenario", "success");
            let _scope = cx.rsjs_shared_state_enter(
                "test.success",
                "sha256:success",
                "SuccessState",
                &[] as &'static [&'static str],
                Rc::new(7u32),
                "{\"value\":7}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.success",
                "sha256:success",
                "SuccessState",
                "SuccessIsland",
            )?;
            cx.rsjs_island_used("success-island");
            cx.rsjs_query_seed_json(r#"["success"]"#, r#"{"value":7}"#);
            cx.out.extend_static(b"<success>")?;
            tokio::task::yield_now().await;
            cx.out.extend_static(b"</success>")
        })
    }
}

struct StatefulFailure;

impl<L: DataLoader> WebRender<L, RenderBuffer> for StatefulFailure {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.failure",
                "sha256:failure",
                "FailureState",
                &[] as &'static [&'static str],
                Rc::new(9u32),
                "{\"secret\":9}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.failure",
                "sha256:failure",
                "FailureState",
                "DoomedIsland",
            )?;
            cx.rsjs_island_used("doomed-island");
            cx.rsjs_query_seed_json(r#"["doomed"]"#, r#"{"secret":9}"#);
            cx.out.extend_static(b"<doomed>")?;
            tokio::task::yield_now().await;
            Err(WriteError::render("child failed"))
        })
    }
}

struct PlainFailure;

impl<L: DataLoader> WebRender<L, RenderBuffer> for PlainFailure {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<plain-doomed>")?;
            tokio::task::yield_now().await;
            Err(WriteError::render("plain child failed"))
        })
    }
}

struct StreamClosedFailure;

impl<L: DataLoader> WebRender<L, RenderBuffer> for StreamClosedFailure {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_island_used("uncatchable-doomed-island");
            cx.out.extend_static(b"<uncatchable-doomed>")?;
            tokio::task::yield_now().await;
            Err(WriteError::StreamClosed)
        })
    }
}

struct EarlyMetadataChild;

impl<L: DataLoader> WebRender<L, RenderBuffer> for EarlyMetadataChild {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            assert!(cx.response_mut().set_status(StatusCode::CREATED));
            assert!(cx.response_mut().insert_header(
                LATE_HEADER.clone(),
                HeaderValue::from_static("early-enough")
            ));
            cx.head_mut().set_title("early child title");
            cx.out.extend_static(b"<early-metadata>")
        })
    }
}

struct NestedPanel;

impl<L: DataLoader> WebRender<L, RenderBuffer> for NestedPanel {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.rsjs_shared_state_acquire::<u32>(
                "test.nested",
                "sha256:nested",
                "NestedState",
                "NestedIsland",
            )?;
            cx.rsjs_island_used("nested-island");
            cx.out.extend_static(b"<nested>")?;
            tokio::task::yield_now().await;
            cx.out.extend_static(b"</nested>")
        })
    }
}

struct NestedThenFail;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for NestedThenFail {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.nested",
                "sha256:nested",
                "NestedState",
                &[] as &'static [&'static str],
                Rc::new(11u32),
                "{\"value\":11}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.nested",
                "sha256:nested",
                "NestedState",
                "OuterShippedIsland",
            )?;
            cx.rsjs_island_used("outer-shipped-island");
            cx.out.extend_static(b"<outer-shipped>")?;
            proa_core::render_child(cx, Suspense::new(NestedPanel, Marker::plain(b"<loading>")))
                .await?;
            cx.rsjs_island_used("discarded-tail-island");
            cx.out.extend_static(b"<discarded-tail>")?;
            Err(WriteError::render("outer tail failed"))
        })
    }
}

struct SuccessRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for SuccessRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(
                cx,
                Suspense::new(StatefulSuccess, Marker::plain(b"<loading>")),
            )
            .await?;
            cx.out.extend_static(b"</shell>")
        })
    }
}

struct RecoveryRoot<F> {
    fallback: F,
}

impl<L, F> WebRender<L, RenderBuffer> for RecoveryRoot<F>
where
    L: DataLoader + 'static,
    F: Clone + WebRenderSync<L, RenderBuffer> + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(
                cx,
                ErrorBoundary::new(
                    Suspense::new(StatefulFailure, Marker::plain(b"<loading>")),
                    self.fallback,
                ),
            )
            .await?;
            cx.out.extend_static(b"</shell>")
        })
    }
}

struct NestedRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for NestedRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(
                cx,
                ErrorBoundary::new(
                    NestedThenFail,
                    Marker::island(b"<outer-fallback>", "outer-fallback-island"),
                ),
            )
            .await?;
            cx.out.extend_static(b"</shell>")
        })
    }
}

struct NearestFallbackRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for NearestFallbackRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            proa_core::render_child(
                cx,
                ErrorBoundary::new(
                    ErrorBoundary::new(
                        Suspense::new(PlainFailure, Marker::plain(b"<loading>")),
                        Marker::plain(b"<inner-fallback>"),
                    ),
                    Marker::plain(b"<outer-fallback>"),
                ),
            )
            .await
        })
    }
}

struct PendingChild {
    started: Rc<Cell<bool>>,
}

struct PanickingChild;

impl<L: DataLoader> WebRender<L, RenderBuffer> for PanickingChild {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.unwind",
                "sha256:unwind",
                "UnwindState",
                &[] as &'static [&'static str],
                Rc::new(17u32),
                "{\"value\":17}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.unwind",
                "sha256:unwind",
                "UnwindState",
                "UnwoundIsland",
            )?;
            cx.rsjs_island_used("unwound-island");
            cx.out.extend_static(b"<unwound-partial>")?;
            tokio::task::yield_now().await;
            panic!("injected render panic")
        })
    }
}

impl<L: DataLoader> WebRender<L, RenderBuffer> for PendingChild {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.cancel",
                "sha256:cancel",
                "CancelState",
                &[] as &'static [&'static str],
                Rc::new(13u32),
                "{\"value\":13}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.cancel",
                "sha256:cancel",
                "CancelState",
                "CancelledIsland",
            )?;
            cx.rsjs_island_used("cancelled-island");
            cx.rsjs_query_seed_json(r#"["cancelled"]"#, r#"{"value":13}"#);
            cx.out.extend_static(b"<cancelled-partial>")?;
            self.started.set(true);
            pending::<()>().await;
            Ok(())
        })
    }
}

struct CancellationRoot {
    started: Rc<Cell<bool>>,
}

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for CancellationRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(
                cx,
                ErrorBoundary::new(
                    Suspense::new(
                        PendingChild {
                            started: self.started,
                        },
                        Marker::plain(b"<loading>"),
                    ),
                    Marker::plain(b"<cancel-fallback>"),
                ),
            )
            .await
        })
    }
}

struct UnwindRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for UnwindRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(
                cx,
                ErrorBoundary::new(
                    Suspense::new(PanickingChild, Marker::plain(b"<loading>")),
                    Marker::plain(b"<panic-fallback>"),
                ),
            )
            .await
        })
    }
}

struct EarlyMetadataRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for EarlyMetadataRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            proa_core::render_child(
                cx,
                Suspense::new(EarlyMetadataChild, Marker::plain(b"<loading>")),
            )
            .await?;
            cx.out.extend_static(b"<tail>")
        })
    }
}

struct UnhandledRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for UnhandledRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(cx, Suspense::new(PlainFailure, Marker::plain(b"<loading>")))
                .await
        })
    }
}

struct UncatchableRoot;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for UncatchableRoot {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"<shell>")?;
            proa_core::render_child(
                cx,
                ErrorBoundary::new(
                    Suspense::new(StreamClosedFailure, Marker::plain(b"<loading>")),
                    Marker::plain(b"<must-not-recover>"),
                ),
            )
            .await
        })
    }
}

async fn poll_once_pending<F>(future: &mut std::pin::Pin<Box<F>>)
where
    F: Future<Output = Result<(), WriteError>>,
{
    poll_fn(|task_cx| match future.as_mut().poll(task_cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(result) => panic!("cancellation fixture unexpectedly completed: {result:?}"),
    })
    .await;
}

#[tokio::test]
async fn successful_child_flushes_state_and_markup() {
    let current = run_suspense(
        SuccessRoot,
        Rc::new(CollectingSink::default()),
        ("test.success", "sha256:success", "SuccessState"),
    )
    .await;
    assert_eq!(
        current.combined_bytes(),
        b"<shell><success></success></shell>"
    );
    assert_eq!(current.status, Some(StatusCode::ACCEPTED));
    assert_eq!(current.before_header.as_deref(), Some("before"));
    assert_eq!(current.late_header, None);
    assert!(current.provider_stack_is_empty);
    assert!(current.fallback_stack_is_empty);
    assert!(current
        .shared_state_seed_table
        .as_deref()
        .is_some_and(|table| table.contains("test.success")));
}

#[tokio::test]
async fn boxed_dynamic_roots_use_the_standard_render_buffer() {
    let current = run_suspense(
        BoxedWebRender::<NoLoader, RenderBuffer>::new(SuccessRoot),
        Rc::new(CollectingSink::default()),
        ("test.success", "sha256:success", "SuccessState"),
    )
    .await;
    assert_eq!(
        current.combined_bytes(),
        b"<shell><success></success></shell>"
    );
    assert!(current.result.is_ok());
}

#[tokio::test]
async fn response_metadata_is_mutable_before_the_first_flush() {
    let current = run_suspense(
        EarlyMetadataRoot,
        Rc::new(CollectingSink::default()),
        ("unused", "sha256:unused", "Unused"),
    )
    .await;
    assert_eq!(current.status, Some(StatusCode::CREATED));
    assert_eq!(current.late_header.as_deref(), Some("early-enough"));
    assert_eq!(current.title.as_deref(), Some("early child title"));
    assert_eq!(current.combined_bytes(), b"<early-metadata><tail>");
}

#[tokio::test]
async fn failed_child_renders_fallback_and_rolls_back_state() {
    let fallback = Marker::island(b"<recovered>", "recovery-island");
    let current = run_suspense(
        RecoveryRoot { fallback },
        Rc::new(CollectingSink::default()),
        ("test.failure", "sha256:failure", "FailureState"),
    )
    .await;
    assert_eq!(current.combined_bytes(), b"<shell><recovered></shell>");
    assert!(!current.islands.contains(&"doomed-island"));
    assert!(current.islands.contains(&"recovery-island"));
    assert!(current.shared_state_seed_table.is_none());
}

#[tokio::test]
async fn failing_fallback_propagates_once() {
    let calls = Rc::new(Cell::new(0));
    let current = run_suspense(
        RecoveryRoot {
            fallback: FailingFallback {
                calls: Rc::clone(&calls),
            },
        },
        Rc::new(CollectingSink::default()),
        ("test.failure", "sha256:failure", "FailureState"),
    )
    .await;
    assert_eq!(calls.get(), 1);
    assert!(current.result.is_err());
}

#[tokio::test]
async fn nested_partial_flush_preserves_shipped_state_and_discards_the_tail() {
    let current = run_suspense(
        NestedRoot,
        Rc::new(CollectingSink::default()),
        ("test.nested", "sha256:nested", "NestedState"),
    )
    .await;
    let bytes = current.combined_bytes();
    assert!(bytes
        .windows(b"<outer-shipped>".len())
        .any(|w| w == b"<outer-shipped>"));
    assert!(bytes
        .windows(b"<nested></nested>".len())
        .any(|w| w == b"<nested></nested>"));
    assert!(!bytes
        .windows(b"<discarded-tail>".len())
        .any(|w| w == b"<discarded-tail>"));
    assert!(bytes
        .windows(b"<outer-fallback>".len())
        .any(|w| w == b"<outer-fallback>"));
    assert!(current.provider_stack_is_empty);
    assert!(current
        .shared_state_seed_table
        .as_deref()
        .is_some_and(|table| table.contains("test.nested")));
}

#[tokio::test]
async fn nested_error_uses_the_nearest_fallback() {
    let current = run_suspense(
        NearestFallbackRoot,
        Rc::new(CollectingSink::default()),
        ("unused", "sha256:unused", "Unused"),
    )
    .await;
    assert_eq!(current.combined_bytes(), b"<inner-fallback>");
    assert!(current.fallback_stack_is_empty);
}

#[tokio::test]
async fn unhandled_catchable_error_discards_child_bytes() {
    let current = run_suspense(
        UnhandledRoot,
        Rc::new(CollectingSink::default()),
        ("unused", "sha256:unused", "Unused"),
    )
    .await;
    assert!(current.result.is_err());
    assert_eq!(current.combined_bytes(), b"<shell>");
}

#[tokio::test]
async fn uncatchable_error_bypasses_fallback() {
    let current = run_suspense(
        UncatchableRoot,
        Rc::new(CollectingSink::default()),
        ("unused", "sha256:unused", "Unused"),
    )
    .await;
    assert_eq!(current.result, Err("stream closed".to_owned()));
    assert_eq!(current.combined_bytes(), b"<shell>");
    assert!(!current
        .combined_bytes()
        .windows(b"<must-not-recover>".len())
        .any(|window| window == b"<must-not-recover>"));
}

#[tokio::test]
async fn stream_flush_failures_propagate() {
    for fail_at in [1, 2] {
        let current = run_suspense(
            SuccessRoot,
            Rc::new(CollectingSink::failing_at(fail_at)),
            ("test.success", "sha256:success", "SuccessState"),
        )
        .await;
        assert!(current.result.is_err(), "flush attempt {fail_at}");
    }
}

#[tokio::test]
async fn cancellation_restores_output_providers_artifacts_and_fallback_stack() {
    let current_sink = Rc::new(CollectingSink::default());
    let current_started = Rc::new(Cell::new(false));
    let mut current_cx = in_order_context(RenderBuffer::new(), &current_sink);
    install_response_head(&mut current_cx);
    {
        let current_outcome = WebRender::render(
            CancellationRoot {
                started: Rc::clone(&current_started),
            },
            &mut current_cx,
        );
        match current_outcome {
            RenderOutcome::Pending(current_future) => {
                let mut current_future = Box::pin(current_future);
                poll_once_pending(&mut current_future).await;
                assert!(current_started.get());
                drop(current_future);
            }
            RenderOutcome::Done(_) => panic!("current cancellation fixture must be pending"),
        }
    }
    let current = capture(
        &mut current_cx,
        &current_sink,
        Ok(()),
        ("test.cancel", "sha256:cancel", "CancelState"),
    );

    assert_eq!(current.combined_bytes(), b"<shell>");
    assert!(current.provider_stack_is_empty);
    assert!(current.fallback_stack_is_empty);
    assert!(current.islands.is_empty());
    assert!(current.query_seed_keys.is_empty());
    assert!(current.shared_state_seed_table.is_none());
}

#[tokio::test]
async fn unwind_restores_output_providers_artifacts_and_fallback_stack() {
    let current_sink = Rc::new(CollectingSink::default());
    let mut current_cx = in_order_context(RenderBuffer::new(), &current_sink);
    install_response_head(&mut current_cx);
    let current_panic =
        std::panic::AssertUnwindSafe(WebRender::render(UnwindRoot, &mut current_cx).resolve())
            .catch_unwind()
            .await;
    assert!(current_panic.is_err());
    let current = capture(
        &mut current_cx,
        &current_sink,
        Ok(()),
        ("test.unwind", "sha256:unwind", "UnwindState"),
    );

    assert_eq!(current.combined_bytes(), b"<shell>");
    assert!(current.provider_stack_is_empty);
    assert!(current.fallback_stack_is_empty);
    assert!(current.islands.is_empty());
    assert!(current.shared_state_seed_table.is_none());
}
