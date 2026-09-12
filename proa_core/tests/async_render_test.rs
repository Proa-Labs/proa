// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the async root-render entry points in
//! `proa_core::lib` (Phase C1 of the async-render migration).
//!
//! These tests exercise three behaviours of `render_to_vec_async`:
//!
//! 1. A plain sync component wrapped with `to_async(...)` and rendered through
//!    the async entry point produces byte-for-byte identical output to the sync
//!    entry point. This confirms that the explicit sync adapter takes the
//!    `Done` fast path and sync components never pay future-driving overhead.
//! 2. An async component (one that returns `RenderOutcome::Pending` with a
//!    real `.await` inside the stack future) produces the expected HTML bytes.
//!    This is the load-bearing test — it proves futures are actually driven
//!    to completion and written into the shared `WebContext` buffer.
//! 3. An async component whose future resolves to `Err(WriteError::...)`
//!    causes the entry point to panic (via `.unwrap()`), matching the
//!    documented behaviour of the sync entry points for `Vec<u8>` backed
//!    buffers where errors are a programmer contract violation.
//!
//! We use `tokio::test` with the multi-threaded runtime so that boxed
//! route-boundary futures still run under the same runtime flavor the
//! framework uses in production.

use std::future::Future;

use proa_core::render::RenderOutcome;
use proa_core::{
    await_component, render_into, render_into_async, render_to_string_async, render_to_vec,
    render_to_vec_async, to_async, try_render_to_string_async, RenderBuffer, WebContext, WebRender,
    WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;

// ---------------------------------------------------------------------------
// Sync component: emits static HTML via WebRenderSync.
// ---------------------------------------------------------------------------

/// A minimal sync component: writes a fixed tag + escape-free text + close.
struct SyncGreeting {
    name: &'static str,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for SyncGreeting {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.out.extend(b"<p>hello ")?;
        cx.out.extend(self.name.as_bytes())?;
        cx.out.extend(b"</p>")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Async component: pushes part of the output from inside a future that
// actually `.await`s something non-trivial.
// ---------------------------------------------------------------------------

/// Component whose render path contains a real `.await`. Simulates the shape
/// of a future data-fetching component: emit an opening tag synchronously,
/// then yield to the runtime, then emit the closing tag.
struct AsyncGreeting {
    name: &'static str,
}

impl<L: ::proa_core::DataLoader> WebRender<L> for AsyncGreeting {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            // Disambiguate `extend`: `Vec<u8>` has both the stdlib `Extend`
            // impl (infallible, `IntoIterator`-based) and the Proa `WriteBuf`
            // impl (returns `Result<(), WriteError>`). We want the latter.
            // `cx.out` is already a `&mut Vec<u8>`, so passing it directly
            // gives `WriteBuf::extend` the `&mut self` it needs.
            WriteBuf::extend(&mut cx.out, b"<p>async hello ")?;
            // Real yield point: the runtime must poll us again after this.
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            WriteBuf::extend(&mut cx.out, self.name.as_bytes())?;
            WriteBuf::extend(&mut cx.out, b"</p>")?;
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// Failing async component: forces the `.unwrap()` inside the entry point to
// panic. Used to pin down the failure behaviour of the Vec<u8> entry points.
// ---------------------------------------------------------------------------

struct FailingAsync;

impl<L: ::proa_core::DataLoader> WebRender<L> for FailingAsync {
    fn render(
        self,
        _cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending(async move {
            // Yield once so this really is an async code path rather than an
            // immediately-Ready future masquerading as Pending.
            tokio::task::yield_now().await;
            Err(WriteError::OutOfCapacity)
        })
    }
}

struct FailingSyncAfterWrite;

impl WebRenderSync for FailingSyncAfterWrite {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.out.extend_static(b"partial")?;
        Err(WriteError::OutOfCapacity)
    }
}

struct PendingAfterWrite;

impl WebRender for PendingAfterWrite {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.out.extend_static(b"partial")?;
            std::future::pending::<()>().await;
            Ok(())
        })
    }
}

struct AwaitedGreeting {
    name: &'static str,
}

struct AwaitedLeaf {
    name: &'static str,
}

impl<B: WriteBuf, L: ::proa_core::DataLoader> WebRenderSync<L, B> for AwaitedLeaf {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        WebRenderSync::render(
            html_sync! {
                <p>"awaited hello "{self.name}</p>
            },
            cx,
        )
    }
}

impl<L: ::proa_core::DataLoader> WebRender<L> for AwaitedGreeting {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        await_component(async move {
            tokio::task::yield_now().await;
            to_async(AwaitedLeaf { name: self.name })
        })
        .render(cx)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sync_component_via_async_adapter_matches_sync_entry() {
    // Independent components so we don't share borrowed state across calls.
    let expected = render_to_vec(SyncGreeting { name: "world" });
    let got = render_to_vec_async(to_async(SyncGreeting { name: "world" })).await;

    assert_eq!(
        got, expected,
        "async entry point must produce byte-identical output to the sync entry point for sync components"
    );
    assert_eq!(String::from_utf8(got).unwrap(), "<p>hello world</p>");
}

#[tokio::test]
async fn async_component_via_async_entry_produces_expected_bytes() {
    let html = render_to_string_async(AsyncGreeting { name: "phase_c" }).await;
    assert_eq!(html, "<p>async hello phase_c</p>");
}

#[tokio::test]
async fn await_component_renders_future_produced_child() {
    let html = render_to_string_async(AwaitedGreeting { name: "helper" }).await;
    assert_eq!(html, "<p>awaited hello helper</p>");
}

#[tokio::test]
async fn render_into_async_reuses_buffer() {
    let mut buf = RenderBuffer::new();
    WriteBuf::extend_static(&mut buf, b"leftover that must be cleared").unwrap();
    render_into_async(&mut buf, AsyncGreeting { name: "reuse" }).await;
    assert_eq!(
        std::str::from_utf8(&buf.to_vec()).unwrap(),
        "<p>async hello reuse</p>"
    );
}

#[test]
fn render_into_restores_the_owned_buffer_when_render_panics() {
    let mut buf = RenderBuffer::new();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_into(&mut buf, FailingSyncAfterWrite);
    }));

    assert!(panic.is_err());
    assert_eq!(buf.to_vec(), b"partial");
    buf.clear();
    buf.extend_static(b"reused").unwrap();
    assert_eq!(buf.to_vec(), b"reused");
}

#[tokio::test]
async fn cancelling_render_into_async_restores_the_owned_buffer() {
    let mut buf = RenderBuffer::new();
    let mut render = Box::pin(render_into_async(&mut buf, PendingAfterWrite));
    let mut task_cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(render.as_mut().poll(&mut task_cx).is_pending());
    drop(render);

    assert_eq!(buf.to_vec(), b"partial");
    buf.clear();
    buf.extend_static(b"reused").unwrap();
    assert_eq!(buf.to_vec(), b"reused");
}

#[tokio::test]
async fn try_render_to_string_async_succeeds_on_valid_utf8() {
    let result = try_render_to_string_async(AsyncGreeting { name: "ok" }).await;
    assert_eq!(result.unwrap(), "<p>async hello ok</p>");
}

#[tokio::test]
#[should_panic(expected = "OutOfCapacity")]
async fn async_error_propagates_as_panic_for_vec_entry_points() {
    // Vec<u8>-backed entry points unwrap on write errors because Vec<u8>
    // is supposed to be infallible; seeing Err here means a component is
    // manually synthesising a failure, which we treat as a bug.
    let _ = render_to_vec_async(FailingAsync).await;
}

// ---------------------------------------------------------------------------
// `_a11y_ok` opt-out stripping parity (sync and async dynamic paths).
// ---------------------------------------------------------------------------

/// The `_a11y_ok` validation opt-out must never appear in rendered markup.
/// The async emitter historically leaked it on the dynamic path (fixed with
/// the ConsolidationSink walk); this pins the corrected behavior on both
/// entry points.
#[tokio::test]
async fn a11y_opt_out_is_stripped_on_both_dynamic_paths() {
    use proa_macros::html;

    struct SyncDynamic {
        label: &'static str,
    }
    impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for SyncDynamic {
        fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
            WebRenderSync::render(
                html_sync! {
                    <div _a11y_ok role="progressbar" aria-label={self.label}>{self.label}</div>
                },
                cx,
            )
        }
    }

    let sync_html = {
        let buf = render_to_vec(SyncDynamic { label: "sync" });
        String::from_utf8(buf).unwrap()
    };
    assert!(
        !sync_html.contains("a11y"),
        "sync dynamic path leaked the opt-out: {sync_html}"
    );

    let async_label = "async";
    let async_html = render_to_string_async(html! {
        <div _a11y_ok role="progressbar" aria-label={async_label}>{async_label}</div>
    })
    .await;
    assert!(
        !async_html.contains("a11y"),
        "async dynamic path leaked the opt-out: {async_html}"
    );
    assert!(async_html.contains("role=\"progressbar\""));
}
