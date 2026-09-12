// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Markdown rendering target
//!
//! This module provides functionality for rendering components to clean Markdown
//! for agent/API consumption. Components map to semantic markdown equivalents:
//! Cards become headings, badges become inline code, tables become markdown tables, etc.
pub mod context;
pub mod escape;
use crate::render::DataLoader;
use crate::render::{MdRender, MdRenderSync, RenderOutcome, WebRender, WebRenderSync};
use crate::shared::write_buf::{WriteBuf, WriteError};
use crate::web::WebContext;
pub use context::{MdContext, MdContextState};
use core::future::Future;
use core::marker::PhantomData;
use core::pin::Pin;
use std::borrow::Cow;

/// Explicit wrapper for trusted, already-authored Markdown source.
///
/// Unlike ordinary strings, which render as escaped literal text, this type
/// preserves Markdown syntax. Keep runtime or user-controlled text on the
/// normal string path unless the caller intentionally accepts raw Markdown.
pub struct TrustedMarkdown<'a>(pub &'a str);

impl<'a, B: WriteBuf> MdRenderSync<B> for TrustedMarkdown<'a> {
    #[inline(always)]
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_trusted_markdown(self.0)
    }
}

impl<'a, B: WriteBuf> MdRender<B> for TrustedMarkdown<'a> {
    #[inline(always)]
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}

/// Runtime text that can be rendered as adaptive inline code without first
/// allocating an intermediate Markdown buffer.
///
/// Components whose Markdown representation is a code span intentionally use
/// this narrower contract instead of arbitrary [`MdRenderSync`] children: the
/// renderer must inspect the raw payload before choosing a safe delimiter.
pub trait MdCodeContent {
    /// Borrow the raw semantic code-span payload.
    fn md_code_content(&self) -> &str;
}

impl MdCodeContent for str {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        self
    }
}

impl<T: MdCodeContent + ?Sized> MdCodeContent for &T {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        (*self).md_code_content()
    }
}

impl MdCodeContent for String {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        self.as_str()
    }
}

impl MdCodeContent for Cow<'_, str> {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        self.as_ref()
    }
}

impl MdCodeContent for crate::Text<'_> {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        self.0
    }
}

impl MdCodeContent for crate::SafeText<'_> {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        self.0
    }
}

impl MdCodeContent for () {
    #[inline(always)]
    fn md_code_content(&self) -> &str {
        ""
    }
}
/// Adapter that bridges `std::fmt::Write` → [`WriteBuf`].
///
/// Used by the `md!` / `md_sync!` macros to write `Display` values directly
/// into the byte buffer without an intermediate `String` allocation. The adapter retains
/// the original [`WriteError`] because `core::fmt::Error` carries no payload;
/// callers should use [`FmtBuf::write_fmt`] or pass a manual formatting result
/// to [`FmtBuf::finish`] instead of discarding that side channel.
pub struct FmtBuf<'a, B: WriteBuf> {
    out: &'a mut B,
    error: Option<WriteError>,
}

impl<'a, B: WriteBuf> FmtBuf<'a, B> {
    /// Wrap a Markdown output buffer for allocation-free `Display` formatting.
    #[inline]
    pub const fn new(out: &'a mut B) -> Self {
        Self { out, error: None }
    }

    /// Format `args` and preserve the exact underlying [`WriteError`].
    #[inline]
    pub fn write_fmt(&mut self, args: core::fmt::Arguments<'_>) -> Result<(), WriteError> {
        let result = core::fmt::write(self, args);
        self.finish(result)
    }

    /// Convert a formatting result back to its original write error.
    ///
    /// A formatter-originated `fmt::Error` with no failed buffer write retains
    /// the legacy [`WriteError::OutOfCapacity`] fallback.
    #[inline]
    pub fn finish(&mut self, result: core::fmt::Result) -> Result<(), WriteError> {
        match (result, self.error.take()) {
            (_, Some(error)) => Err(error),
            (Ok(()), None) => Ok(()),
            (Err(_), None) => Err(WriteError::OutOfCapacity),
        }
    }
}

impl<B: WriteBuf> core::fmt::Write for FmtBuf<'_, B> {
    #[inline]
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if self.error.is_some() {
            return Err(core::fmt::Error);
        }

        match self.out.extend(s.as_bytes()) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.error = Some(error);
                Err(core::fmt::Error)
            }
        }
    }
}
/// Convert a payload-free `fmt::Error` back to the legacy capacity error.
///
/// New code using [`FmtBuf`] should call [`FmtBuf::finish`] or
/// [`FmtBuf::write_fmt`] so structured write failures are not erased.
#[inline]
pub fn fmt_err_to_write(_: core::fmt::Error) -> WriteError {
    WriteError::OutOfCapacity
}
/// Wrapper for synchronous `md_sync!` macro output.
///
/// Contains a closure that renders markdown into an [`MdContext`].
/// Implements the synchronous render traits directly and can also complete
/// immediately in async composition.
pub struct OptimizedMd<B, F> {
    pub render_fn: F,
    pub _marker: PhantomData<fn(&mut B)>,
}
impl<B, F> OptimizedMd<B, F> {
    #[inline(always)]
    pub fn new(render_fn: F) -> Self {
        OptimizedMd {
            render_fn,
            _marker: PhantomData,
        }
    }
}
/// Primary path: compose inside `MdRenderSync` impls via `.render_md(cx)`.
impl<B, F> MdRenderSync<B> for OptimizedMd<B, F>
where
    B: WriteBuf,
    F: FnOnce(&mut MdContext<'_, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        (self.render_fn)(cx)
    }
}

impl<B, F> MdRender<B> for OptimizedMd<B, F>
where
    B: WriteBuf,
    F: FnOnce(&mut MdContext<'_, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(
            self, cx,
        ))
    }
}

/// Bridge path: allows `render_to_string(md_sync!(...))` and
/// `cx.render(md_sync!(...))` in WebRender.
/// Creates a temporary `MdContext` wrapping the `WebContext`'s buffer.
impl<B, L, F> WebRenderSync<L, B> for OptimizedMd<B, F>
where
    B: WriteBuf,
    L: DataLoader,
    F: FnOnce(&mut MdContext<'_, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mut md_cx = MdContext::with_buffer(&mut cx.out);
        (self.render_fn)(&mut md_cx)
    }
}

// proa-perf-ok(scope): intentional immediate sync-to-async composition bridge
impl<B, L, F> WebRender<L, B> for OptimizedMd<B, F>
where
    B: WriteBuf,
    L: DataLoader,
    F: FnOnce(&mut MdContext<'_, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(WebRenderSync::render(
            self, cx,
        ))
    }
}

/// Boxed future returned by [`AsyncMd`]'s type-erased render closure.
pub type AsyncMdFuture<'a> = Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'a>>;

/// Type-erased async Markdown render continuation.
pub type AsyncMdRenderFn<'env, B> =
    dyn for<'a, 'out> FnOnce(&'a mut MdContext<'out, B>) -> AsyncMdFuture<'a> + Send + 'env;

/// Wrapper for dynamic async-capable `md!` macro output.
///
/// The macro emits the template-specific closure. This shared wrapper carries
/// the `MdRender` and `WebRender` implementations once in `proa_core`.
pub struct AsyncMd<'env, B: WriteBuf> {
    pub render_fn: Box<AsyncMdRenderFn<'env, B>>,
    pub _marker: PhantomData<fn(&mut B)>,
}

/// Construct an [`AsyncMd`] from an async render closure.
#[inline(always)]
pub fn async_md_from_fn<'env, B>(render_fn: Box<AsyncMdRenderFn<'env, B>>) -> AsyncMd<'env, B>
where
    B: WriteBuf,
{
    AsyncMd {
        render_fn,
        _marker: PhantomData,
    }
}

impl<'env, B> MdRender<B> for AsyncMd<'env, B>
where
    B: WriteBuf,
{
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::Pending((self.render_fn)(cx))
    }
}

impl<'env, B, L> WebRender<L, B> for AsyncMd<'env, B>
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            let mut md_cx = MdContext::with_buffer(&mut cx.out);
            (self.render_fn)(&mut md_cx).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MeteredWriteBuf, OutputMeter};

    struct ChunkedDisplay;

    impl core::fmt::Display for ChunkedDisplay {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("ab")?;
            f.write_str("cdef")
        }
    }

    #[test]
    fn fmt_buf_preserves_structured_output_limit_errors() {
        let meter = OutputMeter::new(3);
        let mut out = MeteredWriteBuf::new(Vec::new(), meter.borrow());
        let mut writer = FmtBuf::new(&mut out);

        let error = writer
            .write_fmt(format_args!("{}", ChunkedDisplay))
            .unwrap_err();

        assert_eq!(
            error,
            WriteError::OutputLimitExceeded {
                requested: 4,
                remaining: 1,
            }
        );
        assert_eq!(out.as_slice(), b"ab");
        assert_eq!(meter.remaining(), 1);
    }
}
