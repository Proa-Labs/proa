// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![deny(unsafe_code)]

extern crate self as proa_core;

/// Version of the coordinated Proa framework release containing this crate.
///
/// Commercial entitlement ledgers use this value to identify the exact Proa
/// release covered by a perpetual license.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Target-specific modules
pub mod email;
pub mod md;
pub mod web;

// Shared utilities (HTML building blocks used by web/email)
pub mod shared;

// Rendering traits
pub mod render;

// Stack-allocated rendering
pub mod stack;

// Backwards compatibility modules (deprecated, use render:: instead)
pub mod ctx {
    //! Re-export of web for backwards compatibility.
    //!
    //! **Deprecated**: Use `proa_core::web::WebContext` or the re-exported
    //! `proa_core::WebContext` instead.
    pub use crate::web::WebContext as Ctx;
}

pub mod renderable {
    //! Re-export of render traits for backwards compatibility.
    //!
    //! **Deprecated**: Use `proa_core::render::WebRenderSync` (sync) or
    //! `proa_core::render::WebRender` (async-capable) directly.
    pub use crate::render::{WebRender, WebRenderSync};
}

// For backwards compatibility, re-export shared modules at the root
pub mod attr {
    pub use crate::shared::attr::*;
}
pub mod elem {
    pub use crate::shared::elem::*;
}
pub mod html_node {
    pub use crate::shared::html_node::*;
}
pub mod mode {
    pub use crate::shared::mode::*;
}
pub mod tag {
    pub use crate::shared::tag::*;
}
pub mod text {
    pub use crate::shared::text::*;
}

// Primary public API
#[cfg(feature = "html-to-markdown")]
pub use md::{to_markdown, ToMarkdown};
pub use md::{AsyncMd, AsyncMdFuture, MdCodeContent, OptimizedMd, TrustedMarkdown};
#[allow(deprecated)]
pub use render::{
    boxed_render, boxed_sync_render, drive_dyn_render, drive_dyn_send_render,
    render_in_send_context, render_send_child, AwaitComponent, AwaitMdComponent,
    AwaitSendComponent, BoxedWebRender, BoxedWebRenderSync, CacheDependency, CacheHint, CacheMode,
    CacheTrace, DataLoader, DynDataLoader, DynLoaderHandle, DynRenderOutcome, DynSendRenderOutcome,
    DynWebRender, DynWebRenderSend, DynWebRenderSync, ErrorBoundary, HeadContext, HeadEntry,
    InOrderPagePool, InOrderSinkWriter, LoadError, LoadFuture, LoadHints, LoadKey, LoadKeyKind,
    LoadRequest, LoadValue, Locals, MdRender, MdRenderAsyncExt, MdRenderSync, MdRenderSyncExt,
    MdSyncToAsync, MemorySink, NoLoader, PagePool, PendingBoundary, Priority, RenderArtifacts,
    RenderMode, RenderOutcome, RenderTelemetry, RenderedBoundary, RequestContext, RequestLocation,
    ResponseContext, RouteParam, RouteParams, RsjsArtifactBranch, RsjsArtifactCheckpoint,
    RsjsLinkedProviderOccurrenceKey, RsjsLinkedProviderTagGuard, RsjsQuerySeedArtifact,
    RsjsRenderArtifacts, RsjsSharedStateScopeGuard, RsjsSharedStateSeedArtifact,
    SendPendingBoundary, SendPendingBoundaryFuture, SendRenderedBoundary,
    SendStreamBoundaryRecorder, SendStreamChunkFlusher, SendSyncToAsync, SinkCheckpoint, SinkPage,
    SinkWriter, StandardDataLoader, StatelessSyncToSend, StreamBoundaryRecorder,
    StreamChunkFlusher, StreamChunkView, SyncToAsync, TelemetryEvent, TwoSync, WebContext,
    WebContextParts, WebRender, WebRenderSend, WebRenderSendSync, WebRenderSync,
    IN_ORDER_SINK_PAGE_SIZE,
};
pub use shared::attr::{Attr, AttrKey};
pub use shared::attr_value::SafeAttr;
pub use shared::class_list::ClassList;
pub use shared::elem::{el, void, Elem, Normal, Void};
pub use shared::html_node::{
    send_async_html_from_fn, static_html, AsyncHtml, AsyncHtmlFuture, ConstHtml, OptimizedHtml,
    SendAsyncHtml, SendAsyncHtmlFuture, StaticHtml,
};
pub use shared::mode::ContentMode;
pub use shared::output_meter::{
    BorrowedOutputMeter, MeteredCheckpoint, MeteredWriteBuf, OutputMeter, OutputMeterHandle,
    OutputReservation, ReservedCheckpoint, ReservedWriteBuf, SharedOutputMeter,
};
pub use shared::tag::Tag;
pub use shared::text::{
    foreign_content_ok, perf_ok, raw_text_ok, style_ok, text, SafeText, StaticRaw, Text, UnsafeRaw,
};
pub use shared::variant::Variant;
#[allow(deprecated)]
pub use shared::write_buf::DefaultChainedBumpWriter;
pub use shared::write_buf::{
    render_growable, BumpWriter, ChainedBumpCheckpoint, ChainedBumpWriter, CheckpointWriteBuf,
    FlushMark, Part, RenderBuffer, SizeHint, SmallBuf, SmallBufCheckpoint, StagedBuf,
    StreamingChunk, StreamingChunkPart, StreamingZeroCopyBuf, WriteBuf, WriteError, ZeroCopyBuf,
    ZeroCopyCheckpoint, DEFAULT_CHAINED_BUMP_PAGE_SIZE,
};
pub use web::base_path::{AppUrl, BasePath, InvalidBasePath};
pub use web::{csp_nonce_attr, CspNonce, CspNonceAttr, InvalidCspNonce};
pub use web::{RsjsSharedStateSeedCheckpoint, StandardWebContext, STANDARD_PAGE_SIZE};
pub use web::{SendWebContext, SendWebContextParts};

// Backwards compatibility aliases (deprecated)
pub use render::WebContext as Ctx;

/// Backwards-compatible alias for the standard synchronous render trait.
///
/// **Deprecated**: Use `WebRenderSync` directly for sync components, or
/// `WebRender` for async-capable ones.
pub trait Render {
    /// Render to the standard reusable buffer.
    fn render(self, cx: &mut WebContext) -> Result<(), shared::write_buf::WriteError>;
}

impl<T: WebRenderSync> Render for T {
    fn render(self, cx: &mut WebContext) -> Result<(), shared::write_buf::WriteError> {
        WebRenderSync::render(self, cx)
    }
}

/// Temporarily moves a caller-owned writer into a standard context without
/// constructing a replacement writer in the vacated slot.
///
/// The guard restores the writer on ordinary return, error propagation,
/// panic, or async cancellation. This is important for `RenderBuffer`:
/// `mem::take` would allocate a fresh 4 KiB page merely to leave a valid
/// placeholder behind while the context owns the warmed buffer.
struct ReusableWebContext<'target, const N: usize> {
    target: *mut ChainedBumpWriter<N>,
    cx: std::mem::ManuallyDrop<WebContext<NoLoader, ChainedBumpWriter<N>>>,
    _target: std::marker::PhantomData<&'target mut ChainedBumpWriter<N>>,
}

impl<'target, const N: usize> ReusableWebContext<'target, N> {
    #[allow(unsafe_code)]
    fn new(target: &'target mut ChainedBumpWriter<N>) -> Self {
        let target_ptr = std::ptr::from_mut(target);
        // SAFETY: the guard holds the source's exclusive borrow for its full
        // lifetime and writes the exact owned value back from `Drop`. No code
        // can observe or drop the temporarily uninitialized slot.
        let mut output = unsafe { target_ptr.read() };
        output.clear();
        Self {
            target: target_ptr,
            cx: std::mem::ManuallyDrop::new(WebContext::with_buffer(output)),
            _target: std::marker::PhantomData,
        }
    }

    fn context(&mut self) -> &mut WebContext<NoLoader, ChainedBumpWriter<N>> {
        &mut self.cx
    }
}

impl<const N: usize> Drop for ReusableWebContext<'_, N> {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: `cx` is taken exactly once here, and `target` still points to
        // the exclusively borrowed slot from `new`. Restoring with `write`
        // initializes that slot without attempting to drop its moved-out
        // former value.
        let cx = unsafe { std::mem::ManuallyDrop::take(&mut self.cx) };
        let output = cx.into_output();
        unsafe { self.target.write(output) };
    }
}

fn reusable_web_context<const N: usize>(
    target: &mut ChainedBumpWriter<N>,
) -> ReusableWebContext<'_, N> {
    ReusableWebContext::new(target)
}

/// Render the supplied root node into a caller-provided reusable buffer.
pub fn render_into<R: Render>(target: &mut RenderBuffer, root: R) {
    let mut reusable = reusable_web_context(target);
    root.render(reusable.context()).unwrap();
}

/// Render to a fresh `Vec<u8>`.
pub fn render_to_vec<R: Render>(root: R) -> Vec<u8> {
    let mut cx = WebContext::new();
    root.render(&mut cx).unwrap();
    cx.into_output().into_vec()
}

/// Render to a UTF-8 `String`.
///
/// # Panics
///
/// Panics if the rendered output contains invalid UTF-8, which should
/// not happen unless `UnsafeRaw` is used with non-UTF-8 content.
pub fn render_to_string<R: Render>(root: R) -> String {
    let bytes = render_to_vec(root);
    String::from_utf8(bytes).expect("render produced invalid UTF-8 — check UnsafeRaw usage")
}

/// Render to a UTF-8 `String`, returning an error on invalid UTF-8.
pub fn try_render_to_string<R: Render>(root: R) -> Result<String, std::string::FromUtf8Error> {
    let bytes = render_to_vec(root);
    String::from_utf8(bytes)
}

// ---------------------------------------------------------------------------
// Async entry points
// ---------------------------------------------------------------------------
//
// The async variants below use the standard `RenderBuffer`. Async components
// implement `WebRender` directly and typically return
// `RenderOutcome::Pending(async move { ... })`. Sync components stay on
// `WebRenderSync`; when they need to participate in an async tree, wrap them
// with `to_async(...)`.
//
// The match on `RenderOutcome` in each function is what keeps the sync
// fast path free of future-driving overhead: sync components hit the
// `Done(res)` arm and never allocate a future; async components hit
// `Pending(fut)` and are driven to completion via `.await`.
//
/// Drive a `WebRender` root to completion inside an existing [`WebContext`].
///
/// This is the reusable async driver behind the higher-level async root
/// helpers. It is also the hook for advanced callers that want to customize
/// the context directly before rendering (for example by installing a loader
/// or selecting a render mode).
pub async fn render_in_context_async<B, L, R>(
    cx: &mut WebContext<L, B>,
    root: R,
) -> Result<(), WriteError>
where
    B: WriteBuf,
    L: DataLoader,
    R: WebRender<L, B>,
{
    let outcome = root.render(cx);
    outcome.resolve().await
}

/// Render a component asynchronously to a `Vec<u8>` buffer.
///
/// Works for both sync and async components. Sync components take the
/// fast `Done` path immediately and never construct a future; async
/// components `.await` their concrete future. Use this entry point when
/// you have any async component in the tree or when you're already in
/// an async context.
pub async fn render_to_vec_async<R>(root: R) -> Vec<u8>
where
    R: WebRender,
{
    let mut cx = WebContext::new();
    render_in_context_async(&mut cx, root).await.unwrap();
    cx.into_output().into_vec()
}

/// Render a component asynchronously to a UTF-8 `String`.
///
/// # Panics
///
/// Panics if the rendered output contains invalid UTF-8, which should
/// not happen unless `UnsafeRaw` is used with non-UTF-8 content.
pub async fn render_to_string_async<R>(root: R) -> String
where
    R: WebRender,
{
    let bytes = render_to_vec_async(root).await;
    String::from_utf8(bytes).expect("render produced invalid UTF-8 — check UnsafeRaw usage")
}

/// Render a component asynchronously into a caller-provided reusable buffer.
///
/// The buffer is cleared before rendering. Useful for reusing an
/// existing allocation across requests.
pub async fn render_into_async<R>(target: &mut RenderBuffer, root: R)
where
    R: WebRender,
{
    let mut reusable = reusable_web_context(target);
    render_in_context_async(reusable.context(), root)
        .await
        .unwrap();
}

/// Render a component asynchronously to a UTF-8 `String`, returning an
/// error on invalid UTF-8 instead of panicking.
pub async fn try_render_to_string_async<R>(root: R) -> Result<String, std::string::FromUtf8Error>
where
    R: WebRender,
{
    let bytes = render_to_vec_async(root).await;
    String::from_utf8(bytes)
}

/// Explicitly adapt a sync component to the async-capable [`WebRender`] trait.
#[inline(always)]
pub fn to_async<T>(value: T) -> SyncToAsync<T> {
    SyncToAsync(value)
}

/// Lift a [`WebRenderSendSync`] component into the async send-capable surface.
#[inline(always)]
pub fn to_send<T>(value: T) -> SendSyncToAsync<T> {
    SendSyncToAsync(value)
}

/// Adapt an output-only legacy [`WebRenderSync`] value for a send tree.
///
/// The value receives no request state. Stateful components should implement
/// [`WebRenderSendSync`] directly and use [`to_send`] instead.
#[inline(always)]
pub fn to_send_static<T>(value: T) -> StatelessSyncToSend<T> {
    StatelessSyncToSend(value)
}

#[doc(hidden)]
#[inline(always)]
pub fn as_async<T>(value: T) -> SyncToAsync<T> {
    to_async(value)
}

/// Wrap a future that resolves to a `WebRender` child.
///
/// This is the ergonomic bridge for components that need to await some work
/// before they know what renderable child to produce.
#[inline(always)]
pub fn await_component<F>(future: F) -> AwaitComponent<F> {
    AwaitComponent(future)
}

/// Wrap a `Send` future that resolves to a [`WebRenderSend`] child.
#[inline(always)]
pub fn await_send_component<F>(future: F) -> AwaitSendComponent<F> {
    AwaitSendComponent(future)
}

/// Explicitly adapt a sync Markdown component to the async-capable
/// [`MdRender`] trait.
#[inline(always)]
pub fn to_md_async<T>(value: T) -> MdSyncToAsync<T> {
    MdSyncToAsync(value)
}

#[doc(hidden)]
#[inline(always)]
pub fn as_md_async<T>(value: T) -> MdSyncToAsync<T> {
    to_md_async(value)
}

/// Wrap a future that resolves to a Markdown renderable child.
#[inline(always)]
pub fn await_md_component<F>(future: F) -> AwaitMdComponent<F> {
    AwaitMdComponent(future)
}

/// Drive any async-capable Markdown child to completion inside an async
/// Markdown render path.
pub async fn render_md_child<L, B, R>(cx: &mut WebContext<L, B>, child: R) -> Result<(), WriteError>
where
    B: WriteBuf,
    L: DataLoader,
    R: MdRender<L, B>,
{
    child.render_md(cx).resolve().await
}

/// Drive a [`MdRender`] root to completion inside an existing [`WebContext`].
pub async fn render_md_in_context_async<L, B, R>(
    cx: &mut WebContext<L, B>,
    root: R,
) -> Result<(), WriteError>
where
    B: WriteBuf,
    L: DataLoader,
    R: MdRender<L, B>,
{
    root.render_md(cx).resolve().await
}

/// Render an async-capable Markdown component to a `Vec<u8>` buffer.
pub async fn render_md_to_vec_async<R>(root: R) -> Vec<u8>
where
    R: MdRender<NoLoader, Vec<u8>>,
{
    // proa-perf-ok: this root helper intentionally owns its returned Vec without flattening pages
    let mut cx = WebContext::with_buffer(Vec::new());
    render_md_in_context_async(&mut cx, root).await.unwrap();
    cx.into_output()
}

/// Render an async-capable Markdown component to a UTF-8 `String`.
///
/// # Panics
///
/// Panics if the rendered output contains invalid UTF-8.
pub async fn render_md_to_string_async<R>(root: R) -> String
where
    R: MdRender<NoLoader, Vec<u8>>,
{
    let bytes = render_md_to_vec_async(root).await;
    String::from_utf8(bytes).expect("markdown render produced invalid UTF-8")
}

/// Drive any `WebRender` child to completion inside an async render path.
///
/// This is the bridge used by the async-capable `html!` macro: each
/// interpolated `{expr}` becomes `render_child(cx, expr).await?`, which keeps
/// async children on the `Pending` path and expects sync children to be wrapped
/// with [`to_async`] when they need to participate in async composition.
pub async fn render_child<B, L, R>(cx: &mut WebContext<L, B>, child: R) -> Result<(), WriteError>
where
    B: WriteBuf,
    L: DataLoader,
    R: WebRender<L, B>,
{
    child.render(cx).resolve().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escapes_basic_entities() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        WebRenderSync::render(text("Hello & <world>"), &mut cx).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "Hello &amp; &lt;world&gt;");
    }

    #[test]
    fn element_renders_with_attributes() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        WebRenderSync::render(
            el(
                Tag::Div,
                [
                    Attr::Str {
                        k: AttrKey::Class,
                        v: "container",
                    },
                    Attr::Bool {
                        k: AttrKey::Hidden,
                        present: false,
                    },
                ],
                |cx| {
                    WebRenderSync::render(text("Body"), cx)?;
                    Ok(())
                },
            ),
            &mut cx,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<div class=\"container\">Body</div>"
        );
    }

    #[test]
    fn void_element_skips_close_tag() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        WebRenderSync::render(
            void(
                Tag::Img,
                [Attr::Str {
                    k: AttrKey::Src,
                    v: "/hero.png",
                }],
            ),
            &mut cx,
        )
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "<img src=\"/hero.png\">");
    }

    #[test]
    fn safe_text_skips_escaping() {
        let mut out = Vec::new();
        let mut cx = WebContext::with_buffer(&mut out);
        // safe_text! verifies at compile time that string is safe
        WebRenderSync::render(safe_text!("Hello World"), &mut cx).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "Hello World");
    }
}
