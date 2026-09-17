// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use proa_core::{
    DataLoader, MdRender, NoLoader, RenderMode, StreamChunkFlusher, StreamChunkView, WebContext,
    WriteError,
};

#[derive(Default)]
struct VecChunkFlusher {
    output: RefCell<Vec<u8>>,
}

impl StreamChunkFlusher for VecChunkFlusher {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.output.borrow_mut().extend_from_slice(&chunk);
        Ok(())
    }

    fn flush_chunk_view(&self, chunk: &dyn StreamChunkView) -> Result<(), WriteError> {
        let mut output = self.output.borrow_mut();
        chunk.for_each_slice(&mut |slice| {
            output.extend_from_slice(slice);
            Ok(())
        })
    }
}

/// In-order Markdown stream driver.
///
/// The driver is transport-agnostic: callers provide a [`StreamChunkFlusher`]
/// while the root [`RenderBuffer`](proa_core::RenderBuffer) owns and recycles its pages.
pub struct InOrderMarkdownDriver<L: DataLoader, R> {
    root: R,
    loader: Arc<L>,
}

impl<R> InOrderMarkdownDriver<NoLoader, R> {
    pub fn new(root: R) -> Self {
        Self::with_loader(root, Arc::new(NoLoader))
    }
}

impl<L: DataLoader, R> InOrderMarkdownDriver<L, R> {
    /// Keep the caller's request-scoped loader throughout the Markdown tree.
    pub fn with_loader(root: R, loader: Arc<L>) -> Self {
        Self { root, loader }
    }
}

impl<L: DataLoader, R: MdRender<L>> InOrderMarkdownDriver<L, R> {
    pub async fn render_to_flusher(
        self,
        flusher: Rc<dyn StreamChunkFlusher>,
    ) -> Result<(), WriteError> {
        render_md_in_order_to_flusher_with_loader(self.root, self.loader, flusher).await
    }

    pub async fn render_to_vec(self) -> Result<Vec<u8>, WriteError> {
        render_md_in_order_to_vec_with_loader(self.root, self.loader).await
    }
}

/// Render an async-capable Markdown root to an in-order stream flusher.
pub async fn render_md_in_order_to_flusher<R: MdRender>(
    root: R,
    flusher: Rc<dyn StreamChunkFlusher>,
) -> Result<(), WriteError> {
    render_md_in_order_to_flusher_with_loader(root, Arc::new(NoLoader), flusher).await
}

/// Render in document order with an existing request-scoped loader.
pub async fn render_md_in_order_to_flusher_with_loader<L: DataLoader, R: MdRender<L>>(
    root: R,
    loader: Arc<L>,
    flusher: Rc<dyn StreamChunkFlusher>,
) -> Result<(), WriteError> {
    let mut cx = WebContext::with_loader(loader)
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);
    proa_core::render_md_in_context_async(&mut cx, root).await?;
    cx.flush_stream_chunk()
}

/// Collect an in-order Markdown render. Streaming transports should prefer
/// [`render_md_in_order_to_flusher`].
pub async fn render_md_in_order_to_vec<R: MdRender>(root: R) -> Result<Vec<u8>, WriteError> {
    render_md_in_order_to_vec_with_loader(root, Arc::new(NoLoader)).await
}

/// Collect a Markdown render using the caller's loader instance.
pub async fn render_md_in_order_to_vec_with_loader<L: DataLoader, R: MdRender<L>>(
    root: R,
    loader: Arc<L>,
) -> Result<Vec<u8>, WriteError> {
    let sink = Rc::new(VecChunkFlusher::default());
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    render_md_in_order_to_flusher_with_loader(root, loader, flusher).await?;
    let output = std::mem::take(&mut *sink.output.borrow_mut());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;
    use std::time::Duration;

    use proa_core::{MdRenderSync, Priority, RenderBuffer, RenderOutcome};

    use super::*;
    use crate::Suspense;

    struct StaticMd(&'static str);

    impl<Loader: ::proa_core::DataLoader, B: proa_core::WriteBuf> MdRenderSync<Loader, B> for StaticMd {
        fn render_md(self, cx: &mut proa_core::WebContext<Loader, B>) -> Result<(), WriteError> {
            cx.write_md_prepared_static(self.0.as_bytes())
        }
    }

    struct BoundaryPage;

    impl<Loader: ::proa_core::DataLoader> MdRender<Loader, RenderBuffer> for BoundaryPage {
        fn render_md(
            self,
            cx: &mut proa_core::WebContext<Loader, RenderBuffer>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            let fut: Pin<Box<dyn Future<Output = Result<(), WriteError>> + '_>> =
                Box::pin(async move {
                    cx.write_md_prepared_static(b"before\n")?;
                    proa_core::render_md_child(
                        cx,
                        Suspense::new(proa_core::to_md_async(StaticMd("child\n\n")), ()),
                    )
                    .await?;
                    cx.md_blank_line()?;
                    cx.write_md_prepared_static(b"after\n")?;
                    Ok(())
                });
            RenderOutcome::Pending(fut)
        }
    }

    struct CaptureSink {
        chunks: RefCell<Vec<Vec<u8>>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                chunks: RefCell::new(Vec::new()),
            }
        }

        fn chunks(&self) -> Vec<Vec<u8>> {
            self.chunks.borrow().clone()
        }
    }

    impl StreamChunkFlusher for CaptureSink {
        fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
            self.chunks.borrow_mut().push(chunk);
            Ok(())
        }

        fn flush_chunk_view(
            &self,
            chunk: &dyn proa_core::StreamChunkView,
        ) -> Result<(), WriteError> {
            let mut out = Vec::with_capacity(chunk.total_len());
            chunk.for_each_slice(&mut |slice| {
                out.extend_from_slice(slice);
                Ok(())
            })?;
            self.flush_chunk(out)
        }
    }

    #[tokio::test]
    async fn render_to_vec_collects_markdown_output() {
        let out = render_md_in_order_to_vec(proa_core::to_md_async(StaticMd("hello\n")))
            .await
            .unwrap();

        assert_eq!(out, b"hello\n");
    }

    #[tokio::test]
    async fn suspense_flushes_markdown_chunks_in_order() {
        let sink = Rc::new(CaptureSink::new());
        let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();

        render_md_in_order_to_flusher(BoundaryPage, flusher)
            .await
            .unwrap();

        let chunks = sink.chunks();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], b"before\n");
        assert_eq!(chunks[1], b"child\n\n");
        assert_eq!(chunks[2], b"after\n");
    }

    #[tokio::test]
    async fn markdown_suspense_options_preserve_inline_source_order() {
        let out = render_md_in_order_to_vec(Suspense {
            child: proa_core::to_md_async(StaticMd("child\n")),
            fallback: (),
            priority: Priority::Defer,
            timeout: Some(Duration::ZERO),
        })
        .await
        .unwrap();

        assert_eq!(out, b"child\n");
    }

    struct TestLoader(std::sync::atomic::AtomicUsize);

    impl DataLoader for TestLoader {
        fn load(
            &self,
            _: &proa_core::LoadRequest,
        ) -> impl Future<Output = Result<proa_core::LoadValue, proa_core::LoadError>> + Send
        {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            std::future::ready(Ok(proa_core::LoadValue::arc(Arc::new(String::from(
                "*loaded*",
            )))))
        }
    }

    struct NeverHtmlFallback;

    impl proa_core::render::BoundaryErrorFallback<TestLoader> for NeverHtmlFallback {
        fn render_buffer(&self, _: &mut WebContext<TestLoader>) -> Result<(), WriteError> {
            panic!("Markdown must not invoke the registered HTML error fallback");
        }
    }

    struct LoadedChild {
        loader: Arc<TestLoader>,
        fail: bool,
    }

    impl MdRender<TestLoader> for LoadedChild {
        fn render_md(
            self,
            cx: &mut WebContext<TestLoader>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                assert!(Arc::ptr_eq(cx.loader(), &self.loader));
                let value = cx
                    .load_typed::<String>(proa_core::LoadKey::named("probe", "child"))
                    .await?;
                cx.write_md_text(value.as_str())?;
                if self.fail {
                    Err(WriteError::render("markdown child failed"))
                } else {
                    Ok(())
                }
            })
        }
    }

    struct LoadedPage(LoadedChild);

    impl MdRender<TestLoader> for LoadedPage {
        fn render_md(
            self,
            cx: &mut WebContext<TestLoader>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                let _fallback = cx.push_boundary_error_fallback(Rc::new(NeverHtmlFallback));
                proa_core::render_md_child(
                    cx,
                    Suspense {
                        child: self.0,
                        fallback: (),
                        priority: Priority::Defer,
                        timeout: Some(Duration::ZERO),
                    },
                )
                .await
            })
        }
    }

    #[tokio::test]
    async fn markdown_driver_keeps_the_loader_and_never_uses_html_error_fallbacks() {
        let loader = Arc::new(TestLoader(std::sync::atomic::AtomicUsize::new(0)));
        for fail in [false, true] {
            let sink = Rc::new(CaptureSink::new());
            let result = InOrderMarkdownDriver::with_loader(
                LoadedPage(LoadedChild {
                    loader: Arc::clone(&loader),
                    fail,
                }),
                Arc::clone(&loader),
            )
            .render_to_flusher(sink.clone())
            .await;
            if fail {
                assert!(matches!(result, Err(WriteError::Render(_))));
                assert!(sink.chunks().is_empty());
            } else {
                result.unwrap();
                assert_eq!(sink.chunks(), [b"\\*loaded\\*".to_vec()]);
            }
        }
        assert_eq!(loader.0.load(std::sync::atomic::Ordering::Relaxed), 2);
    }
}
