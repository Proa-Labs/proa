// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::rc::Rc;

use proa_core::{
    MdRender, RenderBuffer, RenderMode, StreamChunkFlusher, StreamChunkView, WriteError,
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
/// while the root [`RenderBuffer`] owns and recycles its pages.
pub struct InOrderMarkdownDriver<R> {
    root: R,
}

impl<R> InOrderMarkdownDriver<R> {
    pub fn new(root: R) -> Self {
        Self { root }
    }
}

impl<R> InOrderMarkdownDriver<R>
where
    R: MdRender<RenderBuffer>,
{
    pub async fn render_to_flusher(
        self,
        flusher: Rc<dyn StreamChunkFlusher>,
    ) -> Result<(), WriteError> {
        render_md_in_order_to_flusher(self.root, flusher).await
    }

    pub async fn render_to_vec(self) -> Result<Vec<u8>, WriteError> {
        render_md_in_order_to_vec(self.root).await
    }
}

/// Render an async-capable Markdown root to an in-order stream flusher.
///
pub async fn render_md_in_order_to_flusher<R>(
    root: R,
    flusher: Rc<dyn StreamChunkFlusher>,
) -> Result<(), WriteError>
where
    R: MdRender<RenderBuffer>,
{
    let mut out = RenderBuffer::new();
    let mut cx = proa_core::md::MdContext::with_buffer(&mut out)
        .with_render_mode(RenderMode::ChunkedInOrder)
        .with_stream_chunk_flusher(flusher);

    proa_core::render_md_in_context_async(&mut cx, root).await?;
    cx.flush_stream_chunk()
}

/// Render an async-capable Markdown root to a contiguous byte vector.
///
/// This helper is primarily useful for tests and non-framework integrations.
/// Streaming transports should prefer [`render_md_in_order_to_flusher`].
pub async fn render_md_in_order_to_vec<R>(root: R) -> Result<Vec<u8>, WriteError>
where
    R: MdRender<RenderBuffer>,
{
    let sink = Rc::new(VecChunkFlusher::default());
    let flusher: Rc<dyn StreamChunkFlusher> = sink.clone();
    render_md_in_order_to_flusher(root, flusher).await?;
    let output = sink.output.borrow().clone();
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;
    use std::time::Duration;

    use proa_core::{MdRenderSync, Priority, RenderOutcome};

    use super::*;
    use crate::Suspense;

    struct StaticMd(&'static str);

    impl<B: proa_core::WriteBuf> MdRenderSync<B> for StaticMd {
        fn render_md(self, cx: &mut proa_core::md::MdContext<'_, B>) -> Result<(), WriteError> {
            cx.write_prepared_static(self.0.as_bytes())
        }
    }

    struct BoundaryPage;

    impl MdRender<RenderBuffer> for BoundaryPage {
        fn render_md<'cx, 'out>(
            self,
            cx: &'cx mut proa_core::md::MdContext<'out, RenderBuffer>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
        where
            Self: 'cx,
            'out: 'cx,
        {
            let fut: Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'cx>> =
                Box::pin(async move {
                    cx.write_prepared_static(b"before\n")?;
                    proa_core::render_md_child(
                        cx,
                        Suspense::new(proa_core::to_md_async(StaticMd("child\n\n")), ()),
                    )
                    .await?;
                    cx.blank_line()?;
                    cx.write_prepared_static(b"after\n")?;
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
}
