// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regression tests for rsjs shared-state seed rollback on ErrorBoundary
//! rewind: a `SharedStateProvider` inside a subtree that is discarded must
//! not ship its seed in the page seed table, and a subtree whose markup was
//! already streamed away must keep its seed even though the rewind succeeds.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use proa_core::{
    static_html, DataLoader, ErrorBoundary, RenderOutcome, StreamChunkFlusher, WebContext,
    WebRender, WebRenderSync, WriteBuf, WriteError,
};

/// Enters a provider, writes partial output, then fails with a
/// boundary-catchable error.
struct ProviderThenError;

impl<L: DataLoader> WebRenderSync<L> for ProviderThenError {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let _scope = cx.rsjs_shared_state_enter(
            "test.doomed-subtree",
            "sha256:doomed",
            "DoomedPanelState",
            &[],
            Rc::new(7u32),
            "{\"panel\":\"doomed\"}".to_owned(),
        )?;
        cx.rsjs_shared_state_acquire::<u32>(
            "test.doomed-subtree",
            "sha256:doomed",
            "DoomedPanelState",
            "DoomedPanel",
        )?;
        WriteBuf::extend_static(&mut cx.out, b"<p>doomed</p>")?;
        Err(WriteError::render("panel body failed"))
    }
}

#[test]
fn error_boundary_rewind_drops_the_dead_scopes_seed() {
    let mut cx = WebContext::new();

    WebRenderSync::render(
        ErrorBoundary::new(ProviderThenError, static_html(b"<p>fallback</p>")),
        &mut cx,
    )
    .unwrap();

    assert!(
        cx.rsjs_shared_state_seed_table_json().is_none(),
        "a seed registered by a rewound subtree must not ship"
    );
    assert_eq!(cx.output().to_vec(), b"<p>fallback</p>");
}

#[test]
fn error_boundary_rewind_keeps_seeds_registered_outside_the_boundary() {
    let mut cx = WebContext::new();
    let _outer = cx
        .rsjs_shared_state_enter(
            "test.outer-shell",
            "sha256:outer",
            "OuterShellState",
            &[],
            Rc::new(1u32),
            "{\"shell\":true}".to_owned(),
        )
        .unwrap();
    cx.rsjs_shared_state_acquire::<u32>(
        "test.outer-shell",
        "sha256:outer",
        "OuterShellState",
        "OuterShellConsumer",
    )
    .unwrap();

    WebRenderSync::render(
        ErrorBoundary::new(ProviderThenError, static_html(b"<p>fallback</p>")),
        &mut cx,
    )
    .unwrap();

    let table = cx
        .rsjs_shared_state_seed_table_json()
        .expect("the surviving shell seed must still ship");
    assert!(table.contains("test.outer-shell"));
    assert!(
        !table.contains("test.doomed-subtree"),
        "rolled-back subtree seed must not ship: {table}"
    );
    assert!(table.contains("\"scope_id\":0"));
    assert!(!table.contains("\"scope_id\":1"));
}

/// Async twin of [`ProviderThenError`] for the async ErrorBoundary path.
struct AsyncProviderThenError;

impl<L: DataLoader> WebRender<L> for AsyncProviderThenError {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _scope = cx.rsjs_shared_state_enter(
                "test.doomed-subtree",
                "sha256:doomed",
                "DoomedPanelState",
                &[],
                Rc::new(7u32),
                "{\"panel\":\"doomed\"}".to_owned(),
            )?;
            cx.rsjs_shared_state_acquire::<u32>(
                "test.doomed-subtree",
                "sha256:doomed",
                "DoomedPanelState",
                "AsyncDoomedPanel",
            )?;
            tokio::task::yield_now().await;
            WriteBuf::extend_static(&mut cx.out, b"<p>doomed</p>")?;
            Err(WriteError::render("async panel body failed"))
        })
    }
}

/// Trivially cloneable fallback satisfying the async ErrorBoundary bounds.
#[derive(Clone)]
struct Fallback;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Fallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WriteBuf::extend_static(&mut cx.out, b"<p>fallback</p>")
    }
}

#[tokio::test]
async fn async_error_boundary_rewind_drops_the_dead_scopes_seed() {
    let mut cx = WebContext::new();

    WebRender::render(
        ErrorBoundary::new(AsyncProviderThenError, Fallback),
        &mut cx,
    )
    .resolve()
    .await
    .unwrap();

    assert!(
        cx.rsjs_shared_state_seed_table_json().is_none(),
        "a seed registered by a rewound async subtree must not ship"
    );
    assert_eq!(cx.output().to_vec(), b"<p>fallback</p>");
}

/// Collects the bytes handed to the flusher, i.e. what the client already
/// received and no rewind can take back.
#[derive(Default)]
struct CollectingSink {
    chunks: RefCell<Vec<Vec<u8>>>,
}

impl StreamChunkFlusher for CollectingSink {
    fn flush_chunk(&self, chunk: Vec<u8>) -> Result<(), WriteError> {
        self.chunks.borrow_mut().push(chunk);
        Ok(())
    }
}

/// Enters a provider, writes island markup, ships it through the real
/// `flush_stream_chunk` path (which is what a nested `Suspense` does), then
/// fails with a boundary-catchable error.
struct ProviderFlushThenError;

impl<L: DataLoader> WebRenderSync<L> for ProviderFlushThenError {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let _scope = cx.rsjs_shared_state_enter(
            "test.shipped-subtree",
            "sha256:shipped",
            "ShippedPanelState",
            &[],
            Rc::new(7u32),
            "{\"panel\":\"shipped\"}".to_owned(),
        )?;
        cx.rsjs_shared_state_acquire::<u32>(
            "test.shipped-subtree",
            "sha256:shipped",
            "ShippedPanelState",
            "ShippedPanel",
        )?;
        WriteBuf::extend_static(&mut cx.out, b"<span data-scope=\"0\">shipped</span>")?;
        cx.flush_stream_chunk()?;
        WriteBuf::extend_static(&mut cx.out, b"<p>tail</p>")?;
        Err(WriteError::render("panel tail failed"))
    }
}

#[test]
fn error_boundary_keeps_seeds_whose_markup_was_already_flushed() {
    let sink = Rc::new(CollectingSink::default());
    let mut cx = WebContext::new();
    cx.set_stream_chunk_flusher(Rc::clone(&sink) as Rc<dyn StreamChunkFlusher>);

    WebRenderSync::render(
        ErrorBoundary::new(ProviderFlushThenError, static_html(b"<p>fallback</p>")),
        &mut cx,
    )
    .unwrap();

    // `rewind_to` reported success because the flush swapped in a fresh
    // buffer, not because the markup is gone: it is already on the wire.
    assert_eq!(
        sink.chunks.borrow().len(),
        1,
        "the subtree's markup was streamed before the failure"
    );
    let table = cx.rsjs_shared_state_seed_table_json().expect(
        "seeds for markup the client already has must still ship, or the shipped island \
         cannot hydrate",
    );
    assert!(table.contains("test.shipped-subtree"), "{table}");
    assert_eq!(cx.output().to_vec(), b"<p>fallback</p>");
}
