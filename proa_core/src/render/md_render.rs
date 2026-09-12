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
//! Both traits are generic over the buffer type so the same component can
//! render to a `Vec<u8>`, a `ChainedBumpWriter`, or any other [`WriteBuf`].
//!
//! Synchronous standard impls for `()`, `Option<T>`, strings, and tuples up to
//! arity 12 cover the common composition patterns. Common scalar values also
//! implement [`MdRender`] directly so async `md!` interpolation stays concise.

use std::future::Future;

use crate::md::MdContext;
use crate::render::outcome::RenderOutcome;
use crate::shared::write_buf::{WriteBuf, WriteError};

/// Trait for components that can render to Markdown synchronously.
pub trait MdRenderSync<B: WriteBuf> {
    /// Render this component to Markdown synchronously.
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError>;
}

/// Trait for components that can render to Markdown possibly asynchronously.
///
/// Async Markdown uses the same [`RenderOutcome`] shape as [`crate::WebRender`]:
/// sync completions can return `Done`, while components that need to await
/// return `Pending`.
pub trait MdRender<B: WriteBuf> {
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx;
}

/// Macro-internal dispatch surface for synchronous Markdown interpolation.
///
/// The unique method name prevents a caller importing both [`MdRenderSync`]
/// and [`MdRender`] from making generated macro calls ambiguous. Method syntax
/// still preserves Rust auto-deref for values such as `&&str`.
#[doc(hidden)]
pub trait MdRenderSyncExt<B: WriteBuf>: MdRenderSync<B> {
    fn __proa_render_md_sync(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError>
    where
        Self: Sized,
    {
        MdRenderSync::render_md(self, cx)
    }
}

impl<B, T> MdRenderSyncExt<B> for T
where
    B: WriteBuf,
    T: MdRenderSync<B>,
{
}

/// Macro-internal dispatch surface for async-capable Markdown interpolation.
///
/// This mirrors [`MdRenderSyncExt`] while preserving the concrete future
/// returned by the underlying [`MdRender`] implementation.
#[doc(hidden)]
pub trait MdRenderAsyncExt<B: WriteBuf>: MdRender<B> {
    fn __proa_render_md_async<'a, 'out>(
        self,
        cx: &'a mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'a>
    where
        Self: Sized + 'a,
    {
        MdRender::render_md(self, cx)
    }
}

impl<B, T> MdRenderAsyncExt<B> for T
where
    B: WriteBuf,
    T: MdRender<B>,
{
}

/// Explicit adapter that lifts a sync Markdown component into the
/// async-capable [`MdRender`] surface.
pub struct MdSyncToAsync<T>(pub T);

impl<T, B> MdRender<B> for MdSyncToAsync<T>
where
    T: MdRenderSync<B>,
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
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(self.0.render_md(cx))
    }
}

/// Wrap a future that resolves to a Markdown renderable child.
pub struct AwaitMdComponent<F>(pub F);

impl<F, B, R> MdRender<B> for AwaitMdComponent<F>
where
    F: Future<Output = R>,
    B: WriteBuf,
    R: MdRender<B>,
{
    fn render_md<'cx, 'out>(
        self,
        cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::pending(async move {
            let child = self.0.await;
            child.render_md(cx).resolve().await
        })
    }
}

/// Unit type renders nothing - useful for components with optional children.
impl<B: WriteBuf> MdRenderSync<B> for () {
    fn render_md(self, _cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<B: WriteBuf> MdRender<B> for () {
    fn render_md<'cx, 'out>(
        self,
        _cx: &'cx mut MdContext<'out, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + 'cx>
    where
        Self: 'cx,
        'out: 'cx,
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}

/// `Option<T>` renders `T` if `Some`, nothing if `None` - useful for optional children.
impl<B: WriteBuf, T: MdRenderSync<B>> MdRenderSync<B> for Option<T> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        if let Some(inner) = self {
            inner.render_md(cx)?;
        }
        Ok(())
    }
}

/// &str renders as escaped markdown text.
impl<B: WriteBuf> MdRenderSync<B> for &str {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_text(self)
    }
}

/// String renders as escaped markdown text.
impl<B: WriteBuf> MdRenderSync<B> for String {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_text(&self)
    }
}

/// Borrowed owned strings render without cloning or moving the allocation.
impl<B: WriteBuf> MdRenderSync<B> for &String {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.write_text(self)
    }
}

/// Tuple impls for MdRenderSync (arity 2-12).
macro_rules! impl_md_render_tuple {
    ($($T:ident),+) => {
        impl<Buf: WriteBuf, $($T: MdRenderSync<Buf>),+> MdRenderSync<Buf> for ($($T,)+) {
            #[allow(non_snake_case)]
            fn render_md(self, cx: &mut MdContext<'_, Buf>) -> Result<(), WriteError> {
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
