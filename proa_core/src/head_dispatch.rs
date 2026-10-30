// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Role-directed component lifecycle dispatch. Ordinary tags never enter this path.
use crate::{
    DataLoader, RenderOutcome, SendWebContext, WebContext, WebRender, WebRenderSend,
    WebRenderSendSync, WriteBuf, WriteError,
};
use std::future::Future;

/// Absence marker for synchronous hooks. Async hooks use native futures and
/// their render trait's role selects whether to invoke the hook.
pub struct NoHead;

// Box only metadata boundaries. Keeping a provider's head/body continuation
// inline would recursively enlarge ordinary component futures, even when their
// constant role excludes that branch. Ordinary pending bodies stay inline.
pin_project_lite::pin_project! {
    #[project = BoundaryFutureProj]
    enum BoundaryFuture<H, F> {
        Provider { future: std::pin::Pin<Box<H>> },
        Ordinary { #[pin] future: F },
    }
}
impl<H, F> Future for BoundaryFuture<H, F>
where
    H: Future<Output = Result<(), WriteError>>,
    F: Future<Output = Result<(), WriteError>>,
{
    type Output = Result<(), WriteError>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.project() {
            BoundaryFutureProj::Provider { future } => future.as_mut().poll(cx),
            BoundaryFutureProj::Ordinary { future } => future.poll(cx),
        }
    }
}

#[doc(hidden)]
#[inline]
pub fn begin<'a, L, B, C>(
    child: C,
    cx: &'a mut WebContext<L, B>,
) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>>
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: WebRender<L, B> + 'a,
{
    if C::ROLE.is_layout() || C::ROLE.is_page() {
        return match start_provider(child, cx) {
            Ok(future) => RenderOutcome::pending(BoundaryFuture::Provider {
                // proa-perf-ok: only explicit metadata boundaries own a boxed continuation
                future: Box::pin(future),
            }),
            Err(error) => RenderOutcome::Done(Err(error)),
        };
    }
    // COMPONENT never constructs or polls a head future, including the default.
    match child.render(cx) {
        RenderOutcome::Done(result) => RenderOutcome::Done(result),
        RenderOutcome::Pending(future) => {
            RenderOutcome::Pending(BoundaryFuture::Ordinary { future })
        }
    }
}

// Claim permission before returning any future, including the direct
// render_child helper. Ordinary body futures remain lazy and carry no guard.
fn start_provider<'a, L, B, C>(
    child: C,
    cx: &'a mut WebContext<L, B>,
) -> Result<impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>, WriteError>
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: WebRender<L, B> + 'a,
{
    let permission = cx
        .require_managed_head()?
        .begin_hook_for(std::any::type_name::<C>())?;
    Ok(async move {
        let scope = permission.finish(child.render_head(cx).await)?;
        if C::ROLE.is_page() {
            cx.managed_seal()?;
            if crate::managed_head::streaming(cx.render_mode())
                && cx.head_state().document().is_some_and(|d| d.is_published())
            {
                crate::managed_head::yield_after_publication().await;
            }
        }
        scope.finish(child.render(cx).resolve().await)
    })
}

#[doc(hidden)]
#[inline]
pub fn render<'a, L, B, C>(
    child: C,
    cx: &'a mut WebContext<L, B>,
) -> impl Future<Output = Result<(), WriteError>> + use<'a, L, B, C>
where
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: WebRender<L, B> + 'a,
{
    if C::ROLE.is_layout() || C::ROLE.is_page() {
        let future = start_provider(child, cx);
        return BoundaryFuture::Provider {
            // proa-perf-ok: guard and head/body state belong only to metadata boundaries
            future: Box::pin(async move { future?.await }),
        };
    }
    BoundaryFuture::Ordinary {
        future: async move { child.render(cx).resolve().await },
    }
}

#[doc(hidden)]
#[inline]
pub fn send_begin<'a, L, B, C>(
    child: C,
    cx: &'a mut SendWebContext<L, B>,
) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send + use<'a, L, B, C>>
where
    L: DataLoader + 'a,
    B: WriteBuf + Send + 'a,
    C: WebRenderSend<L, B> + 'a,
{
    if C::ROLE.is_layout() || C::ROLE.is_page() {
        let permission = match cx
            .require_managed_head()
            .and_then(|document| document.begin_hook_for(std::any::type_name::<C>()))
        {
            Ok(permission) => permission,
            Err(error) => return RenderOutcome::Done(Err(error)),
        };
        return RenderOutcome::pending(BoundaryFuture::Provider {
            // proa-perf-ok: only explicit metadata boundaries own a boxed continuation
            future: Box::pin(async move {
                let scope = permission.finish(child.render_head_send(cx).await)?;
                if C::ROLE.is_page() {
                    cx.managed_seal()?;
                    if crate::managed_head::streaming(cx.render_mode())
                        && cx.head_state().document().is_some_and(|d| d.is_published())
                    {
                        crate::managed_head::yield_after_publication().await;
                    }
                }
                scope.finish(child.render_send(cx).resolve().await)
            }),
        });
    }
    match child.render_send(cx) {
        RenderOutcome::Done(result) => RenderOutcome::Done(result),
        RenderOutcome::Pending(future) => {
            RenderOutcome::Pending(BoundaryFuture::Ordinary { future })
        }
    }
}

/// Component intent declared on the render trait and visible to generic dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentRole {
    kind: RoleKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoleKind {
    Component,
    Layout,
    Page,
}
impl ComponentRole {
    /// Ordinary body-only component; a real head hook requires an explicit role.
    pub const COMPONENT: Self = Self {
        kind: RoleKind::Component,
    };
    /// Ancestor that may contribute metadata and render a selected Page.
    pub const LAYOUT: Self = Self {
        kind: RoleKind::Layout,
    };
    /// Selected Page: seal inherited metadata after its optional head hook.
    pub const PAGE: Self = Self {
        kind: RoleKind::Page,
    };
    pub const fn is_page(self) -> bool {
        matches!(self.kind, RoleKind::Page)
    }
    pub const fn is_layout(self) -> bool {
        matches!(self.kind, RoleKind::Layout)
    }
    const fn assert_head_role(self, has_head: bool) {
        assert!(
            !has_head || self.is_layout() || self.is_page(),
            "render_head requires an explicit role: set const ROLE to ComponentRole::LAYOUT or ComponentRole::PAGE"
        );
    }
}

/// Role-directed dispatch information; `runs_head` includes an inherited
/// default hook on an explicit layout or Page, not override detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    pub runs_head: bool,
    pub is_page: bool,
    pub is_layout: bool,
}
pub fn descriptor<L, B, C>() -> Descriptor
where
    L: DataLoader,
    B: WriteBuf,
    C: WebRender<L, B>,
{
    Descriptor {
        runs_head: C::ROLE.is_layout() || C::ROLE.is_page(),
        is_page: C::ROLE.is_page(),
        is_layout: C::ROLE.is_layout(),
    }
}
// Synchronous hooks deliberately cannot return an asynchronous operation.
mod sync_sealed {
    pub trait Operation {}
    impl Operation for super::NoHead {}
    impl Operation for Result<(), crate::WriteError> {}
}
/// Synchronous layout and Page hooks mutate the context directly and return a result;
/// they cannot hide an await. Keep `impl SyncHeadOperation` in the signature
/// to preserve the absence marker without refining the trait's return type.
///
/// ```
/// #![deny(warnings)]
/// use proa_core::{SyncHeadOperation, WebContext, WebRenderSync, WriteError};
/// struct Page { title: &'static str }
/// impl WebRenderSync for Page {
///     const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::PAGE;
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
    type Async: Future<Output = Result<(), WriteError>> + Send;
    fn finish(self) -> Result<(), WriteError>;
    fn into_async(self) -> Self::Async;
}
impl SyncHeadOperation for NoHead {
    const HAS_HEAD: bool = false;
    type Async = std::future::Ready<Result<(), WriteError>>;
    fn finish(self) -> Result<(), WriteError> {
        Ok(())
    }
    fn into_async(self) -> Self::Async {
        std::future::ready(Ok(()))
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
pub(crate) fn sync_presence<
    'a,
    L: DataLoader + 'a,
    B: WriteBuf + 'a,
    C: crate::WebRenderSync<L, B> + 'a,
    H: SyncHeadOperation,
>(
    _: impl FnOnce(&'a C, &'a mut WebContext<L, B>) -> H,
) -> bool {
    const {
        C::ROLE.assert_head_role(H::HAS_HEAD);
    }
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
            if C::ROLE.is_page() {
                Some(
                    cx.require_managed_head()?
                        .track_page(std::any::type_name::<C>()),
                )
            } else {
                None
            }
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

pub(crate) fn send_sync_presence<
    'a,
    L: DataLoader + 'a,
    B: WriteBuf + Send + 'a,
    C: WebRenderSendSync<L, B> + 'a,
    H: SyncHeadOperation,
>(
    _: impl FnOnce(&'a C, &'a mut SendWebContext<L, B>) -> H,
) -> bool {
    const {
        C::ROLE.assert_head_role(H::HAS_HEAD);
    }
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
            if C::ROLE.is_page() {
                Some(
                    cx.require_managed_head()?
                        .track_page(std::any::type_name::<C>()),
                )
            } else {
                None
            }
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
