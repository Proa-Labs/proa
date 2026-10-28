// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Web (HTML) rendering traits and adapters.
//!
//! Two parallel traits live here:
//!
//! - [`WebRenderSync`] for components that render synchronously. Most UI
//!   components implement this. Zero overhead on the render hot path:
//!   no enum dispatch, no future construction, no heap allocation.
//! - [`WebRender`] for components that may need to `.await` data fetches.
//!   Returns a [`RenderOutcome`] enum so the static-dispatch sync fast
//!   path is preserved when no actual await happens.
//!
//! Plus two adapters for crossing the sync/async boundary:
//!
//! - [`SyncToAsync`] lifts a `WebRenderSync` impl into a `WebRender` impl
//!   (immediate `RenderOutcome::Done`).
//! - [`AwaitComponent`] turns a future-of-component into a `WebRender`
//!   that awaits then renders the result.
//!
//! Sibling module [`super::dyn_web_render`] hosts the dyn-friendly
//! erasure adapter for [`WebRender`].
//!
//! # Why components write directly to the output buffer
//!
//! The render methods return completion, keeping locally owned data alive
//! until its borrowed children have rendered. Replacing this with a returned,
//! retained view introduces two independent constraints:
//!
//! **Ownership / self-reference:** returning both an owner and a child borrowing
//! it is not an ordinary movable Rust value. `move` around an already-borrowed
//! template, or a `Box` containing that template, does not repair its lifetime.
//!
//! ```compile_fail,E0505,E0515
//! struct Label<'a>(&'a str);
//! struct Prepared<'a> {
//!     title: String,
//!     child: Label<'a>,
//! }
//! fn prepare(title: String) -> Prepared<'static> {
//!     let child = Label(title.as_str());
//!     Prepared { title, child }
//! }
//! ```
//!
//! **Control flow / heterogeneous return types:** each dynamic `html_sync!`
//! invocation has its own closure type. An ordinary `if` / `else` cannot return
//! both under one `impl WebRenderSync`; it needs an enum, erasure, or a compiler
//! transformation. Rendering in each branch instead gives both the same
//! `Result<(), WriteError>` type.
//!
//! ```compile_fail,E0308
//! use proa_core::WebRenderSync;
//! use proa_macros::html_sync;
//!
//! fn label(title: &str, compact: bool) -> impl WebRenderSync + '_ {
//!     if compact {
//!         html_sync! { <h1>{title}</h1> }
//!     } else {
//!         html_sync! { <p>{title}</p> }
//!     }
//! }
//! ```
//!
//! These constraints do not forbid returned views. An owning closure can create
//! and render borrowed children inside its invocation, and boxed erasure can
//! unify different closure types. Such a closure still hides child discovery
//! inside executable code. The current macros already return renderable values;
//! they do not return an inspectable tree. Buffer writes also do not imply
//! immediate network delivery: the response adapter controls delivery.
//!
//! See `docs/agent/api-reference/direct-buffer-rendering.md` in the repository
//! for the rationale, working example, alternatives, and performance caveats.

use std::future::Future;

use crate::render::loader::NoLoader;
use crate::render::outcome::RenderOutcome;
use crate::render::DataLoader;
use crate::shared::write_buf::{RenderBuffer, WriteBuf, WriteError};
use crate::web::WebContext;

/// Framework-owned marker types used by generated code to prove that an
/// analyzed impl resolved to one of Proa's render traits.
///
/// The module and marker types are public because proc-macro expansions in
/// downstream crates must name them. Their constructors remain private so an
/// unrelated trait cannot accidentally reproduce the witness contract.
#[doc(hidden)]
pub mod __private {
    /// Identity witness for [`super::WebRenderSync`].
    #[derive(Clone, Copy, Debug)]
    pub struct SyncRenderTraitWitness {
        _private: (),
    }

    /// Identity witness for [`super::WebRender`].
    #[derive(Clone, Copy, Debug)]
    pub struct AsyncRenderTraitWitness {
        _private: (),
    }

    pub const SYNC_RENDER_TRAIT_WITNESS: SyncRenderTraitWitness =
        SyncRenderTraitWitness { _private: () };
    pub const ASYNC_RENDER_TRAIT_WITNESS: AsyncRenderTraitWitness =
        AsyncRenderTraitWitness { _private: () };
}

/// Trait for components that can render to HTML **synchronously**.
///
/// This is the original Proa render trait. Synchronous render components use
/// this signature and pay literal zero overhead on the render hot path — no
/// enum dispatch, no future construction, no heap allocation.
///
/// Most user components should implement `WebRenderSync`. Async components
/// that need to `.await` data fetches implement [`WebRender`] instead.
///
/// See the [direct-buffer rendering rationale](self#why-components-write-directly-to-the-output-buffer)
/// for why this returns completion rather than a retained child tree.
///
/// This trait is generic first over the request-scoped loader type `L`, then
/// over the output type `B`. Both have common defaults, so ordinary components
/// can write `WebRenderSync`, or `WebRenderSync<MyLoader>` when they load data,
/// without naming the standard [`RenderBuffer`].
///
/// # Example
///
/// ```rust
/// use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
/// use proa_macros::html_sync;
///
/// struct MyComponent;
///
/// impl<L: DataLoader> WebRenderSync<L> for MyComponent {
///     fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
///         html_sync! { <div>"Hello"</div> }.render(cx)
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not renderable in a synchronous Proa template",
    label = "missing `WebRenderSync<{L}, {B}>` for this child",
    note = "async children require `html!`; when this is a loader mismatch, the child and surrounding `WebContext` must use the same loader type"
)]
pub trait WebRenderSync<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    /// Generated component code uses this associated constant as a
    /// rustc-resolved trait identity witness.
    #[doc(hidden)]
    const __PROA_RENDER_TRAIT_WITNESS: __private::SyncRenderTraitWitness =
        __private::SYNC_RENDER_TRAIT_WITNESS;

    /// Render this component to HTML synchronously.
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}

/// Trait for components that can render to HTML **possibly asynchronously**.
///
/// Async components implement `WebRender` directly: `render` returns
/// `RenderOutcome::Pending(async move { ... })` with a compiler-generated
/// future type. That keeps component-level async rendering on the stack
/// under static dispatch; Proa only boxes futures at erased API boundaries
/// (see [`super::dyn_web_render::DynWebRender`]).
///
/// Like [`WebRenderSync`], this returns completion rather than a retained child
/// tree; local data stays alive inside the rendering future while borrowed
/// children render. See the [direct-buffer rendering rationale](self#why-components-write-directly-to-the-output-buffer).
///
/// # `Send` and executor portability
///
/// The returned future intentionally has no blanket [`Send`] bound. This lets
/// components use request-local state such as `Rc` and keeps the rendering
/// contract usable with local executors. Runtime independence does not require
/// a future to be `!Send`, however: a `Send` future is also portable and can be
/// polled on a single-threaded executor. Erased framework boundaries should
/// preserve that stronger capability when a complete render graph has it, so
/// server adapters can poll such streams directly and reserve a local-executor
/// bridge for genuinely `!Send` renders.
///
/// The portable framework contract and adapter consequences are recorded in
/// `proa_framework/proa_framework_core/README.md` under “`Send` policy and
/// runtime portability.”
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not renderable in an async-capable Proa template",
    label = "missing `WebRender<{L}, {B}>` for this child",
    note = "wrap a sync-only child with `proa_core::to_async(...)`; when this is a loader mismatch, the child and surrounding `WebContext` must use the same loader type"
)]
pub trait WebRender<L: DataLoader = NoLoader, B: WriteBuf = RenderBuffer> {
    /// Generated component code uses this associated constant as a
    /// rustc-resolved trait identity witness.
    #[doc(hidden)]
    const __PROA_RENDER_TRAIT_WITNESS: __private::AsyncRenderTraitWitness =
        __private::ASYNC_RENDER_TRAIT_WITNESS;

    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>;
}

/// Explicit adapter that lifts a sync component into the async-capable
/// [`WebRender`] surface.
pub struct SyncToAsync<T>(pub T);

impl<T, L, B> WebRender<L, B> for SyncToAsync<T>
where
    T: WebRenderSync<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(self.0.render(cx))
    }
}

#[doc(hidden)]
pub type SyncAsAsync<T> = SyncToAsync<T>;

/// Wrap a future that resolves to a `WebRender` child.
pub struct AwaitComponent<F>(pub F);

impl<F, L, B, R> WebRender<L, B> for AwaitComponent<F>
where
    F: Future<Output = R>,
    B: WriteBuf,
    L: DataLoader,
    R: WebRender<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let child = self.0.await;
            child.render(cx).resolve().await
        })
    }
}

/// Unit type renders nothing - useful for components with optional children.
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for () {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for () {
    fn render(
        self,
        _cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
    }
}

/// `Option<T>` renders `T` if `Some`, nothing if `None` - useful for optional children.
impl<L: DataLoader, B: WriteBuf, T: WebRenderSync<L, B>> WebRenderSync<L, B> for Option<T> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(inner) = self {
            inner.render(cx)?;
        }
        Ok(())
    }
}
