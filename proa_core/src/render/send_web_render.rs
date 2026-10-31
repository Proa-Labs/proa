// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compiler-proved `Send` HTML rendering surface.
//!
//! This is intentionally a companion to [`crate::WebRender`], not a stronger
//! blanket bound on it. Local components remain valid; components that opt in
//! here prove that both their value and every pending continuation can migrate
//! safely between executor threads.

use std::future::Future;
use std::pin::Pin;

use crate::render::{DataLoader, NoLoader, RenderOutcome, WebRenderSync};
use crate::web::{SendWebContext, WebContext};
use crate::{RenderBuffer, WriteBuf, WriteError};

/// Synchronous companion used by send-capable component trees.
pub trait WebRenderSendSync<L: DataLoader = NoLoader, B: WriteBuf + Send = RenderBuffer>:
    Send
{
    /// Head providers declare `LAYOUT` or `PAGE`. Only `PAGE` seals metadata.
    const ROLE: crate::head_dispatch::ComponentRole =
        crate::head_dispatch::ComponentRole::COMPONENT;
    fn render_head_send_sync(
        &self,
        _cx: &mut SendWebContext<L, B>,
    ) -> impl crate::head_dispatch::SyncHeadOperation {
        crate::head_dispatch::NoHead
    }

    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError>;
}

/// Render a component with a future whose complete active state is `Send`.
pub trait WebRenderSend<L: DataLoader = NoLoader, B: WriteBuf + Send = RenderBuffer>: Send {
    /// Head providers declare `LAYOUT` or `PAGE`. Only `PAGE` seals metadata.
    const ROLE: crate::head_dispatch::ComponentRole =
        crate::head_dispatch::ComponentRole::COMPONENT;
    /// Override with `async fn render_head_send(...) -> Result<(), WriteError>`
    /// when its captured values are Send. The explicit Future bound keeps the
    /// Send guarantee visible to generic dispatch. Ordinary components skip it.
    /// A custom hook must declare `LAYOUT` or `PAGE`; run `proa lint` to check.
    ///
    /// ```compile_fail
    /// use proa_core::{ComponentRole, RenderOutcome, SendWebContext, WebRenderSend, WriteError};
    /// use std::future::{Future, Ready};
    /// struct Page;
    /// impl WebRenderSend for Page {
    ///     const ROLE: ComponentRole = ComponentRole::PAGE;
    ///     async fn render_head_send(&self, _: &mut SendWebContext) -> Result<(), WriteError> {
    ///         let local = std::rc::Rc::new(1);
    ///         std::future::pending::<()>().await;
    ///         drop(local);
    ///         Ok(())
    ///     }
    ///     fn render_send(self, _: &mut SendWebContext)
    ///         -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send>
    ///     { RenderOutcome::<Ready<_>>::Done(Ok(())) }
    /// }
    /// ```
    fn render_head_send(
        &self,
        _cx: &mut SendWebContext<L, B>,
    ) -> impl Future<Output = Result<(), WriteError>> + Send {
        std::future::ready(Ok(()))
    }

    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send>;
}

/// Lift a send-safe synchronous component into [`WebRenderSend`].
pub struct SendSyncToAsync<T>(pub T);

impl<T, L, B> WebRenderSend<L, B> for SendSyncToAsync<T>
where
    T: WebRenderSendSync<L, B>,
    B: WriteBuf + Send,
    L: DataLoader,
{
    /// Head providers declare `LAYOUT` or `PAGE`; a real hook with the default
    /// `COMPONENT` role fails during code generation. Only `PAGE` seals metadata.
    const ROLE: crate::head_dispatch::ComponentRole = T::ROLE;
    fn render_head_send(
        &self,
        cx: &mut SendWebContext<L, B>,
    ) -> impl Future<Output = Result<(), WriteError>> + Send {
        use crate::head_dispatch::SyncHeadOperation;
        self.0.render_head_send_sync(cx).into_async()
    }
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        crate::head_dispatch::send_sync_presence(T::render_head_send_sync);
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            self.0.render_send_sync(cx),
        )
    }
}

/// Explicit adapter for a context-independent legacy synchronous component.
///
/// The wrapped `WebRenderSync` receives a stateless temporary `WebContext` and
/// may only write output or use output-mode escaping. Components that inspect
/// request state, register artifacts, or install stream callbacks must instead
/// implement [`WebRenderSendSync`] directly. Page and Layout roles are rejected,
/// even when they have no head hook.
pub struct StatelessSyncToSend<T>(pub T);

impl<T, L, B> WebRenderSendSync<L, B> for StatelessSyncToSend<T>
where
    T: for<'a> WebRenderSync<NoLoader, &'a mut B> + Send,
    B: WriteBuf + Send,
    L: DataLoader,
{
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        crate::head_dispatch::ensure_stateless::<B, T>()?;
        cx.__proa_with_stateless_context(|local: &mut WebContext<NoLoader, &mut B>| {
            WebRenderSync::render(self.0, local)
        })
    }
}

/// Await a `Send` future that resolves to a send-renderable child.
pub struct AwaitSendComponent<F>(pub F);

impl<F, L, B, R> WebRenderSend<L, B> for AwaitSendComponent<F>
where
    F: Future<Output = R> + Send,
    B: WriteBuf + Send,
    L: DataLoader,
    R: WebRenderSend<L, B>,
{
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            let child = self.0.await;
            crate::head_dispatch::send_begin(child, cx).resolve().await
        })
    }
}

impl<L: DataLoader, B: WriteBuf + Send> WebRenderSendSync<L, B> for () {
    fn render_send_sync(self, _cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<L: DataLoader, B: WriteBuf + Send> WebRenderSend<L, B> for () {
    fn render_send(
        self,
        _cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}

impl<L, B, T> WebRenderSendSync<L, B> for Option<T>
where
    B: WriteBuf + Send,
    L: DataLoader,
    T: WebRenderSendSync<L, B>,
{
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        if let Some(child) = self {
            crate::head_dispatch::send_sync(child, cx)?;
        }
        Ok(())
    }
}

impl<L, B, T> WebRenderSend<L, B> for Option<T>
where
    B: WriteBuf + Send,
    L: DataLoader,
    T: WebRenderSend<L, B>,
{
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            if let Some(child) = self {
                crate::head_dispatch::send_begin(child, cx)
                    .resolve()
                    .await?;
            }
            Ok(())
        })
    }
}

macro_rules! impl_stateless_builtin_send {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl<L, B> WebRenderSendSync<L, B> for $ty
            where
                B: WriteBuf + Send,
                L: DataLoader,
                for<'a> $ty: WebRenderSync<NoLoader, &'a mut B>,
            {
                #[inline(always)]
                fn render_send_sync(
                    self,
                    cx: &mut SendWebContext<L, B>,
                ) -> Result<(), WriteError> {
                    cx.__proa_with_stateless_context(|local| {
                        WebRenderSync::render(self, local)
                    })
                }
            }

            impl<L, B> WebRenderSend<L, B> for $ty
            where
                B: WriteBuf + Send,
                L: DataLoader,
                for<'a> $ty: WebRenderSync<NoLoader, &'a mut B>,
            {
                #[inline(always)]
                fn render_send(
                    self,
                    cx: &mut SendWebContext<L, B>,
                ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
                    RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
                        self.render_send_sync(cx),
                    )
                }
            }
        )+
    };
}

impl_stateless_builtin_send!(
    String, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64,
);

impl<'value, L, B> WebRenderSendSync<L, B> for &'value str
where
    B: WriteBuf + Send,
    L: DataLoader,
    for<'a> &'value str: WebRenderSync<NoLoader, &'a mut B>,
{
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        cx.__proa_with_stateless_context(|local| WebRenderSync::render(self, local))
    }
}

impl<'value, L, B> WebRenderSend<L, B> for &'value str
where
    B: WriteBuf + Send,
    L: DataLoader,
    for<'a> &'value str: WebRenderSync<NoLoader, &'a mut B>,
{
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(self.render_send_sync(cx))
    }
}

/// Boxed-future outcome at a send-capable dyn-erasure boundary.
pub enum DynSendRenderOutcome<'a> {
    Done(Result<(), WriteError>),
    Pending(Pin<Box<dyn Future<Output = Result<(), WriteError>> + Send + 'a>>),
}

/// Dyn-compatible erasure for [`WebRenderSend`].
pub trait DynWebRenderSend<L: DataLoader = NoLoader, B: WriteBuf + Send = RenderBuffer>:
    Send
{
    fn render_send_dyn<'a>(
        self: Box<Self>,
        cx: &'a mut SendWebContext<L, B>,
    ) -> DynSendRenderOutcome<'a>;
}

impl<T, L, B> DynWebRenderSend<L, B> for T
where
    T: WebRenderSend<L, B> + Send + 'static,
    B: WriteBuf + Send,
    L: DataLoader,
{
    fn render_send_dyn<'a>(
        self: Box<Self>,
        cx: &'a mut SendWebContext<L, B>,
    ) -> DynSendRenderOutcome<'a> {
        match crate::head_dispatch::send_begin(*self, cx) {
            RenderOutcome::Done(result) => DynSendRenderOutcome::Done(result),
            RenderOutcome::Pending(future) => DynSendRenderOutcome::Pending(Box::pin(future)),
        }
    }
}

pub async fn drive_dyn_send_render(outcome: DynSendRenderOutcome<'_>) -> Result<(), WriteError> {
    match outcome {
        DynSendRenderOutcome::Done(result) => result,
        DynSendRenderOutcome::Pending(future) => future.await,
    }
}

pub async fn render_send_child<B, L, R>(
    cx: &mut SendWebContext<L, B>,
    child: R,
) -> Result<(), WriteError>
where
    B: WriteBuf + Send,
    L: DataLoader,
    R: WebRenderSend<L, B>,
{
    crate::head_dispatch::send_begin(child, cx).resolve().await
}

pub async fn render_in_send_context<B, L, R>(
    cx: &mut SendWebContext<L, B>,
    root: R,
) -> Result<(), WriteError>
where
    B: WriteBuf + Send,
    L: DataLoader,
    R: WebRenderSend<L, B>,
{
    crate::head_dispatch::send_begin(root, cx).resolve().await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Yielding;

    impl WebRenderSend for Yielding {
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            RenderOutcome::pending(async move {
                std::future::ready(()).await;
                cx.out.extend_static(b"send")
            })
        }
    }

    fn assert_send<T: Send>(_: T) {}

    #[test]
    fn erased_pending_future_keeps_send_proof() {
        let component: Box<dyn DynWebRenderSend> = Box::new(Yielding);
        let mut cx = SendWebContext::new();
        let outcome = component.render_send_dyn(&mut cx);
        if let DynSendRenderOutcome::Pending(future) = outcome {
            assert_send(future);
        } else {
            panic!("expected pending send render");
        }
    }
}
