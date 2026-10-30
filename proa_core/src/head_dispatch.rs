// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Type-directed component lifecycle dispatch. Ordinary tags never enter this path.
use crate::{DataLoader, WebContext, WebRender, WriteBuf, WriteError};
use std::future::{Future, IntoFuture, Ready};

mod sealed {
    pub trait Operation {}
    impl Operation for super::NoHead {}
    impl<F: std::future::Future<Output = Result<(), crate::WriteError>>> Operation for F {}
}

/// A head hook identifies the document's Page. It completes before the Page
/// body runs, sealing metadata inherited from [`crate::HeadConfig`]. Ordinary
/// components inherit [`NoHead`] and bypass head dispatch entirely.
///
/// Write an `async fn` returning `Result<(), WriteError>` to mutate the context
/// directly. Its future implements this trait automatically. Rust's
/// `refining_impl_trait` lint needs a method-scoped allowance because this trait
/// also accepts the non-Future absence marker; no async-trait boxing is used.
///
/// ```
/// #![deny(warnings)]
/// use proa_core::{RenderOutcome, WebContext, WebRender, WriteError};
/// use std::future::{Future, Ready};
/// struct Page { title: &'static str }
/// impl WebRender for Page {
///     #[allow(refining_impl_trait)]
///     async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
///         let title = std::future::ready(self.title).await;
///         cx.head_mut().title(title)?;
///         Ok(())
///     }
///     fn render(self, _: &mut WebContext)
///         -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
///     {
///         RenderOutcome::<Ready<_>>::Done(Ok(()))
///     }
/// }
/// let _page = Page { title: "Products" };
/// ```
///
/// Returning `impl HeadOperation` with an explicit future remains supported.
/// A Page using only defaults supplies an empty hook returning `Ok(())`.
pub trait HeadOperation: sealed::Operation + IntoFuture<Output = Result<(), WriteError>> {
    type Strategy: Strategy + SendStrategy;
    fn strategy() -> Self::Strategy;
}

/// A zero-sized absence marker, deliberately not a Future.
pub struct NoHead;
impl IntoFuture for NoHead {
    type Output = Result<(), WriteError>;
    type IntoFuture = Ready<Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        std::future::ready(Ok(()))
    }
}
impl HeadOperation for NoHead {
    type Strategy = BodyOnly;
    fn strategy() -> BodyOnly {
        BodyOnly
    }
}
impl<F: Future<Output = Result<(), WriteError>>> HeadOperation for F {
    type Strategy = WithHead;
    fn strategy() -> WithHead {
        WithHead
    }
}

pub struct BodyOnly;
pub struct WithHead;
pub trait Strategy {
    const HAS_HEAD: bool;
    fn begin<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut WebContext<L, B>,
    ) -> crate::RenderOutcome<impl Future<Output = Result<(), WriteError>> + use<'a, Self, L, B, C>>
    where
        L: DataLoader + 'a,
        B: WriteBuf + 'a,
        C: WebRender<L, B> + 'a;
    fn render<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut WebContext<L, B>,
    ) -> impl Future<Output = Result<(), WriteError>> + use<'a, Self, L, B, C>
    where
        L: DataLoader + 'a,
        B: WriteBuf + 'a,
        C: WebRender<L, B> + 'a;
}
impl Strategy for BodyOnly {
    const HAS_HEAD: bool = false;
    fn begin<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut WebContext<L, B>,
    ) -> crate::RenderOutcome<impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>>
    where
        L: DataLoader + 'a,
        B: WriteBuf + 'a,
        C: WebRender<L, B> + 'a,
    {
        child.render(cx)
    }
    fn render<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut WebContext<L, B>,
    ) -> impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>
    where
        L: DataLoader + 'a,
        B: WriteBuf + 'a,
        C: WebRender<L, B> + 'a,
    {
        async move { child.render(cx).resolve().await }
    }
}
impl Strategy for WithHead {
    const HAS_HEAD: bool = true;
    fn begin<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut WebContext<L, B>,
    ) -> crate::RenderOutcome<impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>>
    where
        L: DataLoader + 'a,
        B: WriteBuf + 'a,
        C: WebRender<L, B> + 'a,
    {
        // Select the Page and own its cancellation guard before constructing
        // application code or returning an unpolled continuation.
        let document = match cx.require_managed_head() {
            Ok(document) => document,
            Err(error) => return crate::RenderOutcome::Done(Err(error)),
        };
        let permission = match document.begin_hook_for(std::any::type_name::<C>()) {
            Ok(permission) => permission,
            Err(error) => return crate::RenderOutcome::Done(Err(error)),
        };
        crate::RenderOutcome::pending(async move {
            let scope = permission.finish(child.render_head(cx).await)?;
            cx.managed_seal()?;
            if crate::managed_head::streaming(cx.render_mode()) && document.is_published() {
                crate::managed_head::yield_after_publication().await;
            }
            scope.finish(child.render(cx).resolve().await)
        })
    }
    fn render<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut WebContext<L, B>,
    ) -> impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>
    where
        L: DataLoader + 'a,
        B: WriteBuf + 'a,
        C: WebRender<L, B> + 'a,
    {
        Strategy::begin(self, child, cx).resolve()
    }
}

/// Infer the operation type from the method item without calling it.
#[inline]
pub fn strategy_of<'a, L, B, C, H>(
    _: impl FnOnce(&'a C, &'a mut WebContext<L, B>) -> H,
) -> H::Strategy
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: 'a,
    H: HeadOperation,
{
    H::strategy()
}

/// Page identity is inferred from the head operation type without calling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    pub is_page: bool,
}
pub fn descriptor<'a, L, B, C>() -> Descriptor
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: WebRender<L, B> + 'a,
{
    fn has_head<S: Strategy>(_: S) -> bool {
        S::HAS_HEAD
    }
    Descriptor {
        is_page: has_head(strategy_of(C::render_head)),
    }
}

// Synchronous hooks deliberately cannot return an asynchronous operation.
mod sync_sealed {
    pub trait Operation {}
    impl Operation for super::NoHead {}
    impl Operation for Result<(), crate::WriteError> {}
}
/// Synchronous Page hooks mutate the context directly and return a result;
/// they cannot hide an await. Keep `impl SyncHeadOperation` in the signature
/// to preserve the absence marker without refining the trait's return type.
///
/// ```
/// #![deny(warnings)]
/// use proa_core::{SyncHeadOperation, WebContext, WebRenderSync, WriteError};
/// struct Page { title: &'static str }
/// impl WebRenderSync for Page {
///     fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
///         cx.head_mut().title(self.title)?;
///         Ok(())
///     }
///     fn render(self, _: &mut WebContext) -> Result<(), WriteError> { Ok(()) }
/// }
/// let _page = Page { title: "Products" };
/// ```
///
/// ```compile_fail
/// use proa_core::{SyncHeadOperation, WriteError};
/// fn invalid_sync_hook() -> impl SyncHeadOperation {
///     async { Ok::<(), WriteError>(()) }
/// }
/// ```
pub trait SyncHeadOperation: sync_sealed::Operation {
    const HAS_HEAD: bool;
    type Async: SendHeadOperation;
    fn finish(self) -> Result<(), WriteError>;
    fn into_async(self) -> Self::Async;
}
impl SyncHeadOperation for NoHead {
    const HAS_HEAD: bool = false;
    type Async = NoHead;
    fn finish(self) -> Result<(), WriteError> {
        Ok(())
    }
    fn into_async(self) -> NoHead {
        self
    }
}
impl SyncHeadOperation for Result<(), WriteError> {
    const HAS_HEAD: bool = true;
    type Async = std::future::Ready<Self>;
    fn finish(self) -> Self {
        self
    }
    fn into_async(self) -> Self::Async {
        std::future::ready(self)
    }
}
fn sync_presence<'a, L: DataLoader + 'a, B: WriteBuf + 'a, C: 'a, H: SyncHeadOperation>(
    _: impl FnOnce(&'a C, &'a mut WebContext<L, B>) -> H,
) -> bool {
    H::HAS_HEAD
}
#[doc(hidden)]
#[inline]
pub fn sync<'a, L: DataLoader + 'a, B: WriteBuf + 'a, C: crate::WebRenderSync<L, B> + 'a>(
    child: C,
    cx: &'a mut WebContext<L, B>,
) -> Result<(), WriteError> {
    if !sync_presence(C::render_head) {
        return child.render(cx);
    }
    let document = cx.require_managed_head()?;
    let permission = document.begin_hook_for(std::any::type_name::<C>())?;
    let scope = permission.finish(child.render_head(cx).finish())?;
    cx.managed_seal()?;
    scope.finish(child.render(cx))
}

/// Used to reject a Page fallback before invoking either it or its protected child.
pub(crate) fn is_sync_page<'a, L, B, C>() -> bool
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: crate::WebRenderSync<L, B> + 'a,
{
    sync_presence(C::render_head)
}

// A Send future is required, but the component itself remains Send, not Sync.
/// A head hook used by the Send renderer must retain only Send values across awaits.
///
/// ```compile_fail
/// use proa_core::{SendHeadOperation, WriteError};
/// fn invalid_send_hook() -> impl SendHeadOperation {
///     async {
///         let value = std::rc::Rc::new(1);
///         std::future::pending::<()>().await;
///         drop(value);
///         Ok::<(), WriteError>(())
///     }
/// }
/// ```
pub trait SendHeadOperation: HeadOperation<IntoFuture: Send> {}
impl<O: HeadOperation<IntoFuture: Send>> SendHeadOperation for O {}
use crate::{RenderOutcome, SendWebContext, WebRenderSend, WebRenderSendSync};
pub trait SendStrategy {
    fn begin<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send + use<'a, Self, L, B, C>>
    where
        L: DataLoader + 'a,
        B: WriteBuf + Send + 'a,
        C: WebRenderSend<L, B> + 'a;
}
impl SendStrategy for BodyOnly {
    fn begin<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send + use<'a, L, B, C>>
    where
        L: DataLoader + 'a,
        B: WriteBuf + Send + 'a,
        C: WebRenderSend<L, B> + 'a,
    {
        child.render_send(cx)
    }
}
impl SendStrategy for WithHead {
    fn begin<'a, L, B, C>(
        self,
        child: C,
        cx: &'a mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send + use<'a, L, B, C>>
    where
        L: DataLoader + 'a,
        B: WriteBuf + Send + 'a,
        C: WebRenderSend<L, B> + 'a,
    {
        let document = match cx.require_managed_head() {
            Ok(document) => document,
            Err(error) => return RenderOutcome::Done(Err(error)),
        };
        let permission = match document.begin_hook_for(std::any::type_name::<C>()) {
            Ok(permission) => permission,
            Err(error) => return RenderOutcome::Done(Err(error)),
        };
        RenderOutcome::pending(async move {
            let scope = permission.finish(child.render_head_send(cx).await)?;
            cx.managed_seal()?;
            if crate::managed_head::streaming(cx.render_mode()) && document.is_published() {
                crate::managed_head::yield_after_publication().await;
            }
            scope.finish(child.render_send(cx).resolve().await)
        })
    }
}
fn send_strategy<'a, L, B, C, H>(
    _: impl FnOnce(&'a C, &'a mut SendWebContext<L, B>) -> H,
) -> H::Strategy
where
    L: DataLoader + 'a,
    B: WriteBuf + Send + 'a,
    C: 'a,
    H: SendHeadOperation,
{
    H::strategy()
}
#[doc(hidden)]
pub fn send_begin<'a, L, B, C>(
    child: C,
    cx: &'a mut SendWebContext<L, B>,
) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send + use<'a, L, B, C>>
where
    L: DataLoader + 'a,
    B: WriteBuf + Send + 'a,
    C: WebRenderSend<L, B> + 'a,
{
    SendStrategy::begin(send_strategy(C::render_head_send), child, cx)
}
fn send_sync_presence<
    'a,
    L: DataLoader + 'a,
    B: WriteBuf + Send + 'a,
    C: 'a,
    H: SyncHeadOperation,
>(
    _: impl FnOnce(&'a C, &'a mut SendWebContext<L, B>) -> H,
) -> bool {
    H::HAS_HEAD
}
#[doc(hidden)]
pub fn send_sync<'a, L, B, C>(child: C, cx: &'a mut SendWebContext<L, B>) -> Result<(), WriteError>
where
    L: DataLoader + 'a,
    B: WriteBuf + Send + 'a,
    C: WebRenderSendSync<L, B> + 'a,
{
    if !send_sync_presence(C::render_head_send_sync) {
        return child.render_send_sync(cx);
    }
    let document = cx.require_managed_head()?;
    let permission = document.begin_hook_for(std::any::type_name::<C>())?;
    let scope = permission.finish(child.render_head_send_sync(cx).finish())?;
    cx.managed_seal()?;
    scope.finish(child.render_send_sync(cx))
}

#[doc(hidden)]
pub fn begin<'a, L, B, C>(
    child: C,
    cx: &'a mut WebContext<L, B>,
) -> crate::RenderOutcome<impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>>
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: WebRender<L, B> + 'a,
{
    Strategy::begin(strategy_of(C::render_head), child, cx)
}

#[doc(hidden)]
pub fn ensure_stateless<
    'a,
    B: WriteBuf + 'a,
    C: crate::WebRenderSync<crate::NoLoader, &'a mut B> + 'a,
>() -> Result<(), WriteError> {
    if sync_presence(C::render_head) {
        Err(WriteError::render("stateless adapter cannot carry a Page"))
    } else {
        Ok(())
    }
}
