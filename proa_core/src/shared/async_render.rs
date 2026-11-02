// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed async continuations shared by HTML and Markdown macro output.

use crate::{DataLoader, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError};
use core::{future::Future, marker::PhantomData};

/// A render continuation that retains its capture and future types.
///
/// Unlike an erased `for<'a> FnOnce(&'a mut Context) -> BoxFuture<'a>`, an
/// async closure can relate its captured borrows to the future of each call.
/// Borrowed input only needs to remain alive until rendering completes.
pub struct AsyncRender<L, B, F> {
    render_fn: F,
    marker: PhantomData<fn(&L, &mut B)>,
}

/// Construct the shared typed continuation, inferring its context parameters.
#[inline(always)]
pub fn async_render<L, B, F>(render_fn: F) -> AsyncRender<L, B, F>
where
    L: DataLoader,
    B: WriteBuf,
    F: for<'a> AsyncFnOnce(&'a mut WebContext<L, B>) -> Result<(), WriteError>,
{
    AsyncRender {
        render_fn,
        marker: PhantomData,
    }
}

impl<L, B, F> WebRender<L, B> for AsyncRender<L, B, F>
where
    L: DataLoader,
    B: WriteBuf,
    F: for<'a> AsyncFnOnce(&'a mut WebContext<L, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending((self.render_fn)(cx))
    }
}
