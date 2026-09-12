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
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError>;
}

/// Render a component with a future whose complete active state is `Send`.
pub trait WebRenderSend<L: DataLoader = NoLoader, B: WriteBuf + Send = RenderBuffer>: Send {
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send>;
}

/// Lift a send-safe synchronous component into [`WebRenderSend`].
pub struct SendSyncToAsync<T>(pub T);

impl<T, B, L> WebRenderSend<L, B> for SendSyncToAsync<T>
where
    T: WebRenderSendSync<L, B>,
    B: WriteBuf + Send,
    L: DataLoader,
{
    fn render_send(
        self,
        cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
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
/// implement [`WebRenderSendSync`] directly.
pub struct StatelessSyncToSend<T>(pub T);

impl<T, B, L> WebRenderSendSync<L, B> for StatelessSyncToSend<T>
where
    T: for<'a> WebRenderSync<NoLoader, &'a mut B> + Send,
    B: WriteBuf + Send,
    L: DataLoader,
{
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        cx.__proa_with_stateless_context(|local: &mut WebContext<NoLoader, &mut B>| {
            WebRenderSync::render(self.0, local)
        })
    }
}

/// Await a `Send` future that resolves to a send-renderable child.
pub struct AwaitSendComponent<F>(pub F);

impl<F, B, L, R> WebRenderSend<L, B> for AwaitSendComponent<F>
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
            child.render_send(cx).resolve().await
        })
    }
}

impl<B: WriteBuf + Send, L: DataLoader> WebRenderSendSync<L, B> for () {
    fn render_send_sync(self, _cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<B: WriteBuf + Send, L: DataLoader> WebRenderSend<L, B> for () {
    fn render_send(
        self,
        _cx: &mut SendWebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}

impl<B, L, T> WebRenderSendSync<L, B> for Option<T>
where
    B: WriteBuf + Send,
    L: DataLoader,
    T: WebRenderSendSync<L, B>,
{
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        if let Some(child) = self {
            child.render_send_sync(cx)?;
        }
        Ok(())
    }
}

impl<B, L, T> WebRenderSend<L, B> for Option<T>
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
                child.render_send(cx).resolve().await?;
            }
            Ok(())
        })
    }
}

macro_rules! impl_stateless_builtin_send {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl<B, L> WebRenderSendSync<L, B> for $ty
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

            impl<B, L> WebRenderSend<L, B> for $ty
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

impl<'value, B, L> WebRenderSendSync<L, B> for &'value str
where
    B: WriteBuf + Send,
    L: DataLoader,
    for<'a> &'value str: WebRenderSync<NoLoader, &'a mut B>,
{
    fn render_send_sync(self, cx: &mut SendWebContext<L, B>) -> Result<(), WriteError> {
        cx.__proa_with_stateless_context(|local| WebRenderSync::render(self, local))
    }
}

impl<'value, B, L> WebRenderSend<L, B> for &'value str
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

impl<T, B, L> DynWebRenderSend<L, B> for T
where
    T: WebRenderSend<L, B> + Send + 'static,
    B: WriteBuf + Send,
    L: DataLoader,
{
    fn render_send_dyn<'a>(
        self: Box<Self>,
        cx: &'a mut SendWebContext<L, B>,
    ) -> DynSendRenderOutcome<'a> {
        match WebRenderSend::render_send(*self, cx) {
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
    child.render_send(cx).resolve().await
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
    root.render_send(cx).resolve().await
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
