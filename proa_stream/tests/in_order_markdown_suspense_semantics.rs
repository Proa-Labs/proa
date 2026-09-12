// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Production in-order Markdown `Suspense` streaming semantics.

use std::cell::{Cell, RefCell};
use std::future::{pending, Future};
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::task::Poll;

use futures::future::poll_fn;
use futures::FutureExt;
use proa_core::md::{MdContext, MdContextState};
use proa_core::{
    render_md_in_context_async, MdRender, RenderBuffer, RenderMode, RenderOutcome,
    StreamChunkFlusher, StreamChunkView, WriteBuf, WriteError,
};
use proa_stream::Suspense;

#[derive(Default)]
struct CollectingSink {
    chunks: RefCell<Vec<Vec<u8>>>,
    attempts: Cell<usize>,
    fail_at: Option<usize>,
}

impl CollectingSink {
    fn failing_at(attempt: usize) -> Self {
        Self {
            fail_at: Some(attempt),
            ..Self::default()
        }
    }

    fn record(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        if chunk.is_empty() {
            return Ok(());
        }
        let attempt = self.attempts.get() + 1;
        self.attempts.set(attempt);
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
struct Snapshot {
    result: Result<(), String>,
    chunks: Vec<Vec<u8>>,
    tail: Vec<u8>,
    state: MdContextState,
    flush_attempts: usize,
}

fn snapshot<B: WriteBuf>(
    cx: &MdContext<'_, B>,
    sink: &CollectingSink,
    result: Result<(), WriteError>,
) -> Snapshot {
    Snapshot {
        result: result.map_err(|error| error.to_string()),
        chunks: sink.chunks.borrow().clone(),
        tail: cx.out.to_vec(),
        state: cx.render_state(),
        flush_attempts: sink.attempts.get(),
    }
}

fn state_after_shell() -> MdContextState {
    let mut out = RenderBuffer::new();
    let mut cx = MdContext::with_buffer(&mut out);
    cx.push_heading_offset(1);
    cx.write_prepared_static(b"shell\n")
        .expect("state fixture should render");
    cx.render_state()
}

async fn run_suspense<R>(root: R, sink: Rc<CollectingSink>) -> Snapshot
where
    R: MdRender<RenderBuffer>,
{
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    let mut out = RenderBuffer::new();
    let mut cx = MdContext::with_buffer(&mut out)
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);
    cx.push_heading_offset(1);
    let result = match render_md_in_context_async(&mut cx, root).await {
        Ok(()) => cx.flush_stream_chunk(),
        Err(error) => Err(error),
    };
    snapshot(&cx, &sink, result)
}

struct SuccessfulChild;

impl<B: WriteBuf> MdRender<B> for SuccessfulChild {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.push_heading_offset(1);
            cx.heading(1, "child")?;
            tokio::task::yield_now().await;
            cx.write_prepared_static(b"body\n")
        })
    }
}

struct SuccessRoot;

impl MdRender<RenderBuffer> for SuccessRoot {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"before\n")?;
            proa_core::render_md_child(cx, Suspense::new(SuccessfulChild, ())).await?;
            cx.blank_line()?;
            cx.write_prepared_static(b"after\n")
        })
    }
}

struct InnerChild;

impl<B: WriteBuf> MdRender<B> for InnerChild {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"inner\n")?;
            tokio::task::yield_now().await;
            cx.push_heading_offset(2);
            Ok(())
        })
    }
}

struct NestedThenFail;

impl MdRender<RenderBuffer> for NestedThenFail {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"outer\n")?;
            proa_core::render_md_child(cx, Suspense::new(InnerChild, ())).await?;
            cx.write_prepared_static(b"discarded\n")?;
            cx.push_heading_offset(3);
            Err(WriteError::render("outer failed"))
        })
    }
}

struct NestedFailureRoot;

impl MdRender<RenderBuffer> for NestedFailureRoot {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"shell\n")?;
            proa_core::render_md_child(cx, Suspense::new(NestedThenFail, ())).await
        })
    }
}

struct FailingChild;

impl<B: WriteBuf> MdRender<B> for FailingChild {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.push_heading_offset(4);
            cx.write_prepared_static(b"partial\n")?;
            tokio::task::yield_now().await;
            Err(WriteError::render("child failed"))
        })
    }
}

struct FailureRoot;

impl MdRender<RenderBuffer> for FailureRoot {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"shell\n")?;
            proa_core::render_md_child(cx, Suspense::new(FailingChild, ())).await
        })
    }
}

struct PendingChild {
    started: Rc<Cell<bool>>,
}

impl<B: WriteBuf> MdRender<B> for PendingChild {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.push_heading_offset(5);
            cx.write_prepared_static(b"cancelled\n")?;
            self.started.set(true);
            pending::<()>().await;
            Ok(())
        })
    }
}

struct PendingRoot {
    started: Rc<Cell<bool>>,
}

impl MdRender<RenderBuffer> for PendingRoot {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"shell\n")?;
            proa_core::render_md_child(
                cx,
                Suspense::new(
                    PendingChild {
                        started: self.started,
                    },
                    (),
                ),
            )
            .await
        })
    }
}

struct PanickingChild;

impl<B: WriteBuf> MdRender<B> for PanickingChild {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.push_heading_offset(5);
            cx.write_prepared_static(b"panicking\n")?;
            tokio::task::yield_now().await;
            panic!("injected markdown panic")
        })
    }
}

struct PanickingRoot;

impl MdRender<RenderBuffer> for PanickingRoot {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            cx.write_prepared_static(b"shell\n")?;
            proa_core::render_md_child(cx, Suspense::new(PanickingChild, ())).await
        })
    }
}

async fn cancel_suspense(sink: Rc<CollectingSink>) -> Snapshot {
    let started = Rc::new(Cell::new(false));
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    let mut out = RenderBuffer::new();
    let mut cx = MdContext::with_buffer(&mut out)
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);
    cx.push_heading_offset(1);
    let mut future = Box::pin(render_md_in_context_async(
        &mut cx,
        PendingRoot {
            started: Rc::clone(&started),
        },
    ));
    poll_once_pending(&mut future).await;
    assert!(started.get());
    drop(future);
    snapshot(&cx, &sink, Ok(()))
}

async fn unwind_suspense(sink: Rc<CollectingSink>) -> Snapshot {
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    let mut out = RenderBuffer::new();
    let mut cx = MdContext::with_buffer(&mut out)
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);
    cx.push_heading_offset(1);
    let panic = AssertUnwindSafe(render_md_in_context_async(&mut cx, PanickingRoot))
        .catch_unwind()
        .await;
    assert!(panic.is_err());
    snapshot(&cx, &sink, Ok(()))
}

async fn poll_once_pending<F>(future: &mut std::pin::Pin<Box<F>>)
where
    F: Future<Output = Result<(), WriteError>>,
{
    poll_fn(|task_cx| match future.as_mut().poll(task_cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(result) => panic!("cancellation fixture completed unexpectedly: {result:?}"),
    })
    .await;
}

#[tokio::test]
async fn success_flushes_each_boundary_segment() {
    let snapshot = run_suspense(SuccessRoot, Rc::new(CollectingSink::default())).await;

    assert_eq!(snapshot.result, Ok(()));
    assert_eq!(
        snapshot.chunks,
        [
            b"before\n".to_vec(),
            b"### child\n\nbody\n".to_vec(),
            b"\nafter\n".to_vec(),
        ]
    );
    assert!(snapshot.tail.is_empty());
    assert_eq!(snapshot.flush_attempts, 3);
}

#[tokio::test]
async fn nested_partial_flush_survives_outer_error() {
    let snapshot = run_suspense(NestedFailureRoot, Rc::new(CollectingSink::default())).await;

    assert!(snapshot.result.is_err());
    assert_eq!(
        snapshot.chunks,
        [
            b"shell\n".to_vec(),
            b"outer\n".to_vec(),
            b"inner\n".to_vec()
        ]
    );
    assert!(snapshot.tail.is_empty());
    assert_eq!(snapshot.flush_attempts, 3);
    assert_eq!(snapshot.state, state_after_shell());
}

#[tokio::test]
async fn child_error_discards_unflushed_bytes() {
    let snapshot = run_suspense(FailureRoot, Rc::new(CollectingSink::default())).await;

    assert!(snapshot.result.is_err());
    assert_eq!(snapshot.chunks, [b"shell\n".to_vec()]);
    assert!(snapshot.tail.is_empty());
    assert_eq!(snapshot.flush_attempts, 1);
    assert_eq!(snapshot.state, state_after_shell());
}

#[tokio::test]
async fn transport_failures_propagate_at_each_logical_flush() {
    for attempt in 1..=3 {
        let snapshot =
            run_suspense(SuccessRoot, Rc::new(CollectingSink::failing_at(attempt))).await;
        assert!(snapshot.result.is_err(), "flush attempt {attempt}");
        assert_eq!(snapshot.flush_attempts, attempt, "flush attempt {attempt}");
    }
}

#[tokio::test]
async fn cancellation_restores_markdown_output_and_state() {
    let snapshot = cancel_suspense(Rc::new(CollectingSink::default())).await;

    assert_eq!(snapshot.result, Ok(()));
    assert_eq!(snapshot.chunks, [b"shell\n".to_vec()]);
    assert!(snapshot.tail.is_empty());
    assert_eq!(snapshot.flush_attempts, 1);
    assert_eq!(snapshot.state, state_after_shell());
}

#[tokio::test]
async fn unwind_restores_markdown_output_and_state() {
    let snapshot = unwind_suspense(Rc::new(CollectingSink::default())).await;

    assert_eq!(snapshot.result, Ok(()));
    assert_eq!(snapshot.chunks, [b"shell\n".to_vec()]);
    assert!(snapshot.tail.is_empty());
    assert_eq!(snapshot.flush_attempts, 1);
    assert_eq!(snapshot.state, state_after_shell());
}
