// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Const-friendly HTML types for compile-time HTML generation.
use crate::render::{
    DataLoader, NoLoader, RenderOutcome, SendWebContext, WebContext, WebRender, WebRenderSend,
    WebRenderSendSync, WebRenderSync,
};
use crate::shared::write_buf::{WriteBuf, WriteError};
use core::future::Future;
use core::marker::PhantomData;
use core::pin::Pin;
/// A const-friendly HTML structure that holds a function pointer for rendering.
/// Used to enable compile-time HTML generation with composable children.
///
/// Note: This uses `Vec<u8>` for the function pointer signature since function
/// pointers cannot be generic. For true generic buffer support, implement
/// `WebRender<::proa_core::NoLoader, B>` on a custom struct with raw buffer writes.
#[derive(Debug, Clone, Copy)]
pub struct ConstHtml {
    pub render_fn: fn(&mut WebContext<NoLoader, Vec<u8>>) -> Result<(), WriteError>,
}
impl WebRenderSync<NoLoader, Vec<u8>> for ConstHtml {
    fn render(self, cx: &mut WebContext<NoLoader, Vec<u8>>) -> Result<(), WriteError> {
        (self.render_fn)(cx)
    }
}
impl WebRender<NoLoader, Vec<u8>> for ConstHtml {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, Vec<u8>>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(WebRenderSync::render(
            self, cx,
        ))
    }
}

/// Fully static HTML byte sequence.
///
/// This is the smallest wrapper the `html!` / `html_sync!` macros can emit for
/// a completely static tree: one byte-string literal plus this shared render
/// implementation.
#[derive(Debug, Clone, Copy)]
pub struct StaticHtml {
    pub bytes: &'static [u8],
}

#[inline(always)]
pub const fn static_html(bytes: &'static [u8]) -> StaticHtml {
    StaticHtml { bytes }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticHtml {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend_static(self.bytes)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for StaticHtml {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(WebRenderSync::render(
            self, cx,
        ))
    }
}

impl<L: DataLoader, B: WriteBuf + Send> WebRenderSendSync<L, B> for StaticHtml {
    #[inline(always)]
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend_static(self.bytes)
    }
}

impl<L: DataLoader, B: WriteBuf + Send> WebRenderSend<L, B> for StaticHtml {
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(self.render_send_sync(cx))
    }
}

/// Wrapper for html_optimized! macro output
/// Contains a closure that will render the optimized HTML.
/// The buffer type B is a phantom parameter that helps with type inference.
pub struct OptimizedHtml<B, L, F> {
    pub render_fn: F,
    pub _marker: PhantomData<fn(&mut B, &L)>,
}
impl<L, B, F> OptimizedHtml<B, L, F> {
    /// Create a new OptimizedHtml wrapper.
    #[inline(always)]
    pub fn new(render_fn: F) -> Self {
        OptimizedHtml {
            render_fn,
            _marker: PhantomData,
        }
    }
}

/// Construct an [`OptimizedHtml`] from a closure that renders into a typed
/// `WebContext<L, B>`. The bound on `F` lets the compiler infer the
/// closure parameter type, which is what the `html_sync!` macro relies on so
/// the generated code stays generic over the buffer `B` and the request loader
/// `L`.
#[inline(always)]
pub fn optimized_html_from_fn<B, L, F>(render_fn: F) -> OptimizedHtml<B, L, F>
where
    B: WriteBuf,
    L: DataLoader,
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    OptimizedHtml::new(render_fn)
}
impl<L, B, F> WebRenderSync<L, B> for OptimizedHtml<B, L, F>
where
    B: WriteBuf,
    L: DataLoader,
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        (self.render_fn)(cx)
    }
}
impl<L, B, F> WebRender<L, B> for OptimizedHtml<B, L, F>
where
    B: WriteBuf,
    L: DataLoader,
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(WebRenderSync::render(
            self, cx,
        ))
    }
}

/// Boxed future returned by [`AsyncHtml`]'s type-erased render closure.
pub type AsyncHtmlFuture<'a> = Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'a>>;
/// Type-erased async HTML render continuation.
pub type AsyncHtmlRenderFn<'env, B, L> =
    dyn for<'a> FnOnce(&'a mut WebContext<L, B>) -> AsyncHtmlFuture<'a> + Send + 'env;

/// Wrapper for async `html!` macro output.
///
/// The macro emits only the template-specific closure; this shared wrapper
/// carries the `WebRender` implementation once in `proa_core` instead of
/// generating a fresh local type and impl at every `html!` call site.
pub struct AsyncHtml<'env, B: WriteBuf, L: DataLoader> {
    pub render_fn: Box<AsyncHtmlRenderFn<'env, B, L>>,
    pub _marker: PhantomData<fn(&mut B, &L)>,
}

/// Construct an [`AsyncHtml`] from a type-erased async render closure.
#[inline(always)]
pub fn async_html_from_fn<'env, B, L>(
    render_fn: Box<AsyncHtmlRenderFn<'env, B, L>>,
) -> AsyncHtml<'env, B, L>
where
    B: WriteBuf,
    L: DataLoader,
{
    AsyncHtml {
        render_fn,
        _marker: PhantomData,
    }
}

impl<'env, L, B> WebRender<L, B> for AsyncHtml<'env, B, L>
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending((self.render_fn)(cx))
    }
}

/// Boxed future returned by a send-capable generated HTML closure.
pub type SendAsyncHtmlFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), WriteError>> + Send + 'a>>;
pub type SendAsyncHtmlRenderFn<'env, B, L> =
    dyn for<'a> FnOnce(&'a mut SendWebContext<L, B>) -> SendAsyncHtmlFuture<'a> + Send + 'env;

/// Wrapper emitted by `html_send!`.
pub struct SendAsyncHtml<'env, B: WriteBuf + Send, L: DataLoader> {
    pub render_fn: Box<SendAsyncHtmlRenderFn<'env, B, L>>,
    pub _marker: PhantomData<fn(&mut B, &L)>,
}

#[inline(always)]
pub fn send_async_html_from_fn<'env, B, L>(
    render_fn: Box<SendAsyncHtmlRenderFn<'env, B, L>>,
) -> SendAsyncHtml<'env, B, L>
where
    B: WriteBuf + Send,
    L: DataLoader,
{
    SendAsyncHtml {
        render_fn,
        _marker: PhantomData,
    }
}

impl<'env, L, B> WebRenderSend<L, B> for SendAsyncHtml<'env, B, L>
where
    B: WriteBuf + Send,
    L: DataLoader,
{
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::Pending((self.render_fn)(cx))
    }
}
