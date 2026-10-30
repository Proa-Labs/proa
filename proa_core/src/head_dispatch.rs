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

/// Return type of an optional asynchronous head hook. Return an `async move`
/// block producing `Result<(), WriteError>`; the default hook returns `NoHead`.
/// The distinction selects dispatch before application code is invoked.
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
        body_after_empty_head(child, cx)
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
        async move {
            if C::ROLE.is_page() {
                cx.require_managed_head()?;
                cx.managed_seal()?;
                if crate::managed_head::streaming(cx.render_mode())
                    && cx.head_state().document().is_some_and(|d| d.is_published())
                {
                    crate::managed_head::yield_after_publication().await;
                }
            }
            child.render(cx).resolve().await
        }
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
        crate::RenderOutcome::pending(self.render(child, cx))
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
        async move {
            // Gate before constructing the operation, not merely before polling it.
            cx.require_managed_head()?;
            let permission = cx
                .head_state()
                .document()
                .map(|d| d.begin_hook_for(std::any::type_name::<C>()))
                .transpose()?;
            let result = child.render_head(cx).await;
            let scope = match permission {
                Some(p) => Some(p.finish(result)?),
                None => {
                    result?;
                    None
                }
            };
            if C::ROLE.is_page() {
                cx.managed_seal()?;
                if crate::managed_head::streaming(cx.render_mode())
                    && cx.head_state().document().is_some_and(|d| d.is_published())
                {
                    crate::managed_head::yield_after_publication().await;
                }
            }
            let result = child.render(cx).resolve().await;
            match scope {
                Some(s) => s.finish(result),
                None => result,
            }
        }
    }
}

// Immediately completed ordinary children stay on Done. Box only the managed
// Page continuation: retaining it inline would recursively enlarge every
// ordinary component's future, even when its ROLE excludes the Page branch.
pin_project_lite::pin_project! {
    #[project = BodyFutureProj]
    enum BodyFuture<P, F> {
        Page { future: std::pin::Pin<Box<P>> },
        Ordinary { #[pin] future: F },
    }
}
impl<P, F> Future for BodyFuture<P, F>
where
    P: Future<Output = Result<(), WriteError>>,
    F: Future<Output = Result<(), WriteError>>,
{
    type Output = Result<(), WriteError>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.project() {
            BodyFutureProj::Page { future } => future.as_mut().poll(cx),
            BodyFutureProj::Ordinary { future } => future.poll(cx),
        }
    }
}

// Private dispatch plumbing, not a new application render entry point.
#[inline]
fn body_after_empty_head<L: DataLoader, B: WriteBuf, C: WebRender<L, B>>(
    child: C,
    cx: &mut WebContext<L, B>,
) -> crate::RenderOutcome<impl Future<Output = Result<(), WriteError>> + use<'_, L, B, C>> {
    if C::ROLE.is_page() {
        if let Err(error) = cx.require_managed_head() {
            return crate::RenderOutcome::Done(Err(error));
        }
        if let Err(e) = cx.managed_seal() {
            return crate::RenderOutcome::Done(Err(e));
        }
        if crate::managed_head::streaming(cx.render_mode())
            && cx.head_state().document().is_some_and(|d| d.is_published())
        {
            return crate::RenderOutcome::pending(BodyFuture::Page {
                // proa-perf-ok: only a managed Page that just published needs an owned continuation
                future: Box::pin(async move {
                    crate::managed_head::yield_after_publication().await;
                    child.render(cx).resolve().await
                }),
            });
        }
    }
    match child.render(cx) {
        crate::RenderOutcome::Done(result) => crate::RenderOutcome::Done(result),
        crate::RenderOutcome::Pending(future) => {
            crate::RenderOutcome::Pending(BodyFuture::Ordinary { future })
        }
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

/// Component role declared on the render trait and visible to generic dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentRole {
    page: bool,
}
impl ComponentRole {
    pub const COMPONENT: Self = Self { page: false };
    pub const PAGE: Self = Self { page: true };
    pub const fn is_page(self) -> bool {
        self.page
    }
}
/// The descriptor is visible through the existing render-trait bound.
/// No head method is called to obtain this information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    pub has_head: bool,
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
        has_head: has_head(strategy_of(C::render_head)),
        is_page: C::ROLE.is_page(),
    }
}

// Synchronous hooks deliberately cannot return an asynchronous operation.
mod sync_sealed {
    pub trait Operation {}
    impl Operation for super::NoHead {}
    impl Operation for Result<(), crate::WriteError> {}
}
/// Synchronous hooks return `Result<(), WriteError>`; they cannot hide an await.
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
    let head = sync_presence(C::render_head);
    if head || C::ROLE.is_page() {
        cx.require_managed_head()?;
    }
    let permission = if head {
        cx.head_state()
            .document()
            .map(|d| d.begin_hook_for(std::any::type_name::<C>()))
            .transpose()?
    } else {
        None
    };
    let result = if head {
        child.render_head(cx).finish()
    } else {
        Ok(())
    };
    let scope = match permission {
        Some(p) => Some(p.finish(result)?),
        None => {
            result?;
            None
        }
    };
    if C::ROLE.is_page() {
        cx.managed_seal()?;
    }
    let result = child.render(cx);
    match scope {
        Some(s) => s.finish(result),
        None => result,
    }
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
        if C::ROLE.is_page() {
            if let Err(e) = cx.require_managed_head() {
                return RenderOutcome::Done(Err(e));
            }
            if let Err(e) = cx.managed_seal() {
                return RenderOutcome::Done(Err(e));
            }
            if crate::managed_head::streaming(cx.render_mode())
                && cx.head_state().document().is_some_and(|d| d.is_published())
            {
                return RenderOutcome::pending(BodyFuture::Page {
                    // proa-perf-ok: only a managed Page that just published needs an owned continuation
                    future: Box::pin(async move {
                        crate::managed_head::yield_after_publication().await;
                        child.render_send(cx).resolve().await
                    }),
                });
            }
        }
        match child.render_send(cx) {
            RenderOutcome::Done(result) => RenderOutcome::Done(result),
            RenderOutcome::Pending(future) => {
                RenderOutcome::Pending(BodyFuture::Ordinary { future })
            }
        }
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
        RenderOutcome::pending(async move {
            cx.require_managed_head()?;
            let permission = cx
                .head_state()
                .document()
                .map(|d| d.begin_hook_for(std::any::type_name::<C>()))
                .transpose()?;
            let result = child.render_head_send(cx).await;
            let scope = match permission {
                Some(p) => Some(p.finish(result)?),
                None => {
                    result?;
                    None
                }
            };
            if C::ROLE.is_page() {
                cx.managed_seal()?;
                if crate::managed_head::streaming(cx.render_mode())
                    && cx.head_state().document().is_some_and(|d| d.is_published())
                {
                    crate::managed_head::yield_after_publication().await;
                }
            }
            let result = child.render_send(cx).resolve().await;
            match scope {
                Some(s) => s.finish(result),
                None => result,
            }
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
    let head = send_sync_presence(C::render_head_send_sync);
    if head || C::ROLE.is_page() {
        cx.require_managed_head()?;
    }
    let permission = if head {
        cx.head_state()
            .document()
            .map(|d| d.begin_hook_for(std::any::type_name::<C>()))
            .transpose()?
    } else {
        None
    };
    let result = if head {
        child.render_head_send_sync(cx).finish()
    } else {
        Ok(())
    };
    let scope = match permission {
        Some(p) => Some(p.finish(result)?),
        None => {
            result?;
            None
        }
    };
    if C::ROLE.is_page() {
        cx.managed_seal()?;
    }
    let result = child.render_send_sync(cx);
    match scope {
        Some(s) => s.finish(result),
        None => result,
    }
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
    if C::ROLE.is_page() || sync_presence(C::render_head) {
        Err(WriteError::render(
            "stateless adapter cannot carry a Page or head provider",
        ))
    } else {
        Ok(())
    }
}
