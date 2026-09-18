// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{
    DataLoader, LoadError, LoadKey, LoadRequest, LoadValue, MdRender, MdRenderSync, NoLoader,
    OptimizedMd, RenderBuffer, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteBuf,
    WriteError,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context as TaskContext, Poll, Waker},
};

struct Label<'a>(&'a str);
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Label<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.write_md_text(self.0)
    }
}
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Label<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(proa_core::text::text(self.0), cx)
    }
}
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Label<'_> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(WebRenderSync::render(
            self, cx,
        ))
    }
}
struct YieldOnce(bool);
impl Future for YieldOnce {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}
impl<L: DataLoader, B: WriteBuf> MdRender<L, B> for Label<'_> {
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            YieldOnce(false).await;
            MdRenderSync::render_md(self, cx)
        })
    }
}
struct TestLoader {
    value: Arc<String>,
    calls: AtomicUsize,
}
impl DataLoader for TestLoader {
    fn load(&self, _: &LoadRequest) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        self.calls.fetch_add(1, Ordering::Relaxed);
        std::future::ready(Ok(LoadValue::arc(Arc::clone(&self.value))))
    }
}
struct LoadedLabel<'a>(&'a str);
impl<L: DataLoader, B: WriteBuf> MdRender<L, B> for LoadedLabel<'_> {
    fn render_md(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            let value = cx
                .load_typed::<String>(LoadKey::named("probe", self.0))
                .await?;
            MdRenderSync::render_md(Label(value.as_str()), cx)
        })
    }
}
fn complete<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = TaskContext::from_waker(Waker::noop());
    for _ in 0..4 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
    panic!("probe future did not complete");
}
#[test]
fn all_four_traits_use_the_same_context_and_keep_their_escaping() {
    let input = String::from("*a*<b>");
    let mut cx = WebContext::new();
    WebRenderSync::render(Label(&input), &mut cx).unwrap();
    MdRenderSync::render_md(Label(&input), &mut cx).unwrap();
    complete(WebRender::render(Label(&input), &mut cx).resolve()).unwrap();
    complete(MdRender::render_md(Label(&input), &mut cx).resolve()).unwrap();
    let pair = "*a*&lt;b&gt;\\*a\\*\\<b\\>";
    assert_eq!(cx.into_output().into_string(), pair.repeat(2));
}
#[test]
fn context_generics_put_the_loader_before_the_buffer() {
    let mut cx: WebContext<NoLoader, Vec<u8>> = WebContext::with_buffer(Vec::new());
    let same: &mut WebContext<NoLoader, Vec<u8>> = &mut cx;
    MdRenderSync::render_md(Label("loader first"), same).unwrap();
    assert_eq!(cx.into_output(), b"loader first");
}
#[test]
fn async_markdown_can_use_the_existing_typed_loader() {
    let loader = Arc::new(TestLoader {
        value: Arc::new(String::from("*loaded*")),
        calls: AtomicUsize::new(0),
    });
    let mut cx = WebContext::with_loader(Arc::clone(&loader));
    let key = String::from("borrowed key");
    complete(MdRender::render_md(LoadedLabel(&key), &mut cx).resolve()).unwrap();
    assert!(Arc::ptr_eq(cx.loader(), &loader));
    assert_eq!(loader.calls.load(Ordering::Relaxed), 1);
    assert_eq!(cx.into_output().into_string(), "\\*loaded\\*");
}
#[test]
fn markdown_fragment_web_bridge_keeps_context_loader_and_buffer_identity() {
    let loader = Arc::new(TestLoader {
        value: Arc::new(String::new()),
        calls: AtomicUsize::new(0),
    });
    let output = Vec::with_capacity(64);
    let original_buffer = output.as_ptr();
    let mut cx = WebContext::with_loader_buffer(output, Arc::clone(&loader));
    let original_context = std::ptr::from_ref(&cx);
    let fragment = OptimizedMd::new(|child: &mut WebContext<TestLoader, Vec<u8>>| {
        assert_eq!(std::ptr::from_ref(child), original_context);
        assert!(Arc::ptr_eq(child.loader(), &loader));
        MdRenderSync::render_md(Label("*bridge*"), child)
    });
    WebRenderSync::render(fragment, &mut cx).unwrap();
    assert_eq!(cx.out.as_ptr(), original_buffer);
    assert_eq!(cx.into_output(), b"\\*bridge\\*");
}
#[test]
fn borrowed_buffer_and_borrowed_data_need_no_static_bound() {
    let input = String::from("*borrowed*");
    let mut output = Vec::new();
    {
        let mut cx = WebContext::with_buffer(&mut output);
        complete(MdRender::render_md(Label(&input), &mut cx).resolve()).unwrap();
    }
    assert_eq!(output, b"\\*borrowed\\*");
}
#[test]
fn markdown_no_loader_error_is_preserved() {
    let mut cx = WebContext::new();
    let result = complete(MdRender::render_md(LoadedLabel("missing"), &mut cx).resolve());
    assert!(matches!(result, Err(WriteError::Load(LoadError::NoLoader))));
}
#[test]
fn cancellation_leaves_the_owned_buffer_accessible() {
    let mut cx = WebContext::new();
    let value = String::from("cancel");
    {
        let pending = MdRender::render_md(Label(&value), &mut cx).resolve();
        let mut pending = std::pin::pin!(pending);
        assert!(pending
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop()))
            .is_pending());
    }
    MdRenderSync::render_md(Label("after"), &mut cx).unwrap();
    assert_eq!(cx.into_output().into_string(), "after");
}

struct BorrowedLoader<'a> {
    value: &'a Arc<String>,
}
impl DataLoader for BorrowedLoader<'_> {
    fn load(&self, _: &LoadRequest) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        std::future::ready(Ok(LoadValue::arc(Arc::clone(self.value))))
    }
}

#[test]
fn core_markdown_accepts_a_non_static_loader() {
    let value = Arc::new(String::from("*borrowed loader*"));
    let loader = Arc::new(BorrowedLoader { value: &value });
    let mut cx = WebContext::with_loader(loader);
    complete(MdRender::render_md(LoadedLabel("key"), &mut cx).resolve()).unwrap();
    assert_eq!(cx.into_output().into_string(), "\\*borrowed loader\\*");
}

#[derive(Clone, Copy)]
enum FragmentExit {
    Success,
    Error,
    Panic,
    Pending,
}

fn scoped_fragment(exit: FragmentExit) -> proa_core::md::AsyncMd<'static, NoLoader, RenderBuffer> {
    proa_core::md::async_md_from_fn(Box::new(move |cx| {
        Box::pin(async move {
            assert_eq!(cx.md_state(), proa_core::md::MdState::default());
            cx.md_push_heading_offset(4);
            cx.md_push_list_depth();
            cx.md_set_inline(true);
            cx.write_md_prepared_static(b"fragment\r")?;
            cx.request_mut().set_original_pathname("/fragment");
            YieldOnce(false).await;
            match exit {
                FragmentExit::Success => Ok(()),
                FragmentExit::Error => Err(WriteError::OutOfCapacity),
                FragmentExit::Panic => panic!("fragment unwind"),
                FragmentExit::Pending => std::future::pending().await,
            }
        })
    }))
}

fn outer_context() -> WebContext {
    let mut cx = WebContext::new();
    cx.md_push_heading_offset(2);
    cx.md_push_list_depth();
    cx.write_md_prepared_static(b"outer\r").unwrap();
    cx
}

#[test]
fn async_fragment_restores_complete_state_on_success_error_and_unwind() {
    for exit in [
        FragmentExit::Success,
        FragmentExit::Error,
        FragmentExit::Panic,
    ] {
        let mut cx = outer_context();
        let saved = cx.md_state();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            complete(WebRender::render(scoped_fragment(exit), &mut cx).resolve())
        }));
        match exit {
            FragmentExit::Success => assert!(result.unwrap().is_ok()),
            FragmentExit::Error => assert!(result.unwrap().is_err()),
            FragmentExit::Panic => assert!(result.is_err()),
            FragmentExit::Pending => unreachable!(),
        }
        assert_eq!(cx.md_state(), saved);
        assert_eq!(cx.request().original_pathname(), Some("/fragment"));
        assert_eq!(cx.into_output().into_vec(), b"outer\rfragment\r");
    }
}

#[test]
fn async_fragment_restores_complete_state_on_cancellation() {
    let mut cx = outer_context();
    let saved = cx.md_state();
    let mut future =
        Box::pin(WebRender::render(scoped_fragment(FragmentExit::Pending), &mut cx).resolve());
    let mut task = TaskContext::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut task).is_pending());
    assert!(future.as_mut().poll(&mut task).is_pending());
    drop(future);
    assert_eq!(cx.md_state(), saved);
    assert_eq!(cx.request().original_pathname(), Some("/fragment"));
    assert_eq!(cx.into_output().into_vec(), b"outer\rfragment\r");
}

#[test]
fn direct_markdown_composition_keeps_formatting_changes() {
    let mut cx = outer_context();
    let fragment = OptimizedMd::new(|cx: &mut WebContext| {
        assert_eq!(cx.md_heading_offset(), 2);
        cx.md_push_heading_offset(1);
        cx.write_md_prepared_static(b"\n")
    });
    MdRenderSync::render_md(fragment, &mut cx).unwrap();
    assert_eq!(cx.md_heading_offset(), 3);
    cx.md_block_boundary().unwrap();
    assert_eq!(cx.into_output().into_vec(), b"outer\r\n\n");
}

struct NestedHtml {
    loader: Arc<TestLoader>,
    fail: bool,
}
impl<B: WriteBuf> WebRenderSync<TestLoader, B> for NestedHtml {
    fn render(self, cx: &mut WebContext<TestLoader, B>) -> Result<(), WriteError> {
        assert!(Arc::ptr_eq(cx.loader(), &self.loader));
        assert_eq!(cx.current_mode(), proa_core::ContentMode::Html);
        assert_eq!(
            cx.__proa_current_escape_dialect(),
            proa_core::shared::escape::EscapeDialect::Html
        );
        cx.request_mut().set_original_pathname("/nested");
        cx.rsjs_island_used("nested-artifact");
        cx.out.extend_static(b"html\r")?;
        WebRenderSync::render(
            OptimizedMd::new(|inner: &mut WebContext<TestLoader, B>| {
                assert_eq!(inner.md_state(), proa_core::md::MdState::default());
                inner.md_push_heading_offset(5);
                inner.write_md_prepared_static(b"\n")
            }),
            cx,
        )?;
        cx.md_set_inline(true);
        cx.md_push_heading_offset(3);
        cx.enter_mode(proa_core::ContentMode::Style);
        if self.fail {
            Err(WriteError::OutOfCapacity)
        } else {
            Ok(())
        }
    }
}

#[test]
fn tracked_html_preserves_loader_metadata_and_observed_suffix_across_nested_markdown() {
    for fail in [false, true] {
        let loader = Arc::new(TestLoader {
            value: Arc::new(String::new()),
            calls: AtomicUsize::new(0),
        });
        let mut cx = WebContext::with_loader(Arc::clone(&loader));
        cx.md_push_heading_offset(2);
        cx.md_push_list_depth();
        cx.enter_mode(proa_core::ContentMode::Script);
        let result = cx.render_html_in_md(NestedHtml {
            loader: Arc::clone(&loader),
            fail,
        });
        assert_eq!(result.is_err(), fail);
        assert_eq!(cx.md_heading_offset(), 2);
        assert_eq!(cx.md_list_depth(), 1);
        assert!(!cx.md_inline());
        assert_eq!(cx.current_mode(), proa_core::ContentMode::Script);
        assert_eq!(cx.request().original_pathname(), Some("/nested"));
        assert!(cx.rsjs_islands_used().contains("nested-artifact"));
        cx.md_block_boundary().unwrap();
        assert_eq!(cx.into_output().into_vec(), b"html\r\n\n");
    }
}

#[test]
fn artifact_transaction_crosses_both_format_scopes() {
    let mut cx = WebContext::new();
    for commit in [false, true] {
        let checkpoint = cx.rsjs_artifact_checkpoint();
        cx.render_html_in_md(OptimizedMd::new(|inner: &mut WebContext<NoLoader, _>| {
            inner.rsjs_island_used("scoped-island");
            inner.rsjs_query_seed_json(r#"["scoped"]"#, r#"{"value":1}"#);
            inner.write_md_text("bytes remain")
        }))
        .unwrap();

        // Open transactions are deliberately absent from committed snapshots.
        // Both format scopes must still register against this exact checkpoint.
        assert!(cx.rsjs_artifacts_snapshot().is_empty());
        let pending = cx.rsjs_artifacts_since(&checkpoint);
        assert_eq!(
            pending.rendered_islands().collect::<Vec<_>>(),
            ["scoped-island"]
        );
        assert_eq!(pending.query_seeds().len(), 1);
        if commit {
            cx.rsjs_artifact_commit(checkpoint);
        } else {
            cx.rsjs_artifact_rollback(checkpoint);
        }
        assert_eq!(cx.rsjs_islands_used().contains("scoped-island"), commit);
        assert_eq!(
            cx.rsjs_artifacts_snapshot().query_seeds().len(),
            usize::from(commit)
        );
    }
    assert_eq!(cx.into_output().into_vec(), b"bytes remainbytes remain");
}

struct FailingSink;
impl proa_core::StreamChunkFlusher for FailingSink {
    fn flush_chunk(&self, _: Vec<u8>) -> Result<(), WriteError> {
        Err(WriteError::OutOfCapacity)
    }
}

#[test]
fn markdown_flush_retains_suffix_and_commits_metadata_even_when_handoff_fails() {
    let mut cx = WebContext::new().with_stream_chunk_flusher(std::rc::Rc::new(FailingSink));
    cx.flush_stream_chunk().unwrap();
    assert!(!cx.response().streaming_started());
    assert_eq!(cx.response().stream_flush_generation(), 0);
    cx.write_md_prepared_static(b"first\r").unwrap();
    assert!(cx.flush_stream_chunk().is_err());
    assert!(cx.out.is_empty());
    assert!(cx.response().streaming_started());
    assert_eq!(cx.response().stream_flush_generation(), 1);
    cx.flush_stream_chunk().unwrap();
    assert_eq!(cx.response().stream_flush_generation(), 1);
    cx.write_md_prepared_static(b"\n").unwrap();
    cx.md_block_boundary().unwrap();
    assert_eq!(cx.into_output().into_vec(), b"\n\n");
}
