// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Dyn-compatible erasure adapter for [`WebRender`].
//!
//! [`WebRender`] is the static-dispatch component trait: each implementor
//! produces its own concrete future type, which the compiler tracks
//! through the call stack. That works great for component composition
//! (no boxing, no virtual dispatch) but it is not dyn-compatible, so
//! consumers that need a `Box<dyn ...>` shape (route registration,
//! heterogeneous component collections, FFI boundaries) cannot use
//! `Box<dyn WebRender<L, B>>` directly.
//!
//! [`DynWebRender`] is the boxed-future companion: same render concept,
//! but with a method receiver and return type chosen for dyn safety.
//! A blanket impl bridges any `T: WebRender<L, B>` into [`DynWebRender`],
//! so component authors keep writing the ergonomic `WebRender` trait
//! and only the framework / boxing code touches [`DynWebRender`].
//!
//! See [`DynRenderOutcome`] for the RPITIT explanation and
//! [`DynWebRender`] for the receiver / return-type details.
//!
//! Parallel to the [`crate::DataLoader`] / [`crate::DynDataLoader`] split
//! that already lives in this crate, which solves the same problem for
//! data loaders.

use std::future::Future;
use std::pin::Pin;

use crate::render::web_render::{SyncToAsync, WebRender, WebRenderSync};
use crate::render::{NoLoader, RenderOutcome};
use crate::shared::write_buf::{RenderBuffer, WriteBuf, WriteError};
use crate::web::WebContext;
use crate::DataLoader;

/// Dyn-compatible erasure adapter for synchronous components.
///
/// This is the sync-only counterpart to [`DynWebRender`]. It exists so
/// genuinely runtime-selected, heterogeneous sync children can still render
/// through [`WebRenderSync`] without pretending to be async. Normal component
/// composition should remain generic and statically dispatched; callers opt
/// into this trait through [`BoxedWebRenderSync`] only at a real type-erasure
/// boundary.
pub trait DynWebRenderSync<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer>: Send {
    /// Consume the boxed component and render it through one vtable call.
    fn render_sync_dyn(self: Box<Self>, ctx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}

impl<T, L, B> DynWebRenderSync<L, B> for T
where
    T: WebRenderSync<L, B> + Send + 'static,
    B: WriteBuf,
    L: DataLoader,
{
    fn render_sync_dyn(self: Box<Self>, ctx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(*self, ctx)
    }
}

/// Explicit boxed escape hatch for heterogeneous synchronous children.
///
/// Constructing this wrapper stores the component in one `Box` (non-zero-sized
/// components normally require one heap allocation), and rendering uses one
/// vtable call. In exchange, different concrete component types can be
/// selected at runtime while the wrapper itself implements both
/// [`WebRenderSync`] and [`WebRender`]. Keep ordinary component trees generic:
/// this type is intended for plugin registries, runtime component catalogs,
/// and branches whose concrete child type genuinely cannot be known at
/// compile time. Components must be `Send + 'static`; the render future is
/// allowed to be `!Send`, matching [`WebRender`].
#[must_use = "a boxed component does nothing unless it is rendered"]
pub struct BoxedWebRenderSync<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    inner: Box<dyn DynWebRenderSync<L, B>>,
}

impl<L: DataLoader, B: WriteBuf> BoxedWebRenderSync<L, B> {
    /// Erase one synchronous component.
    pub fn new<T>(component: T) -> Self
    where
        T: WebRenderSync<L, B> + Send + 'static,
    {
        Self {
            inner: Box::new(component),
        }
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BoxedWebRenderSync<L, B> {
    fn render(self, ctx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        self.inner.render_sync_dyn(ctx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for BoxedWebRenderSync<L, B> {
    fn render(
        self,
        ctx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(
            self.inner.render_sync_dyn(ctx),
        )
    }
}

/// Box a synchronous component at an explicit dynamic-composition boundary.
///
/// The buffer and loader types are normally inferred from the surrounding
/// `html_sync!`/`html!` render context.
pub fn boxed_sync_render<T, B, L>(component: T) -> BoxedWebRenderSync<L, B>
where
    T: WebRenderSync<L, B> + Send + 'static,
    B: WriteBuf,
    L: DataLoader,
{
    BoxedWebRenderSync::new(component)
}

/// Boxed-future analogue of [`RenderOutcome`] used at the dyn-erasure
/// boundary in [`DynWebRender`].
///
/// # Why this exists: RPITIT and dyn-compatibility
///
/// [`RenderOutcome`] returns a concrete `impl Future`, using **RPITIT**
/// (return-position impl Trait in trait): a trait method whose return
/// type is `-> impl SomeTrait`. Each implementor produces its own
/// concrete type for the `impl Future` slot, generated by the compiler
/// from the body of `render`. This is great for static dispatch (no
/// boxing, no virtual call) but it makes the trait **not dyn-compatible**
/// — the compiler cannot put a single concrete future type into the
/// vtable for `dyn WebRender<::proa_core::NoLoader, ...>` because every implementor's future is
/// a different type.
///
/// `Box<dyn WebRender<L, B>>` therefore does not compile. To preserve
/// the sync/async split at the framework's `Box<dyn DynWebRender<::proa_core::NoLoader, ...>>`
/// boundary, this enum mirrors `RenderOutcome` with the `Pending`
/// variant boxed into `Pin<Box<dyn Future + 'a>>` — a single concrete
/// type that the vtable can carry.
///
/// Sync routes match `Done` and skip future allocation entirely. Async
/// routes match `Pending` and `.await` the boxed future.
pub enum DynRenderOutcome<'a> {
    /// Render completed synchronously. No future was allocated.
    Done(Result<(), WriteError>),
    /// Render is async. The framework must `.await` the boxed future.
    Pending(Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'a>>),
}

/// Dyn-compatible erasure adapter for [`WebRender`].
///
/// `WebRender<L, B>` is not dyn-compatible because it takes `self` by
/// value (dyn methods need `&self`, `&mut self`, or `self: Box<Self>`)
/// and returns return-position impl Trait in trait (see
/// [`DynRenderOutcome`] for the RPITIT explanation). Consumers that
/// need `Box<dyn ...>` for heterogeneous collections (frameworks
/// registering routes, FFI shims, etc.) cannot use `Box<dyn WebRender>`
/// directly, so this trait wraps `WebRender` in a dyn-friendly shape:
///
/// - Receiver is `self: Box<Self>` (dyn-compatible).
/// - Returns [`DynRenderOutcome`], a concrete enum that erases the
///   per-implementor future type into `Pin<Box<dyn Future + 'a>>`. No
///   `impl Trait` in the trait method signature, so the trait is
///   dyn-compatible.
///
/// One blanket impl covers all `T: WebRender<L, B> + Send + 'static`,
/// so component authors do not need to implement this trait directly.
/// The framework stores `Box<dyn DynWebRender<L, B> + Send>` and calls
/// `render_dyn`, then matches on the returned outcome.
///
/// This trait replaces the previous `RenderOnce` (sync, `Vec<u8>`),
/// `RenderOnceZcb` (sync, ZeroCopyBuf), and `AsyncRenderOnce<L>` (async,
/// `Vec<u8>`) traits with a single unified shape generic over both buffer
/// type `B` and loader `L`.
pub trait DynWebRender<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer>: Send {
    /// Render the boxed component into `ctx`, returning a sync result or
    /// a boxed future depending on whether the component is
    /// async-capable.
    fn render_dyn<'a>(self: Box<Self>, ctx: &'a mut WebContext<L, B>) -> DynRenderOutcome<'a>;
}

impl<T, L, B> DynWebRender<L, B> for T
where
    T: WebRender<L, B> + Send + 'static,
    B: WriteBuf,
    L: DataLoader,
{
    fn render_dyn<'a>(self: Box<Self>, ctx: &'a mut WebContext<L, B>) -> DynRenderOutcome<'a> {
        match WebRender::render(*self, ctx) {
            RenderOutcome::Done(res) => DynRenderOutcome::Done(res),
            RenderOutcome::Pending(fut) => DynRenderOutcome::Pending(Box::pin(fut)),
        }
    }
}

/// Explicit boxed escape hatch for heterogeneous async-capable children.
///
/// Construction stores the component in one `Box` (non-zero-sized components
/// normally require one heap allocation), and rendering uses a vtable call.
/// If the erased component returns [`RenderOutcome::Pending`], the existing
/// [`DynWebRender`] adapter also boxes that future, whose polls are dynamically
/// dispatched. Dropping that pending outcome drops (and therefore cancels) the
/// erased future. Synchronous runtime-selected children should prefer
/// [`BoxedWebRenderSync`], which preserves the immediate sync path and does
/// not box a future.
///
/// Components must be `Send + 'static`. The pending render future itself may
/// be `!Send`, matching the bound on [`WebRender`].
#[must_use = "a boxed component does nothing unless it is rendered"]
pub struct BoxedWebRender<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    inner: Box<dyn DynWebRender<L, B>>,
}

impl<L: DataLoader, B: WriteBuf> BoxedWebRender<L, B> {
    /// Erase one async-capable component.
    pub fn new<T>(component: T) -> Self
    where
        T: WebRender<L, B> + Send + 'static,
    {
        Self {
            inner: Box::new(component),
        }
    }

    /// Erase one synchronous component for use in an async tree.
    ///
    /// Prefer [`BoxedWebRenderSync`] when the surrounding tree is also
    /// synchronous.
    pub fn from_sync<T>(component: T) -> Self
    where
        T: WebRenderSync<L, B> + Send + 'static,
    {
        Self::new(SyncToAsync(component))
    }
}

struct BoxedRenderFuture<'a>(Pin<Box<dyn Future<Output = Result<(), WriteError>> + 'a>>);

impl Future for BoxedRenderFuture<'_> {
    type Output = Result<(), WriteError>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.get_mut().0.as_mut().poll(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for BoxedWebRender<L, B> {
    fn render(
        self,
        ctx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        match self.inner.render_dyn(ctx) {
            DynRenderOutcome::Done(result) => RenderOutcome::<BoxedRenderFuture<'_>>::Done(result),
            DynRenderOutcome::Pending(future) => RenderOutcome::Pending(BoxedRenderFuture(future)),
        }
    }
}

/// Box an async-capable component at an explicit dynamic-composition boundary.
///
/// The buffer and loader types are normally inferred from the surrounding
/// `html!` render context.
pub fn boxed_render<T, B, L>(component: T) -> BoxedWebRender<L, B>
where
    T: WebRender<L, B> + Send + 'static,
    B: WriteBuf,
    L: DataLoader,
{
    BoxedWebRender::new(component)
}

/// Drive a [`DynWebRender`] outcome to completion. Sync results return
/// immediately; pending futures are awaited.
pub async fn drive_dyn_render(outcome: DynRenderOutcome<'_>) -> Result<(), WriteError> {
    match outcome {
        DynRenderOutcome::Done(res) => res,
        DynRenderOutcome::Pending(fut) => fut.await,
    }
}
