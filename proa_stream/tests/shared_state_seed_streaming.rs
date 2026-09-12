// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression tests for rsjs shared-state seed lifetime across the real
//! in-order streaming paths (`Suspense` + `ErrorBoundary`).
//!
//! Two invariants, one per direction:
//!
//! 1. Markup that already left the process keeps its seeds. An
//!    `ErrorBoundary` recovery cannot recall bytes a nested `Suspense`
//!    already flushed, so dropping the seeds of the shipped subtree would
//!    strand its islands with `rsjs/shared-state-missing-scope`.
//! 2. Markup that never left the process loses its seeds. When a streamed
//!    `Suspense` child fails and its buffered bytes are discarded, the
//!    serialized state that subtree registered must not ship either.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use proa_core::{
    DataLoader, ErrorBoundary, NoLoader, RenderBuffer, RenderMode, RenderOutcome,
    StreamChunkFlusher, WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};
use proa_stream::Suspense;

/// Collects everything handed to the flusher, i.e. the bytes the client has
/// already received and that no rewind can take back.
#[derive(Default)]
struct CollectingSink {
    chunks: RefCell<Vec<Vec<u8>>>,
}

impl CollectingSink {
    fn streamed(&self) -> String {
        let mut out = String::new();
        for chunk in self.chunks.borrow().iter() {
            out.push_str(std::str::from_utf8(chunk).expect("streamed chunks are utf-8"));
        }
        out
    }
}

impl StreamChunkFlusher for CollectingSink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.chunks.borrow_mut().push(chunk);
        Ok(())
    }
}

/// Fallback usable as both a `Suspense` loading fallback and an
/// `ErrorBoundary` error fallback on either streaming writer.
#[derive(Clone)]
struct Marker(&'static str);

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Marker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WriteBuf::extend(&mut cx.out, self.0.as_bytes())
    }
}

/// Async leaf that resolves after a yield and writes one element.
struct SlowPanel;

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for SlowPanel {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            WriteBuf::extend_static(&mut cx.out, b"<section>slow</section>")
        })
    }
}

/// Enters a shared-state provider, emits island markup that references the
/// scope, defers through a real `Suspense` (which flushes the shell), and
/// only then fails with a boundary-catchable error.
struct ProviderIslandThenSuspenseThenFail;

impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for ProviderIslandThenSuspenseThenFail {
    fn render(
        self,
        cx: &mut WebContext<L, RenderBuffer>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.cart",
                "sha256:cart",
                "CartState",
                &[],
                Rc::new(7u32),
                "{\"items\":3}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.cart",
                "sha256:cart",
                "CartState",
                "CartBadge",
            )?;
            WriteBuf::extend_static(
                &mut cx.out,
                b"<span data-rsjs-island=\"CartBadge\" data-scope=\"0\"></span>",
            )?;
            proa_core::render_child(cx, Suspense::new(SlowPanel, Marker("<p>loading</p>"))).await?;
            WriteBuf::extend_static(&mut cx.out, b"<div>tail</div>")?;
            Err(WriteError::render("tail section failed"))
        })
    }
}

/// Enters a shared-state provider, writes markup, then fails. Used as a
/// `Suspense` child so the failure is recovered by the streaming boundary
/// while every byte it wrote is still buffered.
struct ProviderThenFail;

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for ProviderThenFail {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.checkout",
                "sha256:checkout",
                "CheckoutState",
                &[],
                Rc::new(1u32),
                "{\"user_email\":\"secret@example.com\",\"cart_total\":4200}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.checkout",
                "sha256:checkout",
                "CheckoutState",
                "CheckoutPanel",
            )?;
            WriteBuf::extend_static(&mut cx.out, b"<div>doomed subtree</div>")?;
            tokio::task::yield_now().await;
            Err(WriteError::render("checkout load failed"))
        })
    }
}

/// Install the configuration `InOrderStreamDriver` builds: in-order render
/// mode and a chunk flusher.
fn in_order_context(sink: &Rc<CollectingSink>) -> WebContext<NoLoader, RenderBuffer> {
    let flusher: Rc<dyn StreamChunkFlusher> = Rc::clone(sink) as Rc<dyn StreamChunkFlusher>;
    let mut cx = WebContext::new();
    cx.set_render_mode(RenderMode::ChunkedInOrder);
    cx.response_mut().mark_streaming_started();
    cx.set_stream_chunk_flusher(flusher);
    cx
}

#[tokio::test]
async fn error_boundary_keeps_seeds_for_markup_already_streamed() {
    let sink = Rc::new(CollectingSink::default());
    let mut cx = in_order_context(&sink);

    WebRender::render(
        ErrorBoundary::new(
            ProviderIslandThenSuspenseThenFail,
            Marker("<p>boundary fallback</p>"),
        ),
        &mut cx,
    )
    .resolve()
    .await
    .unwrap();

    let streamed = sink.streamed();
    assert!(
        streamed.contains("data-rsjs-island=\"CartBadge\""),
        "the provider's island markup must already be on the wire: {streamed}"
    );

    let table = cx.rsjs_shared_state_seed_table_json().expect(
        "the seed table must survive: the island referencing scope 0 was already streamed to \
         the client and cannot be recalled by the boundary rewind",
    );
    assert!(
        table.contains("test.cart") && table.contains("\"scope_id\":0"),
        "shipped markup must keep the seed it hydrates from: {table}"
    );
}

/// Same invariant as [`error_boundary_keeps_seeds_for_markup_already_streamed`]
/// for the other way bytes leave an in-order render: the `Suspense` child's
/// own writer is flushed directly, bypassing the shell context. Here the
/// shell is empty when the boundary is entered, so the pre-flush is a no-op
/// and that direct flush is the only shipment.
#[tokio::test]
async fn error_boundary_keeps_seeds_streamed_by_a_suspense_childs_own_flush() {
    struct SuspenseFirstThenFail;

    impl<L: DataLoader + 'static> WebRender<L, RenderBuffer> for SuspenseFirstThenFail {
        fn render(
            self,
            cx: &mut WebContext<L, RenderBuffer>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                proa_core::render_child(
                    cx,
                    Suspense::new(ProviderIslandPanel, Marker("<p>loading</p>")),
                )
                .await?;
                WriteBuf::extend_static(&mut cx.out, b"<div>tail</div>")?;
                Err(WriteError::render("tail section failed"))
            })
        }
    }

    struct ProviderIslandPanel;

    impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for ProviderIslandPanel {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                let _scope = cx.rsjs_shared_state_enter(
                    "test.cart",
                    "sha256:cart",
                    "CartState",
                    &[],
                    Rc::new(7u32),
                    "{\"items\":3}".to_owned(),
                )?;
                cx.rsjs_shared_state_acquire::<u32>(
                    "test.cart",
                    "sha256:cart",
                    "CartState",
                    "CartBadge",
                )?;
                tokio::task::yield_now().await;
                WriteBuf::extend_static(
                    &mut cx.out,
                    b"<span data-rsjs-island=\"CartBadge\" data-scope=\"0\"></span>",
                )
            })
        }
    }

    let sink = Rc::new(CollectingSink::default());
    let mut cx = in_order_context(&sink);

    WebRender::render(
        ErrorBoundary::new(SuspenseFirstThenFail, Marker("<p>boundary fallback</p>")),
        &mut cx,
    )
    .resolve()
    .await
    .unwrap();

    let streamed = sink.streamed();
    assert!(
        streamed.contains("data-rsjs-island=\"CartBadge\""),
        "the Suspense child's markup was flushed straight from its own writer: {streamed}"
    );
    let table = cx
        .rsjs_shared_state_seed_table_json()
        .expect("the streamed island's seed must survive the boundary rewind");
    assert!(table.contains("test.cart"), "{table}");
}

#[tokio::test]
async fn streamed_suspense_recovery_drops_the_discarded_subtrees_seeds() {
    let sink = Rc::new(CollectingSink::default());
    let mut cx = in_order_context(&sink);

    WriteBuf::extend_static(&mut cx.out, b"<main>").unwrap();
    WebRender::render(
        ErrorBoundary::new(
            Suspense::new(ProviderThenFail, Marker("<p>loading</p>")),
            Marker("<p>boundary fallback</p>"),
        ),
        &mut cx,
    )
    .resolve()
    .await
    .unwrap();

    let streamed = sink.streamed();
    assert!(
        !streamed.contains("doomed subtree"),
        "the failed subtree's markup must be discarded: {streamed}"
    );
    assert!(
        streamed.contains("boundary fallback"),
        "the error fallback must be streamed in its place: {streamed}"
    );
    assert!(
        cx.rsjs_shared_state_seed_table_json().is_none(),
        "no markup references the discarded subtree's scope, so its serialized state must \
         not ship: {:?}",
        cx.rsjs_shared_state_seed_table_json()
    );
}

#[tokio::test]
async fn streamed_suspense_success_keeps_the_childs_seeds() {
    struct ProviderThenOk;

    impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for ProviderThenOk {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                let _scope = cx.rsjs_shared_state_enter(
                    "test.checkout",
                    "sha256:checkout",
                    "CheckoutState",
                    &[],
                    Rc::new(1u32),
                    "{\"cart_total\":4200}".to_owned(),
                )?;
                cx.rsjs_shared_state_acquire::<u32>(
                    "test.checkout",
                    "sha256:checkout",
                    "CheckoutState",
                    "CheckoutPanel",
                )?;
                tokio::task::yield_now().await;
                WriteBuf::extend_static(&mut cx.out, b"<div>checkout</div>")
            })
        }
    }

    let sink = Rc::new(CollectingSink::default());
    let mut cx = in_order_context(&sink);

    WriteBuf::extend_static(&mut cx.out, b"<main>").unwrap();
    WebRender::render(
        ErrorBoundary::new(
            Suspense::new(ProviderThenOk, Marker("<p>loading</p>")),
            Marker("<p>boundary fallback</p>"),
        ),
        &mut cx,
    )
    .resolve()
    .await
    .unwrap();

    let table = cx
        .rsjs_shared_state_seed_table_json()
        .expect("a surviving streamed subtree keeps its seed");
    assert!(table.contains("test.checkout"), "{table}");
    assert!(sink.streamed().contains("<div>checkout</div>"));
}
