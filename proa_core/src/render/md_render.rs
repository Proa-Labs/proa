// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Markdown rendering traits + standard impls.
//!
//! Markdown mirrors the web rendering split:
//!
//! - [`MdRenderSync`] for synchronous Markdown components, the default
//!   zero-overhead fast path.
//! - [`MdRender`] for components that may need to `.await`.
//!
//! Both traits share a loader-first context with the HTML traits. The buffer
//! type remains customizable, so the same component can
//! render to a `Vec<u8>`, a `ChainedBumpWriter`, or any other [`WriteBuf`].
//!
//! Synchronous standard impls for `()`, `Option<T>`, strings, and tuples up to
//! arity 12 cover the common composition patterns. Common scalar values also
//! implement [`MdRender`] directly so async `md!` interpolation stays concise.

use std::future::Future;

use crate::render::outcome::RenderOutcome;
use crate::shared::write_buf::{WriteBuf, WriteError};
use crate::{DataLoader, NoLoader, RenderBuffer, WebContext};

/// Trait for components that can render to Markdown synchronously.
pub trait MdRenderSync<Loader: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    /// Render this component to Markdown synchronously.
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError>;
}

/// Trait for components that can render to Markdown possibly asynchronously.
///
/// Async Markdown uses the same [`RenderOutcome`] shape as [`crate::WebRender`]:
/// sync completions can return `Done`, while components that need to await
/// return `Pending`.
pub trait MdRender<Loader: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    fn render_md(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>;
}

/// Macro-internal dispatch surface for synchronous Markdown interpolation.
///
/// The unique method name prevents a caller importing both [`MdRenderSync`]
/// and [`MdRender`] from making generated macro calls ambiguous. Method syntax
/// still preserves Rust auto-deref for values such as `&&str`.
#[doc(hidden)]
pub trait MdRenderSyncExt<Loader: DataLoader = NoLoader, B: WriteBuf = RenderBuffer>:
    MdRenderSync<Loader, B>
{
    fn __proa_render_md_sync(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError>
    where
        Self: Sized,
    {
        MdRenderSync::render_md(self, cx)
    }
}

impl<Loader: DataLoader, B, T> MdRenderSyncExt<Loader, B> for T
where
    B: WriteBuf,
    T: MdRenderSync<Loader, B>,
{
}

/// Macro-internal dispatch surface for async-capable Markdown interpolation.
///
/// This mirrors [`MdRenderSyncExt`] while preserving the concrete future
/// returned by the underlying [`MdRender`] implementation.
#[doc(hidden)]
pub trait MdRenderAsyncExt<Loader: DataLoader = NoLoader, B: WriteBuf = RenderBuffer>:
    MdRender<Loader, B>
{
    fn __proa_render_md_async(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    where
        Self: Sized,
    {
        MdRender::render_md(self, cx)
    }
}

impl<Loader: DataLoader, B, T> MdRenderAsyncExt<Loader, B> for T
where
    B: WriteBuf,
    T: MdRender<Loader, B>,
{
}

/// Explicit adapter that lifts a sync Markdown component into the
/// async-capable [`MdRender`] surface.
pub struct MdSyncToAsync<T>(pub T);

impl<Loader: DataLoader, T, B> MdRender<Loader, B> for MdSyncToAsync<T>
where
    T: MdRenderSync<Loader, B>,
    B: WriteBuf,
{
    fn render_md(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(self.0.render_md(cx))
    }
}

/// Wrap a future that resolves to a Markdown renderable child.
pub struct AwaitMdComponent<F>(pub F);

impl<Loader: DataLoader, F, B, R> MdRender<Loader, B> for AwaitMdComponent<F>
where
    F: Future<Output = R>,
    B: WriteBuf,
    R: MdRender<Loader, B>,
{
    fn render_md(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let child = self.0.await;
            child.render_md(cx).resolve().await
        })
    }
}

/// Unit type renders nothing - useful for components with optional children.
impl<Loader: DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for () {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<Loader: DataLoader, B: WriteBuf> MdRender<Loader, B> for () {
    fn render_md(
        self,
        _cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}

/// `Option<T>` renders `T` if `Some`, nothing if `None` - useful for optional children.
impl<Loader: DataLoader, B: WriteBuf, T: MdRenderSync<Loader, B>> MdRenderSync<Loader, B>
    for Option<T>
{
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        if let Some(inner) = self {
            inner.render_md(cx)?;
        }
        Ok(())
    }
}

/// &str renders as escaped markdown text.
impl<Loader: DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for &str {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.write_md_text(self)
    }
}

/// String renders as escaped markdown text.
impl<Loader: DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for String {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.write_md_text(&self)
    }
}

/// Borrowed owned strings render without cloning or moving the allocation.
impl<Loader: DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for &String {
    fn render_md(self, cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        cx.write_md_text(self)
    }
}

/// Tuple impls for MdRenderSync (arity 2-12).
macro_rules! impl_md_render_tuple {
    ($($T:ident),+) => {
        impl<Loader: DataLoader, Buf: WriteBuf, $($T: MdRenderSync<Loader, Buf>),+> MdRenderSync<Loader, Buf> for ($($T,)+) {
            #[allow(non_snake_case)]
            fn render_md(self, cx: &mut WebContext<Loader, Buf>) -> Result<(), WriteError> {
                let ($($T,)+) = self;
                $($T.render_md(cx)?;)+
                Ok(())
            }
        }
    };
}

impl_md_render_tuple!(A, B);
impl_md_render_tuple!(A, B, C);
impl_md_render_tuple!(A, B, C, D);
impl_md_render_tuple!(A, B, C, D, E);
impl_md_render_tuple!(A, B, C, D, E, F);
impl_md_render_tuple!(A, B, C, D, E, F, G);
impl_md_render_tuple!(A, B, C, D, E, F, G, H);
impl_md_render_tuple!(A, B, C, D, E, F, G, H, I);
impl_md_render_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_md_render_tuple!(A, B, C, D, E, F, G, H, I, J, K);
impl_md_render_tuple!(A, B, C, D, E, F, G, H, I, J, K, L);
