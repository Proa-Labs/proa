// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The analyzed frontend is tolerant of ordinary Rust that remains exclusively
//! on the server. A server-only strategy must preserve the authored method
//! instead of reconstructing a simplified render body.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use proa_core::{render_to_string, DataLoader, WebContext, WriteBuf, WriteError};
use rsjs_macros::rsjs;

thread_local! {
    static TRACE: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

fn record(step: &'static str) {
    TRACE.with(|trace| trace.borrow_mut().push(step));
}

struct TolerantServerPage {
    skip: bool,
}

struct HostedOpaqueHydrated;

struct HydratedServerProgram {
    skip: bool,
    fail: bool,
}

struct AsyncHydratedServerProgram;

struct AsyncQueryMutationOrder;

struct LexicallyShadowedServerStatement;

struct NestedLexicallyShadowedServerStatement;

struct OrdinaryFunctionsNamedLikePrimitives;

struct ReactiveThenOrdinaryShadow;

struct ServerOnlyWithDeadClientDeclaration;

struct DualUseInboundAlias {
    title: String,
}

struct LoopBodyServerLocal;

struct TraceDrop(&'static str);

struct TraceSerialize(&'static str);

struct TraceQueryInitial;

impl Drop for TraceDrop {
    fn drop(&mut self) {
        record(self.0);
    }
}

impl serde::Serialize for TraceSerialize {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        record(self.0);
        serializer.serialize_str(self.0)
    }
}

impl serde::Serialize for TraceQueryInitial {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        record("query-initial-serialize");
        serializer.serialize_str("initial")
    }
}

impl Drop for TraceQueryInitial {
    fn drop(&mut self) {
        record("query-initial-drop");
    }
}

fn checked_server_step(fail: bool) -> Result<(), WriteError> {
    record("check");
    if fail {
        Err(WriteError::render("stop"))
    } else {
        Ok(())
    }
}

fn initial_count(step: &'static str) -> i64 {
    record(step);
    0
}

fn initial_label(step: &'static str) -> &'static str {
    record(step);
    "initial"
}

fn traced_query_initial(step: &'static str) -> TraceQueryInitial {
    record(step);
    TraceQueryInitial
}

fn server_attr_value() -> &'static str {
    "from helper"
}

fn server_child() -> &'static str {
    record("child");
    "<opaque>"
}

fn server_document() -> &'static str {
    "<server document>"
}

mod ordinary {
    pub(super) fn signal() -> &'static str {
        super::record("ordinary-signal");
        "server"
    }

    pub(super) fn interval() {
        super::record("ordinary-interval");
    }
}

#[rsjs(component)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B> for TolerantServerPage {
    #[allow(clippy::needless_return)] // The analyzer must accept an explicit tail return.
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        record("before");
        if self.skip {
            record("skip");
            return Ok(());
        }
        record("after");

        return proa_macros::html_sync! {
            <main data-source={server_attr_value()}>{server_child()}</main>
        }
        .render(cx);
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B> for HostedOpaqueHydrated {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);

        proa_macros::html_sync! {
            <main data-source={server_attr_value()}>
                <article>{server_document()}</article>
                <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
            </main>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B> for HydratedServerProgram {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        record("before");
        let _drop = TraceDrop("drop");
        checked_server_step(self.fail)?;
        record("before-signal");
        let count = rsjs::signal::<i64>(initial_count("signal"));
        record("after-signal");
        if self.skip {
            record("skip");
            return Ok(());
        }
        record("before-render");

        proa_macros::html_sync! {
            <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
        }
        .render(cx)
    }
}

struct YieldOnce {
    yielded: bool,
}

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            record("poll-ready");
            Poll::Ready(())
        } else {
            self.yielded = true;
            record("poll-pending");
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRender<L, B> for AsyncHydratedServerProgram {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            record("async-start");
            YieldOnce { yielded: false }.await;
            record("after-await");
            let label = rsjs::signal::<&'static str>(initial_label("async-signal"));
            record("before-async-render");

            proa_macros::html! {
                <button on_click={|_| label.set("next")}>{label.get()}</button>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRender<L, B> for AsyncQueryMutationOrder {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let query_key = TraceSerialize("query-key");
            let query: rsjs::Query<TraceQueryInitial> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("query", query_key)),
                traced_query_initial("query-initial"),
                "/api/query",
                rsjs::QueryOptions::default(),
            );
            record("after-query");
            let mutation_key = TraceSerialize("mutation-key");
            let mutation_invalidation = TraceSerialize("mutation-invalidates");
            let mutation = rsjs::mutation::<String, ()>(
                rsjs::QueryKey::new(("mutation", mutation_key)),
                "/api/mutation",
                &[rsjs::QueryKey::new(("query", mutation_invalidation))],
            );
            record("after-mutation");

            proa_macros::html! {
                <button on_click={|_| {
                    query.refetch();
                    mutation.run(String::new());
                }}>
                    "Run"
                </button>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B>
    for LexicallyShadowedServerStatement
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let phase = 7_i64;
        if phase == 7 {
            record("server-shadow");
        }
        let phase = rsjs::signal::<i64>(0);

        proa_macros::html_sync! {
            <button on_click={|_| phase.update(|value| value + 1)}>{phase.get()}</button>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B>
    for NestedLexicallyShadowedServerStatement
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let phase = rsjs::signal::<i64>(0);
        {
            let phase = 7_i64;
            if phase == 7 {
                record("nested-server-shadow");
            }
        }

        proa_macros::html_sync! {
            <button on_click={|_| phase.update(|value| value + 1)}>{phase.get()}</button>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B>
    for OrdinaryFunctionsNamedLikePrimitives
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let server_value = ordinary::signal();
        ordinary::interval();
        let live = rsjs::signal(0_i64);

        proa_macros::html_sync! {
            <button data-server={server_value} on_click={|_| live.set(1)}>{live.get()}</button>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B> for ReactiveThenOrdinaryShadow {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let shadowed = rsjs::signal(0_i64);
        let shadowed = 7_i64;
        let server_label = format!("server-{shadowed}");
        let live = rsjs::signal(0_i64);

        proa_macros::html_sync! {
            <main>
                <button on_click={|_| live.update(|value| value + 1)}>{live.get()}</button>
                <span data-label={server_label}>"server"</span>
            </main>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B>
    for ServerOnlyWithDeadClientDeclaration
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let _drop = TraceDrop("dead-drop");
        record("before-dead");
        let _unused = rsjs::signal::<i64>(initial_count("dead-signal"));
        record("after-dead");

        proa_macros::html_sync! {
            <main data-server-only>"static"</main>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B> for DualUseInboundAlias {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let title = self.title;
        let count = rsjs::signal(0_i64);

        proa_macros::html_sync! {
            <main>
                {for word in title.split_whitespace() {
                    proa_macros::html_sync! {
                        <span>{word}</span>
                    }
                }}
                <output data-title={title}></output>
                <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
            </main>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> proa_core::WebRenderSync<L, B> for LoopBodyServerLocal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);

        proa_macros::html_sync! {
            <main>
                {for index in 0_u8..2_u8 {
                    let day_label = if index == 0 { "one" } else { "two" };
                    proa_macros::html_sync! {
                        <span>{day_label}</span>
                    }
                }}
                <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
            </main>
        }
        .render(cx)
    }
}

#[test]
fn server_only_analysis_preserves_rust_order_and_opaque_helpers() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(TolerantServerPage { skip: false });

    assert_eq!(
        html,
        r#"<main data-source="from helper">&lt;opaque&gt;</main>"#
    );
    TRACE.with(|trace| {
        assert_eq!(&*trace.borrow(), &["before", "after", "child"]);
    });
    assert!(!html.contains("rsjs-island"));
}

#[test]
fn server_only_analysis_preserves_early_return() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(TolerantServerPage { skip: true });

    assert!(html.is_empty());
    TRACE.with(|trace| {
        assert_eq!(&*trace.borrow(), &["before", "skip"]);
    });
}

#[test]
fn hosted_opaque_ssr_coexists_with_a_hydrated_sibling() {
    let html = render_to_string(HostedOpaqueHydrated);

    assert!(html.contains(r#"data-source="from helper""#));
    assert!(html.contains("<article>&lt;server document&gt;</article>"));
    assert!(html.contains("data-rsjs-island-root"));
}

#[test]
fn hydrated_server_program_preserves_statement_initializer_and_drop_order() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(HydratedServerProgram {
        skip: false,
        fail: false,
    });

    assert!(html.contains("data-rsjs-island-root"));
    TRACE.with(|trace| {
        assert_eq!(
            &*trace.borrow(),
            &[
                "before",
                "check",
                "before-signal",
                "signal",
                "after-signal",
                "before-render",
                "drop",
            ]
        );
    });
}

#[test]
fn hydrated_server_program_preserves_early_return_and_try() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let html = render_to_string(HydratedServerProgram {
        skip: true,
        fail: false,
    });
    assert!(html.is_empty());
    TRACE.with(|trace| {
        assert_eq!(
            &*trace.borrow(),
            &[
                "before",
                "check",
                "before-signal",
                "signal",
                "after-signal",
                "skip",
                "drop",
            ]
        );
    });

    TRACE.with(|trace| trace.borrow_mut().clear());
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    use proa_core::WebRenderSync as _;
    let result = HydratedServerProgram {
        skip: false,
        fail: true,
    }
    .render(&mut cx);
    assert_eq!(result, Err(WriteError::render("stop")));
    assert!(out.is_empty());
    TRACE.with(|trace| {
        assert_eq!(&*trace.borrow(), &["before", "check", "drop"]);
    });
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn async_server_program_starts_only_when_polled_and_keeps_await_boundary() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    use proa_core::WebRender as _;
    let outcome = AsyncHydratedServerProgram.render(&mut cx);
    TRACE.with(|trace| assert!(trace.borrow().is_empty()));

    let waker = Waker::from(Arc::new(NoopWake));
    let mut task_cx = Context::from_waker(&waker);
    let mut future = Box::pin(outcome.resolve());
    assert!(matches!(future.as_mut().poll(&mut task_cx), Poll::Pending));
    TRACE.with(|trace| {
        assert_eq!(&*trace.borrow(), &["async-start", "poll-pending"]);
    });

    assert!(matches!(
        future.as_mut().poll(&mut task_cx),
        Poll::Ready(Ok(()))
    ));
    drop(future);
    TRACE.with(|trace| {
        assert_eq!(
            &*trace.borrow(),
            &[
                "async-start",
                "poll-pending",
                "poll-ready",
                "after-await",
                "async-signal",
                "before-async-render",
            ]
        );
    });
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("data-rsjs-island-root"));
}

#[test]
fn query_and_mutation_metadata_stay_at_their_authored_declaration_sites() {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    use proa_core::WebRender as _;
    let outcome = AsyncQueryMutationOrder.render(&mut cx);
    TRACE.with(|trace| assert!(trace.borrow().is_empty()));

    let waker = Waker::from(Arc::new(NoopWake));
    let mut task_cx = Context::from_waker(&waker);
    let mut future = Box::pin(outcome.resolve());
    assert!(matches!(
        future.as_mut().poll(&mut task_cx),
        Poll::Ready(Ok(()))
    ));
    TRACE.with(|trace| {
        assert_eq!(
            &*trace.borrow(),
            &[
                "query-key",
                "query-initial",
                "query-initial-serialize",
                "query-initial-drop",
                "after-query",
                "mutation-key",
                "mutation-invalidates",
                "after-mutation",
            ]
        );
    });
}

#[test]
fn opaque_statement_uses_the_binding_in_scope_at_its_lexical_position() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(LexicallyShadowedServerStatement);

    assert!(html.contains("data-rsjs-island-root"));
    TRACE.with(|trace| assert_eq!(&*trace.borrow(), &["server-shadow"]));
}

#[test]
fn opaque_nested_block_honors_its_own_lexical_shadow() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(NestedLexicallyShadowedServerStatement);

    assert!(html.contains("data-rsjs-island-root"));
    TRACE.with(|trace| assert_eq!(&*trace.borrow(), &["nested-server-shadow"]));
}

#[test]
fn namespaced_ordinary_functions_are_not_claimed_as_rsjs_primitives() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(OrdinaryFunctionsNamedLikePrimitives);

    assert!(html.contains(r#"data-server="server""#));
    TRACE.with(|trace| {
        assert_eq!(&*trace.borrow(), &["ordinary-signal", "ordinary-interval"]);
    });
}

#[test]
fn ordinary_shadow_stops_client_alias_propagation() {
    let html = render_to_string(ReactiveThenOrdinaryShadow);

    assert!(html.contains(r#"data-label="server-7""#));
    assert!(html.contains("data-rsjs-island-root"));
}

#[test]
fn server_only_program_keeps_ordinary_statements_while_pruning_dead_client_declarations() {
    TRACE.with(|trace| trace.borrow_mut().clear());

    let html = render_to_string(ServerOnlyWithDeadClientDeclaration);

    assert_eq!(html, r#"<main data-server-only>static</main>"#);
    TRACE.with(|trace| {
        assert_eq!(
            &*trace.borrow(),
            &["before-dead", "after-dead", "dead-drop"]
        );
    });
}

#[test]
fn inbound_alias_remains_available_to_server_rust_after_client_lowering() {
    let html = render_to_string(DualUseInboundAlias {
        title: String::from("server words"),
    });

    assert!(html.contains(r#"data-title="server words""#));
    assert!(html.contains(">server</span>"), "{html}");
    assert!(html.contains(">words</span>"), "{html}");
    assert!(html.contains("data-rsjs-island-root"));
}

#[test]
fn loop_body_server_local_remains_in_scope_for_repeated_template_output() {
    let html = render_to_string(LoopBodyServerLocal);

    assert!(html.contains(">one</span>"), "{html}");
    assert!(html.contains(">two</span>"), "{html}");
    assert!(html.contains("data-rsjs-island-root"), "{html}");
}
