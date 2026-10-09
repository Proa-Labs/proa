// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::cmp_owned)]
// Comparison is emitted by the attribute macro under test.

//! M7 integration test: a real `#[rsjs(component, client)] impl WebRenderSync<::proa_core::NoLoader, ...> for Counter`
//! block end-to-end through the proc-macro pipeline.
//!
//! Mirrors the M3 hand-built Counter fixture's rendered HTML
//! byte-for-byte. If this test passes, the M7b parser + M7c analyser
//! produce the same `Island` IR the M1 fixture builder produces by
//! hand, and `emit_rust` is consuming it correctly.

// The macro rewrites the impl body to an absolute-path
// `::proa_core::WebRenderSync` impl, but the input source still
// references these names by short name in the trait header. The
// macro discards the original tokens, so rustc sees the imports as
// unused; suppress the warning.
use proa_core::render_to_string;
#[allow(unused_imports)]
use proa_core::UnsafeRaw;
#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
#[allow(unused_imports)]
use rsjs::elem;
use rsjs_macros::rsjs;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on_ready<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

// Parser marker replaced by the attribute macro before method resolution.
// Its identity return keeps the authored initializer well-formed; the macro
// removes every call before rustc type-checks the generated impl.
#[allow(dead_code)]
fn signal<T>(initial: T) -> T {
    initial
}

#[rsjs::client_module(path = "crate::handler_math")]
pub mod handler_math {
    #[rsjs::client]
    pub fn double(value: i64) -> i64 {
        value * 2
    }
}

async fn async_child_label() -> Result<&'static str, WriteError> {
    Ok("Async child label")
}

pub struct AsyncServerPreludeChild;

#[rsjs(component)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for AsyncServerPreludeChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let label = async_child_label().await?;

            proa_macros::html! {
                <strong>{rsjs::server_only(label)}</strong>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct ParentWithAsyncServerPreludeChild;

#[rsjs(component)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for ParentWithAsyncServerPreludeChild
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            proa_macros::html! {
                <section>{AsyncServerPreludeChild {}}</section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct QueryMutationIsland;

#[derive(Debug, Clone, Serialize)]
struct QueryMutationStockPrice {
    value: f64,
    currency: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SaveResult {
    id: String,
    title: String,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for QueryMutationIsland {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let draft = signal(String::new());
            let symbol = "AAPL";
            let selected = signal("AAPL");
            let initial_price = QueryMutationStockPrice {
                value: 172.5,
                currency: "USD",
            };
            let price: rsjs::Query<QueryMutationStockPrice> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("stock", symbol)),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    method: Some(rsjs::HttpMethod::Post),
                    credentials: Some(rsjs::FetchCredentials::Include),
                    response: Some(rsjs::ResponseKind::Text),
                    headers: &[("Accept", "text/plain"), ("X-RSJS", "1")],
                    body: Some(rsjs::RequestBody::Json(r#"{"symbol":"AAPL"}"#)),
                    timeout_ms: Some(1_500),
                },
                rsjs::QueryOptions {
                    interval_ms: Some(5_000),
                    refetch_on_focus: true,
                    download_progress: true,
                    stream_text: true,
                    retry_count: 3,
                    stale_time_ms: 2_000,
                },
            );
            let save = rsjs::mutation_with_options::<String, ()>(
                rsjs::QueryKey::new(("update_cart", selected.get())),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Patch),
                    credentials: Some(rsjs::FetchCredentials::Omit),
                    response: Some(rsjs::ResponseKind::Empty),
                    headers: &[("X-Mutate", "1")],
                    body: None,
                    timeout_ms: Some(2_500),
                },
                &[rsjs::QueryKey::new(("stock", selected.get()))],
                rsjs::MutationOptions {
                    retry_count: 1,
                    on_success_invalidate_strategy: rsjs::InvalidationStrategy::Refetch,
                },
            );
            proa_macros::html! {
                <div>
                    <span>{price.data().value}</span>
                    <span>{price.download_loaded()}</span>
                    <span>{price.download_percent()}</span>
                    <span hidden={!save.is_pending()}>"Saving"</span>
                    <button on_click={|_| save.run(draft.get())}>"Save"</button>
                    <button on_click={|_| price.refetch()}>"Refresh"</button>
                    <button on_click={|_| price.invalidate()}>"Invalidate"</button>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct LiveQueryKeyIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for LiveQueryKeyIsland {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let selected = signal("old");
            let result: rsjs::Query<&'static str> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("search", selected.get(), selected.get())),
                "old result",
                rsjs::FetchRequest {
                    url: "/api/search",
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );

            proa_macros::html! {
                <section>
                    <button type="button" on_click={|_| selected.set("cached")}>"Cached"</button>
                    <button type="button" on_click={|_| selected.set("uncached")}>
                        "Uncached"
                    </button>
                    <output>{result.data()}</output>
                </section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct MutationProjectionIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for MutationProjectionIsland
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let save = rsjs::mutation::<String, SaveResult>(
                rsjs::QueryKey::new(("save", "item")),
                "/api/save",
                &[rsjs::QueryKey::new(("items",))],
            );
            proa_macros::html! {
                <div>
                    <span
                        data-status={save.status()}
                        data-id={save.data().id}
                        data-error={save.error().at("/message").as_str()}
                    >
                        {save.data().title}
                    </span>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct QueryMutationChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for QueryMutationChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let initial_price = QueryMutationStockPrice {
                value: 172.5,
                currency: "USD",
            };
            let price: rsjs::Query<QueryMutationStockPrice> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("child_stock", "AAPL")),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/child-stocks/AAPL",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            let save = rsjs::mutation::<&'static str, ()>(
                rsjs::QueryKey::new(("child_update_cart", "AAPL")),
                "/api/child-cart",
                &[rsjs::QueryKey::new(("child_stock", "AAPL"))],
            );

            proa_macros::html! {
                <article>
                    <span>{price.data().value}</span>
                    <button on_click={|_| {
                        save.run("AAPL");
                        price.refetch();
                    }}>
                        "Save"
                    </button>
                </article>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct ParentWithQueryMutationChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for ParentWithQueryMutationChild
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            proa_macros::html! {
                <section>{QueryMutationChild {}}</section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct ServerLocalMutationKeyIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for ServerLocalMutationKeyIsland
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL");
            let save = rsjs::mutation::<String, ()>(
                rsjs::QueryKey::new(("update_cart", symbol.as_str())),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Patch),
                    response: Some(rsjs::ResponseKind::Empty),
                    ..Default::default()
                },
                (rsjs::QueryKey::new(("stock", symbol.as_str())),),
            );
            proa_macros::html! {
                <div>
                    <span>{save.status()}</span>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct ServerLocalMutationKeyChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for ServerLocalMutationKeyChild
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL");
            let save = rsjs::mutation::<String, ()>(
                rsjs::QueryKey::new(("child_update_cart", symbol.as_str())),
                rsjs::FetchRequest {
                    url: "/api/child-cart",
                    method: Some(rsjs::HttpMethod::Patch),
                    response: Some(rsjs::ResponseKind::Empty),
                    ..Default::default()
                },
                (rsjs::QueryKey::new(("child_stock", symbol.as_str())),),
            );
            proa_macros::html! {
                <article>
                    <span>{save.status()}</span>
                </article>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct ParentWithServerLocalMutationChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for ParentWithServerLocalMutationChild
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            proa_macros::html! {
                <section>{ServerLocalMutationKeyChild {}}</section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[test]
fn rsjs_inlines_async_child_components_into_async_parent() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(ParentWithAsyncServerPreludeChild);

    let html = block_on_ready(proa_core::render_to_string_async(
        ParentWithAsyncServerPreludeChild,
    ));
    assert!(
        html.contains("<strong") && html.contains(">Async child label</strong>"),
        "async parent should inline async child server output, got:\n{html}"
    );
    assert!(
        !html.contains("<rsjs-island") && !html.contains("data-rsjs-island"),
        "async server-only parent should not emit island scaffolding, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.islands.iter().all(|entry| !entry
            .asset_id
            .starts_with("parentwithasyncserverpreludechild_")),
        "async server-only parent should not register a runtime island"
    );
    assert!(
        manifest
            .component_fragments
            .iter()
            .all(|entry| !entry.asset_id.starts_with("asyncserverpreludechild_")),
        "async server-only child should not register a runtime component fragment"
    );
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("asyncserverpreludechild_")),
        "async server-only child should not register a runtime island"
    );
    assert!(
        manifest.component_fragments.iter().all(|entry| !entry
            .asset_id
            .starts_with("parentwithasyncserverpreludechild_")),
        "async server-only parent should not register a runtime component fragment"
    );
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.js_source.contains("mount_asyncserverpreludechild_")),
        "server-only async child should not be imported as a runtime fragment"
    );
}

#[test]
fn rsjs_lowers_query_and_mutation_authoring() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(QueryMutationIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("querymutationisland_"))
        .expect("QueryMutationIsland should be registered")
        .js_source;
    assert!(
        js.contains("import { rsjs, dispose_query, register_query, register_mutation }"),
        "query+mutation island should import both runtime registrars, got:\n{js}"
    );
    assert!(
        js.contains("const __rsjs_query_0 = register_query({")
            && js.contains("key: [null]")
            && js.contains(r#"fetcher: "{\""#)
            && js.contains(r#"\"url\":\"/api/stocks/AAPL\""#)
            && js.contains(r#"\"method\":\"POST\""#)
            && js.contains(r#"\"credentials\":\"include\""#)
            && js.contains(r#"\"response\":\"text\""#)
            && js.contains(r#"\"headers\":[[\"Accept\",\"text/plain\"],[\"X-RSJS\",\"1\"]]"#)
            && js.contains(
                r#"\"body\":{\"kind\":\"json\",\"value\":\"{\\\"symbol\\\":\\\"AAPL\\\"}\"}"#
            )
            && js.contains(r#"\"timeout_ms\":1500"#)
            && js.contains("interval_ms: 5000")
            && js.contains("refetch_on_focus: true")
            && js.contains("download_progress: true")
            && js.contains("stream_text: true"),
        "query declaration should lower to register_query with options, got:\n{js}"
    );
    assert!(
        js.contains("rsjs.__project(__rsjs_query_0.data(), [\"value\"])"),
        "typed query field projection should lower through runtime helpers, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_query_0.download_loaded()")
            && js.contains("__rsjs_query_0.download_percent()"),
        "query progress accessors should lower to runtime handle methods, got:\n{js}"
    );
    assert!(
        js.contains("const __rsjs_mutation_0 = register_mutation({")
            && js.contains("key: [\"update_cart\", __sig_1.get()]")
            && js.contains(r#"runner: "{\""#)
            && js.contains(r#"\"url\":\"/api/cart\""#)
            && js.contains(r#"\"method\":\"PATCH\""#)
            && js.contains(r#"\"credentials\":\"omit\""#)
            && js.contains(r#"\"response\":\"empty\""#)
            && js.contains(r#"\"headers\":[[\"X-Mutate\",\"1\"]]"#)
            && js.contains(r#"\"timeout_ms\":2500"#)
            && js.contains("invalidates: [[\"stock\", __sig_1.get()]]")
            && js.contains("retry_count: 1"),
        "mutation declaration should lower to register_mutation with invalidation, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_mutation_0.is_pending()")
            && js.contains("__rsjs_mutation_0.run(__sig_0.get());"),
        "mutation reads and run statements should lower, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_query_0.refetch();") && js.contains("__rsjs_query_0.invalidate();"),
        "query refetch/invalidate statements should lower, got:\n{js}"
    );
}

#[test]
fn rsjs_rebinds_signal_derived_query_keys_with_one_coalesced_watcher() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(LiveQueryKeyIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("livequerykeyisland_"))
        .expect("LiveQueryKeyIsland should be registered")
        .js_source;

    assert!(
        js.contains("import { rsjs, dispose_query, register_query, rebind_query }")
            && js.contains("const __rsjs_query_registration_0 = () => ({")
            && js.contains("key: [\"search\", __sig_0.get(), __sig_0.get()]")
            && js.contains("rebind_query(__rsjs_query_0, __rsjs_query_registration_0());"),
        "signal-derived keys should emit stable-handle rebind machinery, got:\n{js}"
    );
    assert!(
        js.contains("}, [__sig_0]);")
            && js.contains("__rsjs_disposers.push(__rsjs_query_key_watcher_0._dispose);"),
        "repeated key reads should coalesce to one disposer-owned dependency, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_mutation_result_projection_with_idle_ssr_defaults() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(MutationProjectionIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("mutationprojectionisland_"))
        .expect("MutationProjectionIsland should be registered")
        .js_source;
    assert!(
        js.contains("rsjs.__project(__rsjs_mutation_0.data(), [\"id\"])")
            && js.contains("rsjs.__project(__rsjs_mutation_0.data(), [\"title\"])")
            && js.contains(
                "rsjs.__as_str(rsjs.__project(__rsjs_mutation_0.error(), [\"message\"]))"
            )
            && js.contains("[__rsjs_mutation_0]"),
        "mutation data/error projections should lower through runtime helpers and subscribe to the mutation handle, got:\n{js}"
    );

    let html = block_on_ready(proa_core::render_to_string_async(MutationProjectionIsland));
    assert!(
        html.contains(r#"data-status="idle""#)
            && html.contains(r#"data-id="""#)
            && html.contains(r#"data-error="""#),
        "server render should expose idle mutation state and empty data/error projections, got:\n{html}"
    );
}

#[test]
fn descriptor_compilation_keeps_query_mutation_child_graph_separate() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(ParentWithQueryMutationChild);

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("parentwithquerymutationchild_")),
        "a parent with only a direct component edge must not copy the child graph into a \
         provisional parent island"
    );
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("querymutationchild_"))
        .expect("QueryMutationChild should retain its provisional island artifact");
    let js = child.js_source;
    assert!(
        js.contains("const __rsjs_query_0 = register_query({")
            && js.contains("key: [\"child_stock\", \"AAPL\"]")
            && js.contains(r#"\"url\":\"/api/child-stocks/AAPL\""#),
        "the child artifact should own its query registration, got:\n{js}"
    );
    assert!(
        js.contains("const __rsjs_mutation_0 = register_mutation({")
            && js.contains("key: [\"child_update_cart\", \"AAPL\"]")
            && js.contains("invalidates: [[\"child_stock\", \"AAPL\"]]")
            && js.contains("__rsjs_mutation_0.run(\"AAPL\");")
            && js.contains("__rsjs_query_0.refetch();"),
        "the child artifact should own its mutation and handler statements, got:\n{js}"
    );
    let html = block_on_ready(proa_core::render_to_string_async(
        ParentWithQueryMutationChild,
    ));
    assert!(
        html.contains("<article")
            && html.contains(">172.5</span>")
            && html.contains(r#""0":{"key":["child_stock","AAPL"]}"#),
        "the direct child should render its provisional SSR/query snapshot through the parent, \
         got:\n{html}"
    );
}

#[test]
fn rsjs_allows_server_local_mutation_keys() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(ServerLocalMutationKeyIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("serverlocalmutationkeyisland_"))
        .expect("ServerLocalMutationKeyIsland should be registered")
        .js_source;
    assert!(
        js.contains("const __rsjs_mutation_0 = register_mutation({")
            && js.contains("root,")
            && js.contains("key: [null]")
            && js.contains(r#"\"url\":\"/api/cart\""#),
        "server-local mutation key should use JS placeholder plus root metadata, got:\n{js}"
    );

    let html = block_on_ready(proa_core::render_to_string_async(
        ServerLocalMutationKeyIsland,
    ));
    assert!(
        html.contains(r#""0":{"key":["update_cart","AAPL"]"#)
            && html.contains(r#""invalidates":[["stock","AAPL"]]"#)
            && html.contains(r#""on_success_invalidate_strategy":"refetch""#),
        "mutation metadata should serialize the server-local key at render time, got:\n{html}"
    );
}

#[test]
fn descriptor_compilation_keeps_server_local_mutation_metadata_with_child() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(ParentWithServerLocalMutationChild);

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.islands.iter().all(|entry| !entry
            .asset_id
            .starts_with("parentwithserverlocalmutationchild_")),
        "a direct child edge alone must not create a provisional parent island"
    );
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("serverlocalmutationkeychild_"))
        .expect("ServerLocalMutationKeyChild should retain its provisional island artifact");
    assert!(
        child
            .js_source
            .contains("const __rsjs_mutation_0 = register_mutation({")
            && child.js_source.contains("root,")
            && child.js_source.contains("key: [null]")
            && child.js_source.contains("invalidates: []")
            && child.js_source.contains(r#"\"url\":\"/api/child-cart\""#),
        "the child artifact should own its mutation registration and metadata fallbacks, got:\n{}",
        child.js_source
    );
    let html = block_on_ready(proa_core::render_to_string_async(
        ParentWithServerLocalMutationChild,
    ));
    assert!(
        html.contains(r#""0":{"key":["child_update_cart","AAPL"]"#)
            && html.contains(r#""invalidates":[["child_stock","AAPL"]]"#)
            && html.contains(r#""on_success_invalidate_strategy":"refetch""#),
        "the direct child should serialize its own provisional mutation metadata, got:\n{html}"
    );
}

pub struct QueryInitialIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for QueryInitialIsland {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let initial_price = StockPrice {
                value: 172.5,
                currency: String::from("USD"),
                active: true,
            };
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                "stock:AAPL",
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions {
                    stale_time_ms: 2_000,
                    ..Default::default()
                },
            );
            proa_macros::html! {
                <div>
                    <span
                        data-currency={price.data().currency}
                        data-status={price.status()}
                        data-success={price.is_success()}
                    >
                        {price.data().value}
                    </span>
                    <em hidden={!price.data().active}>"active"</em>
                    <strong hidden={price.data().active}>"inactive"</strong>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StockPrice {
    value: f64,
    currency: String,
    active: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SearchResult {
    id: String,
    title: String,
    url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SearchPayload {
    results: Vec<SearchResult>,
}

pub struct QueryArrayProjectionIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for QueryArrayProjectionIsland
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let initial = SearchPayload {
                results: vec![
                    SearchResult {
                        id: String::from("latency"),
                        title: String::from("latency.so"),
                        url: String::from("https://latency.so"),
                    },
                    SearchResult {
                        id: String::from("docs"),
                        title: String::from("Proa docs"),
                        url: String::from("/docs"),
                    },
                ],
            };
            let search: rsjs::Query<SearchPayload> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("search", "proa")),
                initial,
                rsjs::FetchRequest {
                    url: "/api/search?q=proa",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            proa_macros::html! {
                <div>
                    <a
                        href={search.data().results[0].url}
                        data-result-id={search.data().results[0].id}
                    >
                        {search.data().results[0].title}
                    </a>
                    <span>{search.data().results[1].title}</span>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct DynamicQueryUrlIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for DynamicQueryUrlIsland {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let url = signal(String::from("/api/search?q=proa"));
            let initial = SearchPayload {
                results: vec![SearchResult {
                    id: String::from("docs"),
                    title: String::from("Proa docs"),
                    url: String::from("/docs"),
                }],
            };
            let search: rsjs::Query<SearchPayload> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("search", "dynamic-url", url.get())),
                initial,
                url.get(),
                rsjs::QueryOptions::default(),
            );
            proa_macros::html! {
                <div>
                    <span>{search.data().results[0].title}</span>
                    <button on_click={|_| search.refetch()}>"Refetch"</button>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct QueryBackedListIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for QueryBackedListIsland {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let initial = SearchPayload {
                results: vec![
                    SearchResult {
                        id: String::from("latency"),
                        title: String::from("latency.so"),
                        url: String::from("https://latency.so"),
                    },
                    SearchResult {
                        id: String::from("docs"),
                        title: String::from("Proa docs"),
                        url: String::from("/docs"),
                    },
                ],
            };
            let search: rsjs::Query<SearchPayload> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("search", "list")),
                initial,
                "/api/search?q=proa",
                rsjs::QueryOptions::default(),
            );
            proa_macros::html! {
                <ul>
                    {for result in search.data().results {
                        proa_macros::html! {
                            <a key={result.id} href={result.url}>{result.title}</a>
                        }
                    }}
                </ul>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

struct StockPriceLoader;

impl proa_core::DataLoader for StockPriceLoader {
    fn load(
        &self,
        request: &proa_core::LoadRequest,
    ) -> impl Future<Output = Result<proa_core::LoadValue, proa_core::LoadError>> + Send {
        let key = request.key().clone();
        async move {
            if key == proa_core::LoadKey::named("stock", "AAPL") {
                Ok(proa_core::LoadValue::bytes(bytes::Bytes::from_static(
                    br#"{"value":174.25,"currency":"USD","active":true}"#,
                )))
            } else {
                Err(proa_core::LoadError::custom(format!(
                    "unexpected key: {key}"
                )))
            }
        }
    }
}

fn assert_stock_query_seed(seed_table: &str, namespace: &str) {
    let parsed: serde_json::Value =
        serde_json::from_str(seed_table).expect("query seed table should be valid JSON");
    let expected_key = serde_json::json!([namespace, "AAPL"]);
    let query = parsed
        .get("queries")
        .and_then(serde_json::Value::as_array)
        .and_then(|queries| {
            queries
                .iter()
                .find(|query| query.get("key") == Some(&expected_key))
        })
        .unwrap_or_else(|| panic!("query seed table should contain {expected_key}: {seed_table}"));
    let state = query.get("state").expect("query seed should contain state");
    let data = state
        .get("data")
        .expect("query seed state should contain data");

    assert_eq!(state.get("key"), Some(&expected_key));
    assert_eq!(
        data.get("active").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        data.get("currency").and_then(serde_json::Value::as_str),
        Some("USD")
    );
    assert_eq!(
        data.get("value").and_then(serde_json::Value::as_f64),
        Some(174.25)
    );
    assert_eq!(
        state
            .get("data_updated_at")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
}

pub struct DataLoaderQueryInitialIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for DataLoaderQueryInitialIsland
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL");
            let initial_price: StockPrice = cx
                .load_json(proa_core::LoadKey::named("stock", symbol.as_str()))
                .await?;
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("stock", symbol.as_str())),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            proa_macros::html! {
                <div>
                    <span data-currency={price.data().currency}>{price.data().value}</span>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct DataLoaderQueryInitialChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for DataLoaderQueryInitialChild
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL");
            let initial_price: StockPrice = cx
                .load_json(proa_core::LoadKey::named("stock", symbol.as_str()))
                .await?;
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("child_stock", symbol.as_str())),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/child-stocks/AAPL",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            proa_macros::html! {
                <article>
                    <span data-currency={price.data().currency}>{price.data().value}</span>
                </article>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct ParentWithDataLoaderQueryChild;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> proa_core::WebRender<L, B>
    for ParentWithDataLoaderQueryChild
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        proa_core::RenderOutcome::pending(async move {
            proa_macros::html! {
                <section>{DataLoaderQueryInitialChild {}}</section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[test]
fn rsjs_accepts_server_value_query_initializers() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(QueryInitialIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("queryinitialisland_"))
        .expect("QueryInitialIsland should be registered")
        .js_source;
    assert!(
        js.contains("const __rsjs_query_0 = register_query({")
            && js.contains("key: [\"stock:AAPL\"]")
            && js.contains(r#"\"url\":\"/api/stocks/AAPL\""#)
            && js.contains("stale_time_ms: 2000")
            && js.contains("rsjs.__project(__rsjs_query_0.data(), [\"currency\"])")
            && js.contains("rsjs.__project(__rsjs_query_0.data(), [\"value\"])"),
        "server-initialized queries should still lower client fetchers and options, got:\n{js}"
    );

    let html = block_on_ready(proa_core::render_to_string_async(QueryInitialIsland));
    assert!(
        html.contains(">172.5</span>") || html.contains(">172.5"),
        "server render should use the query initial value for text, got:\n{html}"
    );
    assert!(
        html.contains(r#"data-currency="USD""#)
            && html.contains(r#"data-status="success""#)
            && html.contains(r#"data-success="true""#),
        "server render should use query initial value/status for attrs, got:\n{html}"
    );
    assert!(
        html.contains(r#"<em data-rsjs-attr=""#) && !html.contains(r#"<em hidden"#),
        "hidden={{!price.data().active}} should SSR false as absent, got:\n{html}"
    );
    assert!(
        html.contains(r#"<strong hidden"#),
        "hidden={{price.data().active}} should SSR true as present, got:\n{html}"
    );
    assert!(
        html.contains(r#""0":{"key":["stock:AAPL"]}"#),
        "adjacent query state should carry the effective key locator, got:\n{html}"
    );
}

#[test]
fn rsjs_lowers_typed_query_array_index_projection() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(QueryArrayProjectionIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("queryarrayprojectionisland_"))
        .expect("QueryArrayProjectionIsland should be registered")
        .js_source;
    assert!(
        js.contains("rsjs.__project(__rsjs_query_0.data(), [\"results\", \"0\", \"url\"])")
            && js.contains("rsjs.__project(__rsjs_query_0.data(), [\"results\", \"0\", \"id\"])")
            && js
                .contains("rsjs.__project(__rsjs_query_0.data(), [\"results\", \"0\", \"title\"])")
            && js
                .contains("rsjs.__project(__rsjs_query_0.data(), [\"results\", \"1\", \"title\"])"),
        "typed array indexing should lower to JSON pointer projections, got:\n{js}"
    );
    assert!(
        js.contains("bind_attr(__node_1, \"href\", \"url\""),
        "query-projected href should lower to a sanitized URL attr binding, got:\n{js}"
    );

    let html = block_on_ready(proa_core::render_to_string_async(
        QueryArrayProjectionIsland,
    ));
    assert!(
        html.contains(r#"href="https://latency.so""#)
            && html.contains(r#"data-result-id="latency""#)
            && html.contains(">latency.so</a>")
            && html.contains(">Proa docs</span>"),
        "server render should project query array values from the initial JSON, got:\n{html}"
    );
}

#[test]
fn rsjs_snapshots_dynamic_query_url_with_its_live_key() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(DynamicQueryUrlIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("dynamicqueryurlisland_"))
        .expect("DynamicQueryUrlIsland should be registered")
        .js_source;
    assert!(
        js.contains("key: [\"search\", \"dynamic-url\", __sig_0.get()]")
            && js.contains("fetcher: __sig_0.get(),")
            && !js.contains("fetcher: () =>")
            && js.contains("rebind_query(__rsjs_query_0, __rsjs_query_registration_0());"),
        "reactive request inputs should be snapshotted atomically with their live key, got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_query_0.refetch()"),
        "query refetch should remain available with a dynamic fetcher, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_query_backed_keyed_list() {
    fn assert_web_render<R: proa_core::WebRender<proa_core::NoLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(QueryBackedListIsland);

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("querybackedlistisland_"))
        .expect("QueryBackedListIsland should be registered")
        .js_source;
    assert!(
        js.contains("bind_list")
            && js.contains(
                "bind_list(__node_0, () => rsjs.__project(__rsjs_query_0.data(), [\"results\"]), {"
            ),
        "query-backed loops should lower to bind_list over the projected query data, got:\n{js}"
    );
    assert!(
        js.contains("deps: [],") && js.contains("extra_handles: [__rsjs_query_0],"),
        "query-backed list should subscribe to the query handle, got:\n{js}"
    );

    let html = block_on_ready(proa_core::render_to_string_async(QueryBackedListIsland));
    assert!(
        html.contains(r#"<a href="https://latency.so" data-rsjs-key="latency">latency.so</a>"#)
            && html.contains(r#"<a href="/docs" data-rsjs-key="docs">Proa docs</a>"#),
        "server render should replay rows from the typed query initializer, got:\n{html}"
    );
}

#[test]
fn rsjs_records_page_level_query_seed_table_entries() {
    let mut out = Vec::new();
    let seed_table = {
        let mut cx = proa_core::WebContext::with_buffer(&mut out);
        block_on_ready(proa_core::render_in_context_async(
            &mut cx,
            QueryInitialIsland,
        ))
        .unwrap();
        block_on_ready(proa_core::render_in_context_async(
            &mut cx,
            QueryInitialIsland,
        ))
        .unwrap();
        cx.rsjs_query_seed_table_json()
            .expect("query_with_initial should record a page-level seed")
    };

    assert!(
        seed_table.contains(r#""key":["stock:AAPL"]"#)
            && seed_table.contains(r#""active":true"#)
            && seed_table.contains(r#""currency":"USD""#)
            && seed_table.contains(r#""value":172.5"#),
        "page-level query seed table should carry the structural key and query state, got:\n{seed_table}"
    );
    assert_eq!(
        seed_table.matches(r#""key":["stock:AAPL"]"#).count(),
        2,
        "the table should mention the deduped key once as entry key and once in state, got:\n{seed_table}"
    );
}

#[test]
fn rsjs_query_with_initial_can_use_cx_dataloader() {
    fn assert_web_render<R: proa_core::WebRender<StockPriceLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(DataLoaderQueryInitialIsland);

    let mut out = Vec::new();
    let mut cx = proa_core::WebContext::with_loader_buffer(&mut out, Arc::new(StockPriceLoader));
    block_on_ready(proa_core::render_in_context_async(
        &mut cx,
        DataLoaderQueryInitialIsland,
    ))
    .unwrap();
    let seed_table = cx
        .rsjs_query_seed_table_json()
        .expect("DataLoader query initial should record a page-level seed");
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">174.25</span>") || html.contains(">174.25"),
        "server render should use the DataLoader-loaded query initial value, got:\n{html}"
    );
    assert!(
        html.contains(r#"data-currency="USD""#),
        "server render should project attrs from the DataLoader-loaded query value, got:\n{html}"
    );
    assert!(
        html.contains(r#""0":{"key":["stock","AAPL"]}"#),
        "adjacent query state should serialize the server-local key locator, got:\n{html}"
    );
    assert_stock_query_seed(&seed_table, "stock");
}

#[test]
fn descriptor_compilation_keeps_dataloader_query_initializer_with_child() {
    fn assert_web_render<R: proa_core::WebRender<StockPriceLoader, Vec<u8>>>(_root: R) {}
    assert_web_render(ParentWithDataLoaderQueryChild);

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.islands.iter().all(|entry| !entry
            .asset_id
            .starts_with("parentwithdataloaderquerychild_")),
        "a direct child edge alone must not copy a DataLoader query into a provisional parent \
         island"
    );
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("dataloaderqueryinitialchild_"))
        .expect("DataLoaderQueryChild should retain its provisional island artifact");
    assert!(
        child
            .js_source
            .contains("const __rsjs_query_0 = register_query({")
            && child.js_source.contains("key: [null]")
            && child
                .js_source
                .contains(r#"\"url\":\"/api/child-stocks/AAPL\""#),
        "the child artifact should own its query registration with a metadata-backed key, got:\n{}",
        child.js_source
    );
    let mut out = Vec::new();
    let mut cx = proa_core::WebContext::with_loader_buffer(&mut out, Arc::new(StockPriceLoader));
    block_on_ready(proa_core::render_in_context_async(
        &mut cx,
        ParentWithDataLoaderQueryChild,
    ))
    .unwrap();
    let seed_table = cx
        .rsjs_query_seed_table_json()
        .expect("inlined child query initial should record a page-level seed");
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">174.25</span>") || html.contains(">174.25"),
        "server render should use the direct child's query initial value, got:\n{html}"
    );
    assert!(
        html.contains(r#"data-currency="USD""#),
        "server render should project attrs from the direct child's query value, got:\n{html}"
    );
    assert!(
        html.contains(r#""0":{"key":["child_stock","AAPL"]}"#),
        "the child should dehydrate its provisional query key locator, got:\n{html}"
    );
    assert_stock_query_seed(&seed_table, "child_stock");
}

pub struct Counter {
    pub initial: i32,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| count.update(|c| c - 1)}>"-"</button>
                <span>{count.get()}</span>
                <button on_click={|_| count.update(|c| c + 1)}>"+"</button>
            </div>
        }
        .render(cx)
    }
}

/// Expected HTML (modulo the asset id, which is now a hashed
/// disambiguator). Both islands lower to the same IR shape, so the
/// rendered HTML structure must match exactly. The asset id format
/// is `counter_<hash8>` (see `rsjs_macros::asset`).
fn expected_counter_html(asset_id: &str) -> String {
    format!(
        concat!(
            r#"<rsjs-island data-rsjs-island-root="{id}">"#,
            r#"<div>"#,
            r#"<button data-rsjs-handler="1">-</button>"#,
            r#"<span data-rsjs-bind="3">0</span>"#,
            r#"<button data-rsjs-handler="4">+</button>"#,
            r#"</div>"#,
            r#"</rsjs-island>"#,
            r#"<script type="application/json" data-rsjs-island="{id}">"#,
            r#"{{"signals":["0"]}}"#,
            r#"</script>"#,
        ),
        id = asset_id
    )
}

#[test]
fn counter_renders_expected_hydration_html() {
    let html = render_to_string(Counter { initial: 0 });
    // Discover the asset id from the rendered output: it lives in
    // both `<rsjs-island data-rsjs-island-root="...">` and the seed `<script>`.
    let prefix = r#"<rsjs-island data-rsjs-island-root=""#;
    let after_prefix = html
        .strip_prefix(prefix)
        .expect("rendered HTML must start with the rsjs-island wrapper");
    let id_end = after_prefix.find('"').expect("missing closing quote");
    let asset_id = &after_prefix[..id_end];
    assert!(
        asset_id.starts_with("counter_"),
        "expected counter_<hash> asset id, got: {asset_id}"
    );
    assert_eq!(html, expected_counter_html(asset_id));
}

#[test]
fn counter_seed_script_uses_request_csp_nonce() {
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    cx.set_csp_nonce(proa_core::CspNonce::new("cnNqcy1zZWVkLW5vbmNl").unwrap());
    Counter { initial: 0 }.render(&mut cx).unwrap();
    let html = String::from_utf8(out).unwrap();
    assert!(html.contains(r#" data-rsjs-island="counter_"#,));
    assert!(html.contains(r#" nonce="cnNqcy1zZWVkLW5vbmNl">{"signals":["0"]}</script>"#));
    assert_eq!(html.matches(r#"nonce="cnNqcy1zZWVkLW5vbmNl""#).count(), 1);
}

/// Regression coverage for the round-3 review: a `#[rsjs(component, client)]` body with
/// a static button label (`"Submit"`) and ordinary HTML attributes
/// (`type="submit"`, `class="primary"`, `aria-label="..."`) must
/// preserve all of them in the rendered output. Before the fix,
/// emit_rust rebuilt HTML from the IR side-tables and silently
/// dropped author markup.
pub struct StaticContent;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticContent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div class="card" id="card-1">
                <button
                    type="submit"
                    class="primary"
                    aria-label="confirm action"
                    on_click={|_| count.update(|c| c + 1)}
                >
                    "Submit"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct StaticChildPanel;

#[rsjs(component)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticChildPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <aside class="child-panel">
                <strong>"Static child"</strong>
            </aside>
        }
        .render(cx)
    }
}

pub struct ParentWithStaticChildPanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithStaticChildPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <section>
                {StaticChildPanel {}}
                <span>{count.get()}</span>
            </section>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_preserves_user_authored_attrs_and_static_text() {
    let html = render_to_string(StaticContent);
    // The button must keep every author-written attribute *and* the
    // literal `Submit` body text.
    assert!(
        html.contains(r#"type="submit""#),
        "expected type=\"submit\" to survive emit_rust, got:\n{html}"
    );
    assert!(
        html.contains(r#"class="primary""#),
        "expected class=\"primary\" to survive emit_rust, got:\n{html}"
    );
    assert!(
        html.contains(r#"aria-label="confirm action""#),
        "expected aria-label=\"confirm action\" to survive emit_rust, got:\n{html}"
    );
    assert!(
        html.contains(">Submit<"),
        "expected literal text \"Submit\" inside <button>, got:\n{html}"
    );
    // The wrapping <div> preserves its attributes too.
    assert!(
        html.contains(r#"class="card""#),
        "expected class=\"card\" on the root div, got:\n{html}"
    );
    assert!(
        html.contains(r#"id="card-1""#),
        "expected id=\"card-1\" on the root div, got:\n{html}"
    );
}

#[test]
fn rsjs_inlines_server_only_child_without_fragment_runtime() {
    let html = render_to_string(ParentWithStaticChildPanel);
    assert!(
        html.contains("Static child") && html.contains("child-panel"),
        "server-only child markup should be rendered by the parent, got:\n{html}"
    );
    assert!(
        !html.contains("data-rsjs-component"),
        "server-only child should not be marked as a client fragment mount, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("staticchildpanel_")),
        "server-only child should not register a runtime island entry"
    );
    assert!(
        manifest
            .component_fragments
            .iter()
            .all(|entry| !entry.asset_id.starts_with("staticchildpanel_")),
        "server-only child should not register a component-fragment entry"
    );

    let parent = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("parentwithstaticchildpanel_"))
        .expect("ParentWithStaticChildPanel island should be registered");
    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("component_staticchildpanel_"),
        "parent JS should not import a server-only child fragment, got:\n{}",
        parent.js_source
    );
}

pub struct SvgUseTag;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for SvgUseTag {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <svg>
                    <use href="#heart" />
                </svg>
                <button on_click={|_| count.update(|c| c + 1)}>"+"</button>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_accepts_keyword_like_svg_use_tag() {
    let html = render_to_string(SvgUseTag);
    // SVG voids serialize self-closing in the HTML dialect so sibling
    // leaves cannot be re-parsed as nested children in foreign content.
    assert!(
        html.contains(r##"<use href="#heart"/>"##),
        "expected self-closing SVG <use> tag to render inside island, got:\n{html}"
    );
}

pub struct ServerOnlySlot;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerOnlySlot {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                {rsjs::server_only(UnsafeRaw(r##"<svg><use href="#heart"></use></svg>"##))}
                <button on_click={|_| count.update(|c| c + 1)}>"+"</button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_preserves_explicit_server_only_slot() {
    let html = render_to_string(ServerOnlySlot);
    assert!(
        html.contains(r##"<svg><use href="#heart"></use></svg>"##),
        "expected server_only UnsafeRaw content to render verbatim, got:\n{html}"
    );
    assert!(
        html.contains("data-rsjs-bind"),
        "normal reactive bindings should still be emitted, got:\n{html}"
    );
}

pub struct HandlerRefCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerRefCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let increment = rsjs::handler(|_| count.update(|c| c + 1));
        proa_macros::html_sync! {
            <div>
                <button on_click={increment(())}>"+"</button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_accepts_direct_handler_ref_calls() {
    let html = render_to_string(HandlerRefCounter);
    assert!(
        html.contains(r#"data-rsjs-handler="1""#),
        "direct handler-ref call should still bind a click handler, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerrefcounter_"))
        .expect("HandlerRefCounter island should be registered")
        .js_source;
    assert!(
        js.contains("__sig_0.update(")
            && js.contains("checked_int_add(__sig_0.get(), 1n, \"i64\")"),
        "handler ref body should inline into emitted JS, got:\n{js}"
    );
}

pub struct HandlerClientHelper;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerClientHelper {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let total = signal(21_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    console::log(handler_math::double(total.get()));
                    total.set(total.get() * 2_i64);
                }}>
                    "Double"
                </button>
                <span>{total.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_records_symbolic_client_helper_edges_from_handlers() {
    let html = render_to_string(HandlerClientHelper);
    assert!(
        html.contains(r#"data-rsjs-handler="1""#) && html.contains(r#"data-rsjs-bind="3""#),
        "handler helper fixture should render handler and binding markers, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerclienthelper_"))
        .expect("HandlerClientHelper island should be registered")
        .js_source;
    assert!(
        js.contains("__rsjs_unlinked_helper_edge_0(")
            && !js.contains("__rsjs_client_module_")
            && js.contains("__sig_0.set("),
        "descriptor-stage JS should retain the reachable handler helper as a symbolic \
         rustc-resolved edge; got:\n{js}"
    );
}

pub struct HandlerPayloadCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerPayloadCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let source = signal(41_i64);
        let selected = signal(0_i64);
        let apply = rsjs::handler(|value| selected.set(value));
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    apply(source.get());
                    selected.update(|v| v + 1);
                }}>
                    "Apply"
                </button>
                <span>{selected.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_substitutes_handler_ref_payloads_inside_block_handlers() {
    let _ = render_to_string(HandlerPayloadCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerpayloadcounter_"))
        .expect("HandlerPayloadCounter island should be registered")
        .js_source;
    assert!(
        js.contains("__sig_1.set(__sig_0.get())"),
        "handler payload should substitute into signal.set payload, got:\n{js}"
    );
    assert!(
        js.contains("__sig_1.update(")
            && js.contains("checked_int_add(__sig_1.get(), 1n, \"i64\")"),
        "ordinary block-handler statements should remain in source order, got:\n{js}"
    );
}

pub struct NamedEventHandlerRefButton;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NamedEventHandlerRefButton {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let picked = signal(String::new());
        let on_pick = rsjs::event_handler(
            |event: rsjs::MouseEvent<elem::Button>, label: &'static str| {
                console::log(label);
                picked.set(event.closest_attr("data-date"));
            },
        );

        proa_macros::html_sync! {
            <div>
                <button data_date="sample-day" on_click={on_pick("day")}>"Pick"</button>
                <span>{picked.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_accepts_named_event_handler_ref_calls_with_payloads() {
    let _ = render_to_string(NamedEventHandlerRefButton);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("namedeventhandlerrefbutton_"))
        .expect("NamedEventHandlerRefButton island should be registered")
        .js_source;
    assert!(
        js.contains("console.log(\"day\");"),
        "event handler payload should substitute into the named handler body, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set((event.target?.closest?.(\"[data-date]\")?.getAttribute(\"data-date\") ?? \"\"));"),
        "named event handler should lower event accessors with the real DOM event scope, got:\n{js}"
    );
}

pub struct EventOpsForm;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for EventOpsForm {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let checked = signal(false);
        let key = signal(String::new());
        let shortcut = signal(false);
        proa_macros::html_sync! {
            <div>
                <input on_input={|event| {
                    event.prevent_default();
                    let current = event.value();
                    console::log(current);
                    console::info(current);
                    console::debug(current);
                    console::warn(current);
                    console::error(current);
                    console::table(current);
                    console::time("eventops");
                    console::time_end("eventops");
                    console::group("eventops");
                    console::group_end();
                    value.set(current);
                    let is_checked = event.checked();
                    checked.set(is_checked);
                    key.set(event.key());
                    shortcut.set(event.is_cmd_or_ctrl("K"));
                }} />
                <span>{value.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_typed_event_operations() {
    let _ = render_to_string(EventOpsForm);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("eventopsform_"))
        .expect("EventOpsForm island should be registered")
        .js_source;
    assert!(
        js.contains("event.preventDefault();"),
        "prevent_default should lower to preventDefault, got:\n{js}"
    );
    assert!(
        js.contains("let __local_0 = (event.target?.value ?? \"\");")
            && js.contains("console.log(__local_0);"),
        "event.value() handler local should lower to a JS local used by console.log, got:\n{js}"
    );
    for expected in [
        "console.info(__local_0);",
        "console.debug(__local_0);",
        "console.warn(__local_0);",
        "console.error(__local_0);",
        "console.table(__local_0);",
        "console.time(\"eventops\");",
        "console.timeEnd(\"eventops\");",
        "console.group(\"eventops\");",
        "console.groupEnd();",
    ] {
        assert!(
            js.contains(expected),
            "expected console helper lowering `{expected}`, got:\n{js}"
        );
    }
    assert!(
        js.contains("__sig_0.set(__local_0);"),
        "target_value should reuse the lowered event.value() local, got:\n{js}"
    );
    assert!(
        js.contains("let __local_1 = !!event.target?.checked;")
            && js.contains("__sig_1.set(__local_1);"),
        "target_checked should lower to a JS local and signal set, got:\n{js}"
    );
    assert!(
        js.contains("__sig_2.set((event.key ?? \"\"));"),
        "key should lower to event.key expression, got:\n{js}"
    );
    assert!(
        js.contains("__sig_3.set(((!!event.metaKey || !!event.ctrlKey) && ((String(((event.key ?? \"\")) ?? \"\").toLowerCase()) === \"k\")));"),
        "cmd-or-ctrl shortcut should lower to modifier and normalized key checks, got:\n{js}"
    );
}

pub struct RichEventOpsForm;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RichEventOpsForm {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mouse_x = signal(0.0_f64);
        let input_data = signal(String::new());
        let attr_value = signal(String::new());
        let current_value = signal(String::new());
        let current_checked = signal(false);
        proa_macros::html_sync! {
            <div>
                <button
                    id="rich-event-button"
                    data_kind="primary"
                    on_click={|event: rsjs::MouseEvent<elem::Button>| {
                    mouse_x.set(event.client_x());
                    attr_value.set(event.closest_attr("data-kind"));
                    console::log(
                        event.type_(),
                        event.current_target_id(),
                        event.target_attr("id"),
                        event.current_target_attr("data-kind"),
                        event.button(),
                        event.buttons(),
                        event.detail(),
                    );
                }}
                >
                    "capture mouse"
                </button>
                <input on_input={|event: rsjs::InputEvent<elem::Input>| {
                    let input = event.current_target();
                    input_data.set(event.data());
                    current_value.set(input.value());
                    current_checked.set(input.checked());
                    console::log(
                        event.input_type(),
                        input.get_attribute("aria-label"),
                        input.client_width(),
                    );
                }} />
                <span>{mouse_x.get()}</span>
                <span>{input_data.get()}</span>
                <span>{attr_value.get()}</span>
                <span data_checked={current_checked.get()}>{current_value.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_richer_typed_event_accessors() {
    let _ = render_to_string(RichEventOpsForm);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("richeventopsform_"))
        .expect("RichEventOpsForm island should be registered")
        .js_source;
    assert!(
        js.contains("__sig_0.set((event.clientX ?? 0));"),
        "MouseEvent::client_x should lower to event.clientX, got:\n{js}"
    );
    assert!(
        js.contains("console.log((event.type ?? \"\"), (event.__rsjsCurrentTarget?.id ?? \"\"), (event.target?.getAttribute?.(\"id\") ?? \"\"), (event.__rsjsCurrentTarget?.getAttribute?.(\"data-kind\") ?? \"\"), (event.button ?? 0), (event.buttons ?? 0), (event.detail ?? 0));"),
        "typed event accessors should lower to DOM event reads, got:\n{js}"
    );
    assert!(
        js.contains("__sig_1.set((event.data ?? \"\"));")
            && js.contains("console.log((event.inputType ?? \"\"),"),
        "InputEvent accessors should lower to input event fields, got:\n{js}"
    );
    assert!(
        js.contains("__sig_2.set((event.target?.closest?.(\"[data-kind]\")?.getAttribute(\"data-kind\") ?? \"\"));"),
        "closest_attr should lower to a static closest attribute read, got:\n{js}"
    );
    assert!(
        js.contains("let __local_0 = event.__rsjsCurrentTarget;")
            && js.contains("__sig_3.set(((__local_0)?.value ?? \"\"));")
            && js.contains("__sig_4.set(((__local_0)?.checked ?? false));")
            && js.contains("((__local_0)?.getAttribute?.(\"aria-label\") ?? \"\")")
            && js.contains("((__local_0)?.clientWidth ?? 0)"),
        "first-class typed currentTarget locals should lower to delegated DOM reads, got:\n{js}"
    );
}

pub struct CommonDomEventsForm;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CommonDomEventsForm {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let key = signal(String::new());
        let pointer_x = signal(0.0_f64);
        let touch_x = signal(0.0_f64);
        let wheel_y = signal(0.0_f64);
        let scroll_top = signal(0.0_f64);
        let pasted = signal(String::new());
        let before_input = signal(String::new());
        let composition = signal(String::new());
        let drop_name = signal(String::new());
        let selected_name = signal(String::new());
        let selected_text = signal(String::new());
        let drop_text = signal(String::new());
        let focused = signal(false);
        let toggle_state = signal(String::new());
        let route_search = signal(String::new());
        let hash_url = signal(String::new());
        let storage_value = signal(String::new());
        let resized_width = signal(0_i32);
        let window_scroll_y = signal(0.0_f64);
        proa_macros::html_sync! {
            <div>
                <input
                    type="file"
                    on_focus={|_| focused.set(true)}
                    on_blur={|_| focused.set(false)}
                    on_change={|event: rsjs::ChangeEvent| {
                        selected_name.set(event.target_first_file_name());
                        event.target_first_file_text_into_or(&selected_text, "unreadable");
                        console::log(
                            event.target_file_count(),
                            event.target_first_file_type(),
                            event.target_first_file_size(),
                            event.target_file_name_at(1),
                            event.target_file_type_at(1),
                            event.target_file_size_at(1),
                        );
                    }}
                    on_beforeinput={|event: rsjs::InputEvent| {
                        before_input.set(event.data());
                    }}
                    on_compositionupdate={|event: rsjs::CompositionEvent| {
                        composition.set(event.data());
                    }}
                    on_paste={|event: rsjs::ClipboardEvent| {
                        event.prevent_default();
                        pasted.set(event.clipboard_text());
                    }}
                    on_keydown={|event: rsjs::KeyboardEvent| {
                        key.set(event.key());
                    }}
                    on_keyup={|event: rsjs::KeyboardEvent| {
                        console::log(event.code());
                    }}
                />
                <button
                    on_pointerdown={|event: rsjs::PointerEvent| {
                        pointer_x.set(event.client_x());
                        console::log(
                            event.pointer_id(),
                            event.pointer_type(),
                            event.is_primary(),
                            event.pressure(),
                            event.tangential_pressure(),
                            event.tilt_x(),
                            event.tilt_y(),
                            event.twist(),
                            event.pointer_width(),
                            event.pointer_height(),
                        );
                    }}
                    on_pointerup={|event: rsjs::PointerEvent| {
                        console::log(event.pointer_id());
                    }}
                    on_pointermove={|event: rsjs::PointerEvent| {
                        console::log(event.pressure());
                    }}
                    on_pointerover={|event: rsjs::PointerEvent| {
                        console::log(event.pointer_type());
                    }}
                    on_pointerout={|event: rsjs::PointerEvent| {
                        console::log(event.is_primary());
                    }}
                    on_pointerenter={|event: rsjs::PointerEvent| {
                        console::log(event.client_y());
                    }}
                    on_pointerleave={|event: rsjs::PointerEvent| {
                        console::log(event.page_y());
                    }}
                    on_gotpointercapture={|event: rsjs::PointerEvent| {
                        console::log(event.pointer_id());
                    }}
                    on_lostpointercapture={|event: rsjs::PointerEvent| {
                        console::log(event.pointer_id());
                    }}
                    on_mousemove={|event: rsjs::MouseEvent| {
                        console::log(event.movement_x());
                    }}
                    on_scroll={|event| {
                        scroll_top.set(event.current_target_scroll_top());
                        console::log(event.current_target_scroll_left());
                    }}
                    on_wheel={|event: rsjs::WheelEvent| {
                        event.prevent_default();
                        wheel_y.set(event.delta_y());
                        console::log(event.delta_x(), event.delta_z(), event.delta_mode());
                    }}
                    on_touchstart={|event: rsjs::TouchEvent| {
                        touch_x.set(event.changed_touch_client_x());
                        console::log(
                            event.changed_touch_identifier(),
                            event.changed_touch_count(),
                            event.changed_touch_client_x_at(1),
                            event.target_touch_count(),
                            event.target_touch_client_y_at(0),
                            event.touch_count(),
                            event.touch_identifier_at(0),
                        );
                    }}
                    on_touchmove={|event: rsjs::TouchEvent| {
                        console::log(event.changed_touch_client_y());
                    }}
                    on_touchend={|event: rsjs::TouchEvent| {
                        console::log(event.changed_touch_page_x());
                    }}
                    on_touchcancel={|event: rsjs::TouchEvent| {
                        console::log(event.changed_touch_screen_y());
                    }}
                    on_dragstart={|event: rsjs::DragEvent| {
                        console::log(event.client_x());
                    }}
                    on_dragenter={|event: rsjs::DragEvent| {
                        event.prevent_default();
                        console::log(event.data_transfer_text());
                    }}
                    on_dragover={|event: rsjs::DragEvent| {
                        event.prevent_default();
                        console::log(event.data_transfer_file_count());
                    }}
                    on_dragleave={|event: rsjs::DragEvent| {
                        console::log(event.data_transfer_first_file_type());
                    }}
                    on_drop={|event: rsjs::DragEvent| {
                        event.prevent_default();
                        drop_name.set(event.data_transfer_first_file_name());
                        event.data_transfer_first_file_text_into(&drop_text);
                        console::log(
                            event.data_transfer_first_file_size(),
                            event.data_transfer_file_name_at(1),
                            event.data_transfer_file_size_at(1),
                            event.data_transfer_item_count(),
                            event.data_transfer_item_kind_at(0),
                            event.data_transfer_item_type_at(0),
                        );
                    }}
                    on_dragend={|event: rsjs::DragEvent| {
                        console::log(event.type_());
                    }}
                >
                    "track"
                </button>
                <details on_toggle={|event: rsjs::ToggleEvent| {
                    toggle_state.set(event.new_state());
                    console::log(event.old_state());
                }}>
                    <summary>"More"</summary>
                </details>
                <div
                    popover="manual"
                    on_beforetoggle={|event: rsjs::ToggleEvent| {
                    event.prevent_default();
                    console::log(event.old_state(), event.new_state());
                }}
                >
                    "Popover"
                </div>
                <div
                    on_animationstart={|event: rsjs::AnimationEvent| {
                        console::log(event.animation_name(), event.elapsed_time(), event.pseudo_element());
                    }}
                    on_animationiteration={|event: rsjs::AnimationEvent| {
                        console::log(event.animation_name());
                    }}
                    on_animationend={|event: rsjs::AnimationEvent| {
                        console::log(event.animation_name());
                    }}
                    on_animationcancel={|event: rsjs::AnimationEvent| {
                        console::log(event.animation_name());
                    }}
                    on_transitionrun={|event: rsjs::TransitionEvent| {
                        console::log(event.property_name(), event.elapsed_time(), event.pseudo_element());
                    }}
                    on_transitionstart={|event: rsjs::TransitionEvent| {
                        console::log(event.property_name());
                    }}
                    on_transitionend={|event: rsjs::TransitionEvent| {
                        console::log(event.property_name());
                    }}
                    on_transitioncancel={|event: rsjs::TransitionEvent| {
                        console::log(event.property_name());
                    }}
                >
                    "Motion"
                </div>
                <div
                    on_window_popstate={|event: rsjs::PopStateEvent<rsjs::web::dom::Window>| {
                        route_search.set(rsjs::location::search());
                        console::log(event.type_());
                    }}
                    on_window_hashchange={|event: rsjs::HashChangeEvent<rsjs::web::dom::Window>| {
                        hash_url.set(event.new_url());
                        console::log(event.old_url());
                    }}
                    on_window_storage={|event: rsjs::StorageEvent<rsjs::web::dom::Window>| {
                        storage_value.set(event.storage_new_value());
                        console::log(
                            event.storage_key(),
                            event.storage_old_value(),
                            event.storage_url(),
                            event.storage_area(),
                        );
                    }}
                    on_window_resize={|_: rsjs::Event<rsjs::web::dom::Window>| {
                        resized_width.set(rsjs::window::inner_width());
                    }}
                    on_window_scroll={|_: rsjs::Event<rsjs::web::dom::Window>| {
                        window_scroll_y.set(rsjs::window::scroll_y());
                    }}
                >
                    "Window events"
                </div>
                <span>{key.get()}</span>
                <span>{pointer_x.get()}</span>
                <span>{touch_x.get()}</span>
                <span>{wheel_y.get()}</span>
                <span>{scroll_top.get()}</span>
                <span>{pasted.get()}</span>
                <span>{before_input.get()}</span>
                <span>{composition.get()}</span>
                <span>{drop_name.get()}</span>
                <span>{selected_name.get()}</span>
                <span>{selected_text.get()}</span>
                <span>{drop_text.get()}</span>
                <span data-focused={focused.get()}></span>
                <span>{toggle_state.get()}</span>
                <span>{route_search.get()}</span>
                <span>{hash_url.get()}</span>
                <span>{storage_value.get()}</span>
                <span>{resized_width.get()}</span>
                <span>{window_scroll_y.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_common_dom_event_names() {
    let _ = render_to_string(CommonDomEventsForm);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("commondomeventsform_"))
        .expect("CommonDomEventsForm island should be registered")
        .js_source;
    for event in [
        "blur",
        "focus",
        "keydown",
        "keyup",
        "mousemove",
        "paste",
        "pointerdown",
        "pointerup",
        "pointermove",
        "pointerover",
        "pointerout",
        "pointerenter",
        "pointerleave",
        "gotpointercapture",
        "lostpointercapture",
        "scroll",
        "touchcancel",
        "touchend",
        "touchmove",
        "touchstart",
        "wheel",
        "beforeinput",
        "compositionupdate",
        "dragstart",
        "dragenter",
        "dragover",
        "dragleave",
        "drop",
        "dragend",
        "toggle",
        "beforetoggle",
        "animationstart",
        "animationiteration",
        "animationend",
        "animationcancel",
        "transitionrun",
        "transitionstart",
        "transitionend",
        "transitioncancel",
    ] {
        assert!(
            js.contains(&format!("bind_delegated_events(root, \"{event}\"")),
            "expected delegated binding for {event}, got:\n{js}"
        );
    }
    assert!(
        js.contains("__sig_0.set((event.key ?? \"\"));"),
        "keydown should lower KeyboardEvent::key, got:\n{js}"
    );
    assert!(
        js.contains("__sig_1.set((event.clientX ?? 0));"),
        "pointerdown should lower PointerEvent::client_x, got:\n{js}"
    );
    assert!(
        js.contains("console.log((event.pointerId ?? 0), (event.pointerType ?? \"\"), !!event.isPrimary, (event.pressure ?? 0), (event.tangentialPressure ?? 0), (event.tiltX ?? 0), (event.tiltY ?? 0), (event.twist ?? 0), (event.width ?? 0), (event.height ?? 0));"),
        "pointer event accessors should lower to PointerEvent fields, got:\n{js}"
    );
    assert!(
        js.contains("__sig_2.set((event.changedTouches?.[0]?.clientX ?? 0));"),
        "touchstart should lower TouchEvent::changed_touch_client_x, got:\n{js}"
    );
    assert!(
        js.contains("__sig_3.set((event.deltaY ?? 0));")
            && js.contains(
                "console.log((event.deltaX ?? 0), (event.deltaZ ?? 0), (event.deltaMode ?? 0));"
            ),
        "wheel accessors should lower to WheelEvent delta fields, got:\n{js}"
    );
    assert!(
        js.contains("__sig_4.set((event.__rsjsCurrentTarget?.scrollTop ?? 0));")
            && js.contains("console.log((event.__rsjsCurrentTarget?.scrollLeft ?? 0));"),
        "scroll accessors should lower to current target scroll fields, got:\n{js}"
    );
    assert!(
        js.contains("__sig_6.set((event.data ?? \"\"));")
            && js.contains("__sig_7.set((event.data ?? \"\"));"),
        "beforeinput/composition data accessors should lower to event.data, got:\n{js}"
    );
    assert!(
        js.contains(".set((event.newState ?? \"\"));")
            && js.contains("console.log((event.oldState ?? \"\"));")
            && js.contains("console.log((event.oldState ?? \"\"), (event.newState ?? \"\"));"),
        "toggle accessors should lower to ToggleEvent oldState/newState fields, got:\n{js}"
    );
    assert!(
        js.contains(
            "console.log((event.animationName ?? \"\"), (event.elapsedTime ?? 0), (event.pseudoElement ?? \"\"));"
        ) && js.contains(
            "console.log((event.propertyName ?? \"\"), (event.elapsedTime ?? 0), (event.pseudoElement ?? \"\"));"
        ),
        "animation/transition accessors should lower to event fields, got:\n{js}"
    );
    for event in ["popstate", "hashchange", "storage", "resize", "scroll"] {
        let needle = format!(", \"{event}\", __handler_");
        assert!(
            js.contains("bind_window_event(") && js.contains(&needle),
            "expected window binding for {event}, got:\n{js}"
        );
    }
    assert!(
        js.contains("window.location.search ?? \"\"")
            && js.contains("console.log((event.type ?? \"\"));"),
        "popstate handler should lower location read and event type, got:\n{js}"
    );
    assert!(
        js.contains("window.scrollY ?? 0"),
        "window scroll handler should lower window.scroll_y(), got:\n{js}"
    );
    assert!(
        js.contains(".set((event.newURL ?? \"\"));")
            && js.contains("console.log((event.oldURL ?? \"\"));"),
        "hashchange accessors should lower to oldURL/newURL, got:\n{js}"
    );
    assert!(
        js.contains(".set((event.newValue ?? \"\"));")
            && js.contains("console.log((event.key ?? \"\"), (event.oldValue ?? \"\"), (event.url ?? \"\"), (() => { try { return event.storageArea === window.localStorage ? \"local\" : (event.storageArea === window.sessionStorage ? \"session\" : \"\"); } catch (_) { return \"\"; } })());"),
        "storage event accessors should lower to StorageEvent fields, got:\n{js}"
    );
    assert!(
        js.contains("__sig_9.set((event.target?.files?.[0]?.name ?? \"\"));")
            && js.contains(
                "console.log((event.target?.files?.length ?? 0), (event.target?.files?.[0]?.type ?? \"\"), (event.target?.files?.[0]?.size ?? 0),"
            )
            && js.contains("(event.target?.files?.[Math.trunc(Number(1) || 0)]?.name ?? \"\")")
            && js.contains("(event.target?.files?.[Math.trunc(Number(1) || 0)]?.type ?? \"\")")
            && js.contains("(event.target?.files?.[Math.trunc(Number(1) || 0)]?.size ?? 0)"),
        "file input event accessors should lower to target.files reads, got:\n{js}"
    );
    assert!(
        js.contains("{ const __rsjs_file = event.target?.files?.[0]; let __rsjs_file_text = null; try { __rsjs_file_text = __rsjs_file?.text?.(); } catch (_) {} if (__rsjs_file_text) { __rsjs_file_text.then((__text) => __sig_10.set(String(__text ?? \"\"))).catch(() => __sig_10.set(\"unreadable\")); } else { __sig_10.set(\"unreadable\"); } }")
            && js.contains("{ const __rsjs_file = event.dataTransfer?.files?.[0]; let __rsjs_file_text = null; try { __rsjs_file_text = __rsjs_file?.text?.(); } catch (_) {} if (__rsjs_file_text) { __rsjs_file_text.then((__text) => __sig_11.set(String(__text ?? \"\"))).catch(() => __sig_11.set(\"\")); } else { __sig_11.set(\"\"); } }"),
        "file text helpers should lower to async File.text() signal writes, got:\n{js}"
    );
    assert!(
        js.contains("console.log((event.dataTransfer?.getData?.(\"text/plain\") ?? \"\"));")
            && js.contains("console.log((event.dataTransfer?.files?.length ?? 0));")
            && js.contains("console.log((event.dataTransfer?.files?.[0]?.type ?? \"\"));")
            && js.contains("__sig_8.set((event.dataTransfer?.files?.[0]?.name ?? \"\"));")
            && js.contains(
                "console.log((event.dataTransfer?.files?.[0]?.size ?? 0), (event.dataTransfer?.files?.[Math.trunc(Number(1) || 0)]?.name ?? \"\"),"
            )
            && js.contains("(event.dataTransfer?.files?.[Math.trunc(Number(1) || 0)]?.size ?? 0)")
            && js.contains("(event.dataTransfer?.items?.length ?? 0)")
            && js.contains("(event.dataTransfer?.items?.[Math.trunc(Number(0) || 0)]?.kind ?? \"\")")
            && js.contains("(event.dataTransfer?.items?.[Math.trunc(Number(0) || 0)]?.type ?? \"\")"),
        "drag/drop accessors should lower to dataTransfer reads, got:\n{js}"
    );
    assert!(
        js.contains(
            "console.log((event.changedTouches?.[0]?.identifier ?? 0), (event.changedTouches?.length ?? 0),"
        )
            && js.contains(
                "(event.changedTouches?.[Math.trunc(Number(1) || 0)]?.clientX ?? 0)"
            )
            && js.contains("(event.targetTouches?.length ?? 0)")
            && js.contains(
                "(event.targetTouches?.[Math.trunc(Number(0) || 0)]?.clientY ?? 0)"
            )
            && js.contains("(event.touches?.length ?? 0)")
            && js.contains("(event.touches?.[Math.trunc(Number(0) || 0)]?.identifier ?? 0)")
            && js.contains("console.log((event.changedTouches?.[0]?.clientY ?? 0));")
            && js.contains("console.log((event.changedTouches?.[0]?.pageX ?? 0));")
            && js.contains("console.log((event.changedTouches?.[0]?.screenY ?? 0));"),
        "touch accessors should lower to changedTouches reads, got:\n{js}"
    );
    assert!(
        js.contains("__sig_5.set((event.clipboardData?.getData?.(\"text/plain\") ?? \"\"));"),
        "paste should lower ClipboardEvent::clipboard_text, got:\n{js}"
    );
    assert!(
        js.contains("console.log((event.movementX ?? 0));"),
        "mousemove should lower MouseEvent::movement_x, got:\n{js}"
    );
}

pub struct BrowserEffects;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BrowserEffects {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let hash = signal("filters=open".to_string());
        let clipboard_default = signal(String::new());
        let clipboard_fallback = signal(String::new());
        let registration_options = signal(r#"{"challenge":"register"}"#.to_string());
        let authentication_options = signal(r#"{"challenge":"authenticate"}"#.to_string());
        let registration_default = signal(String::new());
        let registration_fallback = signal(String::new());
        let authentication_default = signal(String::new());
        let authentication_fallback = signal(String::new());
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    rsjs::window::scroll_to(0_i64, 100_i64);
                    rsjs::window::scroll_by(0_i64, 20_i64);
                    rsjs::window::print();
                    rsjs::location::set_hash(hash.get());
                    rsjs::location::assign("/catalog/filters");
                    rsjs::location::replace(hash.get());
                    rsjs::location::reload();
                    rsjs::history::push_state("/catalog#filters");
                    rsjs::history::replace_state("/catalog#filters=open");
                    rsjs::history::back();
                    rsjs::history::forward();
                    rsjs::history::go(1_i64);
                    rsjs::document::dispatch_event("filters:changed");
                    rsjs::document::dispatch_event_with_detail("filters:changed", hash.get());
                    rsjs::document::set_title(hash.get());
                    rsjs::clipboard::write_text(hash.get());
                    rsjs::clipboard::read_text_into(&clipboard_default);
                    rsjs::clipboard::read_text_into_or(&clipboard_fallback, "denied");
                    rsjs::webauthn::create_json_into(registration_options.get(), &registration_default);
                    rsjs::webauthn::create_json_into_or(registration_options.get(), &registration_fallback, "registration failed");
                    rsjs::webauthn::get_json_into(authentication_options.get(), &authentication_default);
                    rsjs::webauthn::get_json_into_or(authentication_options.get(), &authentication_fallback, "authentication failed");
                    rsjs::local_storage::set_item("filters", hash.get());
                    rsjs::local_storage::remove_item("filters:stale");
                    rsjs::session_storage::set_item("filters", hash.get());
                    rsjs::session_storage::remove_item("filters:stale");
                    rsjs::url::set_search_param("filters", hash.get());
                    rsjs::url::delete_search_param("stale");
                    rsjs::url::push_search_param("page", "2");
                    rsjs::url::push_delete_search_param("modal");
                }}>
                    "sync url"
                </button>
                <span>{clipboard_default.get()}</span>
                <span>{clipboard_fallback.get()}</span>
                <span>{registration_default.get()}</span>
                <span>{registration_fallback.get()}</span>
                <span>{authentication_default.get()}</span>
                <span>{authentication_fallback.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_browser_api_effects() {
    let _ = render_to_string(BrowserEffects);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("browsereffects_"))
        .expect("BrowserEffects island should be registered")
        .js_source;
    assert!(
        js.contains("window.scrollTo?.(Number(0n), Number(100n));"),
        "window scroll_to should lower to window.scrollTo, got:\n{js}"
    );
    assert!(
        js.contains("window.scrollBy?.(Number(0n), Number(20n));"),
        "window scroll_by should lower to window.scrollBy, got:\n{js}"
    );
    assert!(
        js.contains("window.print?.();"),
        "window print should lower to window.print, got:\n{js}"
    );
    assert!(
        js.contains("window.location.hash = String(__sig_0.get());"),
        "location hash should lower to a window.location write, got:\n{js}"
    );
    assert!(
        js.contains("window.location.assign(String(\"/catalog/filters\"));"),
        "location assign should lower to window.location.assign, got:\n{js}"
    );
    assert!(
        js.contains("window.location.replace(String(__sig_0.get()));"),
        "location replace should lower to window.location.replace, got:\n{js}"
    );
    assert!(
        js.contains("window.location.reload();"),
        "location reload should lower to window.location.reload, got:\n{js}"
    );
    assert!(
        js.contains("window.history?.pushState?.(null, \"\", String(\"/catalog#filters\"));"),
        "push_state should lower to history.pushState, got:\n{js}"
    );
    assert!(
        js.contains(
            "window.history?.replaceState?.(null, \"\", String(\"/catalog#filters=open\"));"
        ),
        "replace_state should lower to history.replaceState, got:\n{js}"
    );
    assert!(
        js.contains("window.history?.back?.();"),
        "history back should lower to history.back, got:\n{js}"
    );
    assert!(
        js.contains("window.history?.forward?.();"),
        "history forward should lower to history.forward, got:\n{js}"
    );
    assert!(
        js.contains("window.history?.go?.(Number(1n));"),
        "history go should lower to history.go, got:\n{js}"
    );
    assert!(
        js.contains("document.dispatchEvent(new CustomEvent(String(\"filters:changed\")));"),
        "dispatch_event should lower to CustomEvent without detail, got:\n{js}"
    );
    assert!(
        js.contains(
            "document.dispatchEvent(new CustomEvent(String(\"filters:changed\"), { detail: __sig_0.get() }));"
        ),
        "dispatch_event_with_detail should lower to CustomEvent detail, got:\n{js}"
    );
    assert!(
        js.contains("document.title = String(__sig_0.get());"),
        "set_title should lower to a document.title write, got:\n{js}"
    );
    assert!(
        js.contains("navigator.clipboard?.writeText?.(String(__sig_0.get()));"),
        "clipboard write_text should lower to navigator.clipboard.writeText, got:\n{js}"
    );
    assert!(
        js.contains("navigator.clipboard?.readText?.();")
            && js.contains(".then((__text) => __sig_1.set(String(__text ?? \"\"))).catch(() => __sig_1.set(\"\"));")
            && js.contains(".then((__text) => __sig_2.set(String(__text ?? \"\"))).catch(() => __sig_2.set(\"denied\"));")
            && js.contains("} else { __sig_2.set(\"denied\"); }"),
        "clipboard read_text_into helpers should lower to guarded readText promise writes, got:\n{js}"
    );
    assert!(
        js.contains(
            "globalThis.PublicKeyCredential?.parseCreationOptionsFromJSON?.(JSON.parse(String(__sig_3.get())))"
        ) && js.contains(
            "globalThis.navigator?.credentials?.create?.({ publicKey: __rsjs_webauthn_options });"
        ) && js.contains(
            "globalThis.PublicKeyCredential?.parseRequestOptionsFromJSON?.(JSON.parse(String(__sig_4.get())))"
        ) && js.contains(
            "globalThis.navigator?.credentials?.get?.({ publicKey: __rsjs_webauthn_options });"
        ) && js.contains(".set(JSON.stringify(__json));")
            && js.contains("__sig_6.set(\"registration failed\")")
            && js.contains("__sig_8.set(\"authentication failed\")"),
        "webauthn helpers should lower to guarded PublicKeyCredential JSON helpers, got:\n{js}"
    );
    assert!(
        js.contains("try { window.localStorage?.setItem?.(String(\"filters\"), String(__sig_0.get())); } catch (_) {}"),
        "local_storage::set_item should lower to localStorage.setItem, got:\n{js}"
    );
    assert!(
        js.contains(
            "try { window.localStorage?.removeItem?.(String(\"filters:stale\")); } catch (_) {}"
        ),
        "local_storage::remove_item should lower to localStorage.removeItem, got:\n{js}"
    );
    assert!(
        js.contains("try { window.sessionStorage?.setItem?.(String(\"filters\"), String(__sig_0.get())); } catch (_) {}"),
        "session_storage::set_item should lower to sessionStorage.setItem, got:\n{js}"
    );
    assert!(
        js.contains(
            "try { window.sessionStorage?.removeItem?.(String(\"filters:stale\")); } catch (_) {}"
        ),
        "session_storage::remove_item should lower to sessionStorage.removeItem, got:\n{js}"
    );
    assert!(
        js.contains("const __url = new URL(window.location.href); __url.searchParams.set(String(\"filters\"), String(__sig_0.get())); window.history?.replaceState?.(null, \"\", __url.href);"),
        "url::set_search_param should lower to URLSearchParams.set + replaceState, got:\n{js}"
    );
    assert!(
        js.contains("const __url = new URL(window.location.href); __url.searchParams.delete(String(\"stale\")); window.history?.replaceState?.(null, \"\", __url.href);"),
        "url::delete_search_param should lower to URLSearchParams.delete + replaceState, got:\n{js}"
    );
    assert!(
        js.contains("const __url = new URL(window.location.href); __url.searchParams.set(String(\"page\"), String(\"2\")); window.history?.pushState?.(null, \"\", __url.href);"),
        "url::push_search_param should lower to URLSearchParams.set + pushState, got:\n{js}"
    );
    assert!(
        js.contains("const __url = new URL(window.location.href); __url.searchParams.delete(String(\"modal\")); window.history?.pushState?.(null, \"\", __url.href);"),
        "url::push_delete_search_param should lower to URLSearchParams.delete + pushState, got:\n{js}"
    );
}

pub struct BrowserReads;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BrowserReads {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let viewport_width = signal(0_i32);
        let storage_enabled = signal(false);
        let storage_count = signal(0_i64);
        let storage_ratio = signal(0.0_f64);
        proa_macros::html_sync! {
            <div
                data-current-url={rsjs::location::href()}
                data-viewport-width={rsjs::window::inner_width()}
                data-query-filter={rsjs::url::search_param_or("filters", "all")}
            >
                <span>{rsjs::location::search()}</span>
                <button on_click={|_| {
                    value.set(rsjs::url::search_param("filters"));
                    console::log(rsjs::local_storage::get_item_or(String::new(), String::new()));
                    viewport_width.set(rsjs::window::inner_width());
                    console::log(rsjs::window::inner_height(), rsjs::window::scroll_x(), rsjs::window::scroll_y());
                    console::log(rsjs::session_storage::get_item(String::new()));
                    storage_enabled.set(rsjs::local_storage::get_bool_or("enabled", false));
                    storage_count.set(rsjs::local_storage::get_i64_or("count", 0_i64));
                    storage_ratio.set(rsjs::session_storage::get_f64_or("ratio", 0.0_f64));
                    rsjs::history::replace_state(rsjs::location::pathname());
                }}>
                    "read browser state"
                </button>
                <span>{viewport_width.get()}</span>
                <span
                    data-storage-enabled={storage_enabled.get()}
                    data-storage-count={storage_count.get()}
                    data-storage-ratio={storage_ratio.get()}
                ></span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_browser_api_reads() {
    let html = render_to_string(BrowserReads);
    assert!(
        html.contains(r#"data-current-url="""#),
        "SSR should use an empty fallback for browser-only location reads, got:\n{html}"
    );
    assert!(
        html.contains(r#"data-viewport-width="0""#),
        "SSR should use a zero fallback for browser-only window reads, got:\n{html}"
    );
    assert!(
        html.contains(r#"data-query-filter="all""#),
        "SSR should use the explicit fallback for browser-only URL query reads, got:\n{html}"
    );
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("browserreads_"))
        .expect("BrowserReads island should be registered")
        .js_source;
    assert!(
        js.contains("window.location.href ?? \"\""),
        "location::href should lower to a location read, got:\n{js}"
    );
    assert!(
        js.contains("window.location.search ?? \"\""),
        "location::search should lower to a location read, got:\n{js}"
    );
    assert!(
        js.contains("window.location.pathname ?? \"\""),
        "location::pathname should lower to a location read, got:\n{js}"
    );
    assert!(
        js.contains("window.innerWidth ?? 0")
            && js.contains("window.innerHeight ?? 0")
            && js.contains("window.scrollX ?? 0")
            && js.contains("window.scrollY ?? 0"),
        "window CSSOM reads should lower to guarded window fields, got:\n{js}"
    );
    assert!(
        js.contains("window.localStorage?.getItem?.(String(\"\"))"),
        "local_storage::get_item_or should lower to localStorage.getItem, got:\n{js}"
    );
    assert!(
        js.contains("return __v == null ? String(\"\") : __v;"),
        "local_storage::get_item_or should use the explicit fallback, got:\n{js}"
    );
    assert!(
        js.contains("window.sessionStorage?.getItem?.(String(\"\"))"),
        "session_storage::get_item should lower to sessionStorage.getItem, got:\n{js}"
    );
    assert!(
        js.contains("window.localStorage?.getItem?.(String(\"enabled\"))")
            && js.contains("return typeof __parsed === \"boolean\" ? __parsed : false;"),
        "local_storage::get_bool_or should JSON-parse booleans with fallback, got:\n{js}"
    );
    assert!(
        js.contains("window.localStorage?.getItem?.(String(\"count\"))")
            && js.contains("/^(?:0|-?[1-9][0-9]*)$/.test(__v)")
            && js.contains("const __parsed = BigInt(__v);")
            && js.contains(
                "__parsed >= -9223372036854775808n && __parsed <= 9223372036854775807n ? __parsed : 0n;"
            ),
        "local_storage::get_i64_or should parse canonical full-range i64 decimals with a BigInt fallback, got:\n{js}"
    );
    assert!(
        js.contains("window.sessionStorage?.getItem?.(String(\"ratio\"))")
            && js.contains("Number.isFinite(__parsed) ? __parsed : 0.0;"),
        "session_storage::get_f64_or should JSON-parse finite numbers with fallback, got:\n{js}"
    );
    assert!(
        js.contains("new URLSearchParams((typeof window !== \"undefined\" && window.location) ? (window.location.search ?? \"\") : \"\")"),
        "url::search_param should lower to URLSearchParams over window.location.search, got:\n{js}"
    );
    assert!(
        js.contains("return __v == null ? String(\"all\") : __v;"),
        "url::search_param_or should use the explicit fallback, got:\n{js}"
    );
}

pub struct EncodingHelpers;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for EncodingHelpers {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let text = signal(String::from("hi"));
        let decoded = signal(String::new());
        proa_macros::html_sync! {
            <div data-token={rsjs::encoding::base64url_encode(rsjs::encoding::utf8_bytes(text.get()))}>
                <button
                    data-token="aGk"
                    on_click={|event| {
                        let bytes = rsjs::encoding::base64url_decode(event.current_target_attr("data-token"));
                        decoded.set(rsjs::encoding::utf8_string(bytes));
                        console::log(rsjs::encoding::base64_encode(rsjs::encoding::utf8_bytes(event.value())));
                    }}
                >
                    "Decode"
                </button>
                <span>{decoded.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_encoding_helpers() {
    let _ = render_to_string(EncodingHelpers);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("encodinghelpers_"))
        .expect("EncodingHelpers island should be registered")
        .js_source;

    assert!(
        js.contains("new TextEncoder().encode"),
        "utf8_bytes should lower to TextEncoder, got:\n{js}"
    );
    assert!(
        js.contains("new TextDecoder().decode"),
        "utf8_string should lower to TextDecoder, got:\n{js}"
    );
    assert!(
        js.contains("btoa(__s)"),
        "base64/base64url encode should lower through btoa, got:\n{js}"
    );
    assert!(
        js.contains("replace(/\\+/g, \"-\").replace(/\\//g, \"_\").replace(/=+$/g, \"\")"),
        "base64url encode should emit url-safe replacement, got:\n{js}"
    );
    assert!(
        js.contains("atob(__v)"),
        "base64url decode should lower through atob, got:\n{js}"
    );
}

pub struct ConditionalValues;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ConditionalValues {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let active = signal(true);
        proa_macros::html_sync! {
            <div class={if active.get() { "active" } else { "idle" }}>
                <span>{if active.get() { "Active" } else { "Idle" }}</span>
                <button on_click={|_| {
                    rsjs::history::replace_state(if active.get() { "/active" } else { "/idle" });
                }}>
                    "sync"
                </button>
            </div>
        }
        .render(cx)
    }
}

pub struct TemplateElseIfSections;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TemplateElseIfSections {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = signal("idle".to_string());
        proa_macros::html_sync! {
            <div>
                {if state.get() == "active" {
                    proa_macros::html_sync! {
                        <span>"Active"</span>
                    }
                } else if state.get() == "paused" {
                    proa_macros::html_sync! {
                        <em>"Paused"</em>
                    }
                } else {
                    proa_macros::html_sync! {
                        <strong>"Idle"</strong>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_value_level_conditionals() {
    let html = render_to_string(ConditionalValues);
    assert!(
        html.contains(r#"class="active""#),
        "server render should evaluate the initial conditional attribute, got:\n{html}"
    );
    assert!(
        html.contains(">Active<"),
        "server render should evaluate the initial conditional text, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("conditionalvalues_"))
        .expect("ConditionalValues island should be registered")
        .js_source;
    assert!(
        js.contains("(__sig_0.get() ? \"Active\" : \"Idle\")"),
        "text conditional should lower to a JS ternary, got:\n{js}"
    );
    assert!(
        js.contains("(__sig_0.get() ? \"active\" : \"idle\")"),
        "attribute conditional should lower to a JS ternary, got:\n{js}"
    );
    assert!(
        js.contains("window.history?.replaceState?.(null, \"\", String((__sig_0.get() ? \"/active\" : \"/idle\")));"),
        "handler conditional should lower inside browser effects, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_template_else_if_chains() {
    let html = render_to_string(TemplateElseIfSections);
    assert!(
        html.contains(">Active<") && html.contains(">Paused<") && html.contains(">Idle<"),
        "server render should preserve all else-if branch nodes, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-cond-then=").count(),
        3,
        "template else-if lowering should emit one conditional section per branch, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("templateelseifsections_"))
        .expect("TemplateElseIfSections island should be registered")
        .js_source;
    assert!(
        js.matches("bind_conditional(").count() >= 3,
        "template else-if chain should bind each flattened branch, got:\n{js}"
    );
    assert!(
        js.contains("(__sig_0.get() === \"active\")")
            && js.contains("(__sig_0.get() === \"paused\")")
            && js.contains("=== false"),
        "template else-if predicates should include active, else-if, and negated prefix checks, got:\n{js}"
    );
}

pub struct NodeRefOpsForm;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NodeRefOpsForm {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let input = rsjs::node_ref::<rsjs::HtmlInputElement>();
        let form = rsjs::node_ref::<rsjs::HtmlFormElement>();
        let dialog = rsjs::node_ref::<rsjs::HtmlDialogElement>();
        let popover = rsjs::node_ref::<rsjs::HtmlElement>();
        let current = signal(String::new());
        proa_macros::html_sync! {
            <div>
                <input id="node-ref-input" ref={input} />
                <form ref={form}></form>
                <dialog ref={dialog}>"Dialog"</dialog>
                <div popover="" ref={popover}>"Popover"</div>
                <button on_click={|_| {
                    input.focus();
                    input.click();
                    form.request_submit();
                    form.reset();
                    dialog.show_modal();
                    dialog.close();
                    popover.show_popover();
                    popover.hide_popover();
                    popover.toggle_popover();
                    input.scroll_to(0_i64, 32_i64);
                    input.scroll_by(0_i64, 8_i64);
                    input.set_pointer_capture(7_i64);
                    input.release_pointer_capture(7_i64);
                    input.select_all();
                    input.set_value("Andrew");
                    input.set_custom_validity("");
                    input.set_selection_range(0_i64, 5_i64);
                    input.set_attribute("aria-expanded", "true");
                    input.remove_attribute("hidden");
                    input.set_style_property("outline-color", "red");
                    input.report_validity();
                    current.set(input.value());
                    console::log(
                        input.id(),
                        input.checked(),
                        input.check_validity(),
                        input.validation_message(),
                        input.validity_valid(),
                        input.will_validate(),
                        input.selection_start(),
                        input.selection_end(),
                        input.scroll_top(),
                        input.scroll_left(),
                        input.client_width(),
                        input.client_height(),
                        input.scroll_width(),
                        input.scroll_height(),
                        input.offset_width(),
                        input.offset_height(),
                        input.rect_x(),
                        input.rect_y(),
                        input.rect_top(),
                        input.rect_right(),
                        input.rect_bottom(),
                        input.rect_left(),
                        input.rect_width(),
                        input.rect_height(),
                        input.file_count(),
                        input.first_file_name(),
                        input.first_file_type(),
                        input.first_file_size(),
                        input.get_attribute("data-kind"),
                        input.has_attribute("hidden"),
                    );
                }}>
                    "sync"
                </button>
                <span>{current.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_node_ref_bindings_and_dom_methods() {
    let html = render_to_string(NodeRefOpsForm);
    assert!(
        html.contains(r#"data-rsjs-ref="1""#),
        "ref={{input}} should render a hydration marker on the input, got:\n{html}"
    );
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("noderefopsform_"))
        .expect("NodeRefOpsForm island should be registered")
        .js_source;
    assert!(
        js.contains("const __ref_0 = { current: null };")
            && js.contains("__ref_0.current = __node_1;"),
        "node_ref should declare and assign a JS ref handle, got:\n{js}"
    );
    assert!(
        js.contains("__ref_0.current?.focus?.();")
            && js.contains("__ref_0.current?.click?.();")
            && js.contains(".current?.requestSubmit?.();")
            && js.contains(".current?.reset?.();")
            && js.contains(".current?.showModal?.();")
            && js.contains(".current?.close?.();")
            && js.contains(".current?.showPopover?.();")
            && js.contains(".current?.hidePopover?.();")
            && js.contains(".current?.togglePopover?.();")
            && js.contains("__ref_0.current?.scrollTo?.(Number(0n), Number(32n));")
            && js.contains("__ref_0.current?.scrollBy?.(Number(0n), Number(8n));")
            && js.contains("__ref_0.current?.setPointerCapture?.(Number(7n));")
            && js.contains("__ref_0.current?.releasePointerCapture?.(Number(7n));")
            && js.contains("__ref_0.current?.select?.();")
            && js.contains("__ref_0.current.value = \"Andrew\";")
            && js.contains("__ref_0.current?.setCustomValidity?.(String(\"\"));")
            && js.contains("__ref_0.current?.setSelectionRange?.(Number(0n), Number(5n));")
            && js.contains("__ref_0.current?.setAttribute?.(\"aria-expanded\", String(\"true\"));")
            && js.contains("__ref_0.current?.removeAttribute?.(\"hidden\");")
            && js.contains(
                "__ref_0.current?.style?.setProperty(\"outline-color\", String(\"red\"));"
            ),
        "node_ref effect methods should lower to direct DOM operations, got:\n{js}"
    );
    assert!(
        js.contains("__ref_0.current?.reportValidity?.();")
            &&
        js.contains("__sig_0.set((__ref_0.current?.value ?? \"\"));")
            && js.contains(
                "console.log((__ref_0.current?.id ?? \"\"), (__ref_0.current?.checked ?? false), (__ref_0.current?.checkValidity?.() ?? true), (__ref_0.current?.validationMessage ?? \"\"), (__ref_0.current?.validity?.valid ?? true), (__ref_0.current?.willValidate ?? false), (__ref_0.current?.selectionStart ?? 0), (__ref_0.current?.selectionEnd ?? 0), (__ref_0.current?.scrollTop ?? 0), (__ref_0.current?.scrollLeft ?? 0), (__ref_0.current?.clientWidth ?? 0), (__ref_0.current?.clientHeight ?? 0), (__ref_0.current?.scrollWidth ?? 0), (__ref_0.current?.scrollHeight ?? 0), (__ref_0.current?.offsetWidth ?? 0), (__ref_0.current?.offsetHeight ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.x ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.y ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.top ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.right ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.bottom ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.left ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.width ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.height ?? 0), (__ref_0.current?.files?.length ?? 0), (__ref_0.current?.files?.[0]?.name ?? \"\"), (__ref_0.current?.files?.[0]?.type ?? \"\"), (__ref_0.current?.files?.[0]?.size ?? 0), (__ref_0.current?.getAttribute?.(\"data-kind\") ?? \"\"), (__ref_0.current?.hasAttribute?.(\"hidden\") ?? false));"
            ),
        "node_ref value accessors should lower to DOM property reads, got:\n{js}"
    );
    assert!(
        js.contains(
            "(__ref_0.current?.getBoundingClientRect?.()?.x ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.y ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.top ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.right ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.bottom ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.left ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.width ?? 0), (__ref_0.current?.getBoundingClientRect?.()?.height ?? 0)"
        ),
        "node_ref geometry accessors should lower to getBoundingClientRect fields, got:\n{js}"
    );
}

pub struct SearchableLabels;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for SearchableLabels {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <input
                    type="search"
                    value={query.get()}
                    on_input={|event| query.set(event.value())}
                />
                {for label in ["Alpha", "Beta"] {
                    proa_macros::html_sync! {
                        <button hidden={!label.to_lowercase().contains(query.get().to_lowercase())}>
                            {label}
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_string_search_expressions() {
    let html = render_to_string(SearchableLabels);
    assert!(
        html.contains(">Alpha</button>") && html.contains(">Beta</button>"),
        "initial empty query should render every repeated label, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("searchablelabels_"))
        .expect("SearchableLabels island should be registered")
        .js_source;
    assert!(
        js.contains(".toLowerCase()") && js.contains(".includes("),
        "string search expression should lower to JavaScript string operations, got:\n{js}"
    );
    assert!(
        js.contains("JSON.parse((__node_"),
        "loop-local label should still be read through a repeated node payload, got:\n{js}"
    );
}

pub struct StringMethodPanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for StringMethodPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal(String::from("  proa rocks  "));
        proa_macros::html_sync! {
            <div>
                <input
                    type="search"
                    value={query.get()}
                    on_input={|event| query.set(event.value().trim().to_uppercase())}
                />
                <span data-upper={query.get().to_uppercase()}>{query.get().trim()}</span>
                <p hidden={!query.get().to_uppercase().starts_with("  PROA")}>"prefix"</p>
                <p hidden={!query.get().ends_with("rocks  ")}>"suffix"</p>
                <p hidden={query.get().is_empty()}>"present"</p>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_extended_string_methods() {
    let html = render_to_string(StringMethodPanel);
    assert!(
        html.contains("data-upper=\"  PROA ROCKS  \""),
        "to_uppercase should produce the SSR initial attribute value, got:\n{html}"
    );
    assert!(
        html.contains(">proa rocks<"),
        "trim should produce the SSR initial text value, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("stringmethodpanel_"))
        .expect("StringMethodPanel island should be registered")
        .js_source;
    assert!(
        js.contains(".toUpperCase()"),
        "to_uppercase should lower to String.prototype.toUpperCase, got:\n{js}"
    );
    assert!(
        js.contains(r#".replace(/^[\u0009-\u000D\u0020\u0085\u00A0"#),
        "trim should lower to the explicit Rust char::is_whitespace matcher, got:\n{js}"
    );
    assert!(
        !js.contains(".trim()"),
        "trim must not use JavaScript's divergent native whitespace set, got:\n{js}"
    );
    assert!(
        js.contains(".startsWith("),
        "starts_with should lower to String.prototype.startsWith, got:\n{js}"
    );
    assert!(
        js.contains(".endsWith("),
        "ends_with should lower to String.prototype.endsWith, got:\n{js}"
    );
    assert!(
        js.contains("=== false"),
        "is_empty should lower as the negated non-emptiness check, got:\n{js}"
    );
}

pub struct DocumentEventPanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocumentEventPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let root = rsjs::node_ref::<rsjs::HtmlElement>();
        let open = signal(true);
        proa_macros::html_sync! {
            <div
                ref={root}
                data-open={open.get()}
                on_document_pointerdown={|event: rsjs::PointerEvent<rsjs::web::dom::Document>| {
                    if event.target_outside(root) {
                        open.set(false);
                    }
                }}
                on_document_keydown={|event: rsjs::KeyboardEvent<rsjs::web::dom::Document>| {
                    if event.key() == "Escape" {
                        open.set(false);
                    }
                }}
            >
                "Panel"
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_document_events_and_event_target_containment() {
    let html = render_to_string(DocumentEventPanel);
    assert!(
        html.contains("data-rsjs-ref") && html.contains("data-rsjs-handler"),
        "document event owner should render ref and handler markers, got:\n{html}"
    );
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("documenteventpanel_"))
        .expect("DocumentEventPanel island should be registered")
        .js_source;
    assert!(
        js.contains("bind_document_event(__node_")
            && js.contains(", \"keydown\", __handler_")
            && js.contains(", \"pointerdown\", __handler_"),
        "document event handlers should lower to bind_document_event, got:\n{js}"
    );
    assert!(
        js.contains("if (((event.key ?? \"\") === \"Escape\"))")
            && js.contains("if (!((__ref_0.current?.contains?.(event.target)) ?? false))"),
        "document event predicates should lower event key and target_outside(ref), got:\n{js}"
    );
}

pub struct HandlerIfCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerIfCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let enabled = signal(true);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    if enabled.get() {
                        count.set(1_i64);
                    } else {
                        count.set(2_i64);
                    }
                }}>
                    "toggle"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerBoolMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerBoolMatchCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let enabled = signal(true);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match enabled.get() {
                    true => count.set(1_i64),
                    false => count.set(2_i64),
                }}>
                    "toggle"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerStaticForLoopCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerStaticForLoopCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    for value in [1_i64, 2_i64] {
                        count.set(value);
                    }
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerLiteralMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerLiteralMatchCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match count.get() {
                    0_i64 => count.set(1_i64),
                    1_i64 => count.set(2_i64),
                    _ => count.set(3_i64),
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerGuardedLiteralMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerGuardedLiteralMatchCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match count.get() {
                    0_i64 if count.get() == 0_i64 => count.set(1_i64),
                    1_i64 => count.set(2_i64),
                    _ => count.set(3_i64),
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerRangeMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerRangeMatchCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match count.get() {
                    0_i64..=2_i64 => count.set(1_i64),
                    3_i64..5_i64 => count.set(2_i64),
                    _ => count.set(3_i64),
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerOptionMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerOptionMatchCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let maybe = signal::<Option<i64>>(None);
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match maybe.get() {
                    Some(_) => count.set(1_i64),
                    None => count.set(2_i64),
                    _ => count.set(3_i64),
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerOptionPayloadMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerOptionPayloadMatchCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let maybe = signal(Some(9_007_199_254_740_993_i64));
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match maybe.get() {
                    Some(value) => count.set(value),
                    None => count.set(2_i64),
                    _ => count.set(3_i64),
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerGuardedOptionPayloadMatchCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerGuardedOptionPayloadMatchCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let maybe = signal::<Option<i64>>(None);
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| match maybe.get() {
                    Some(value) if value > 0_i64 => count.set(value),
                    Some(_) => count.set(1_i64),
                    _ => count.set(2_i64),
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerStaticRangeForLoopCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerStaticRangeForLoopCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    for value in 1_i64..=2_i64 {
                        count.set(value);
                    }
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerDynamicRangeForLoopCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerDynamicRangeForLoopCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    for value in count.get()..3_i64 {
                        count.set(value);
                    }
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerLocalReassignmentCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerLocalReassignmentCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    let mut next = count.get();
                    next = next + 1_i64;
                    count.set(next);
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct HandlerLocalCompoundAssignmentCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerLocalCompoundAssignmentCounter
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    let mut next = count.get();
                    next += 4_i64;
                    next %= 3_i64;
                    count.set(next);
                }}>
                    "apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct InlineClosureReactiveCounter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for InlineClosureReactiveCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <span>{(|value: i64| value + 1_i64)(count.get())}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_lowers_if_else_inside_handler_bodies() {
    let _ = render_to_string(HandlerIfCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerifcounter_"))
        .expect("HandlerIfCounter island should be registered")
        .js_source;

    assert!(
        js.contains("if (__sig_1.get()) {"),
        "handler if condition should lower to structured JS, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(1n);") && js.contains("__sig_0.set(2n);"),
        "handler if branches should lower both signal writes, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_bool_match_inside_handler_bodies() {
    let _ = render_to_string(HandlerBoolMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerboolmatchcounter_"))
        .expect("HandlerBoolMatchCounter island should be registered")
        .js_source;

    assert!(
        js.contains("if (__sig_1.get()) {"),
        "handler bool match should lower through structured if JS, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(1n);") && js.contains("__sig_0.set(2n);"),
        "handler bool match arms should lower both signal writes, got:\n{js}"
    );
}

#[test]
fn rsjs_unrolls_static_array_for_loops_inside_handler_bodies() {
    let _ = render_to_string(HandlerStaticForLoopCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerstaticforloopcounter_"))
        .expect("HandlerStaticForLoopCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = 1n;") && js.contains("let __local_1 = 2n;"),
        "static array for loop should lower each item to a handler local, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(__local_0);") && js.contains("__sig_0.set(__local_1);"),
        "static array for loop body should be unrolled for each item, got:\n{js}"
    );
}

#[test]
fn rsjs_unrolls_static_range_for_loops_inside_handler_bodies() {
    let _ = render_to_string(HandlerStaticRangeForLoopCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handlerstaticrangeforloopcounter_")
        })
        .expect("HandlerStaticRangeForLoopCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = 1n;") && js.contains("let __local_1 = 2n;"),
        "static range for loop should lower each value to a handler local, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(__local_0);") && js.contains("__sig_0.set(__local_1);"),
        "static range for loop body should be unrolled for each value, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_dynamic_range_for_loops_inside_handler_bodies() {
    let _ = render_to_string(HandlerDynamicRangeForLoopCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handlerdynamicrangeforloopcounter_")
        })
        .expect("HandlerDynamicRangeForLoopCounter island should be registered")
        .js_source;

    assert!(
        js.contains("const __rsjs_range_start_raw_0 = __sig_0.get();")
            && js.contains(
                "const __rsjs_range_start_0 = typeof __rsjs_range_start_raw_0 === \"bigint\" ? __rsjs_range_start_raw_0 : Math.trunc(Number(__rsjs_range_start_raw_0));"
            ),
        "dynamic range for loop should evaluate and normalize the start bound once, got:\n{js}"
    );
    assert!(
        js.contains("const __rsjs_range_end_raw_0 = 3n;")
            && js.contains(
                "const __rsjs_range_end_0 = typeof __rsjs_range_end_raw_0 === \"bigint\" ? __rsjs_range_end_raw_0 : Math.trunc(Number(__rsjs_range_end_raw_0));"
            ),
        "dynamic range for loop should evaluate and normalize the end bound once, got:\n{js}"
    );
    assert!(
        js.contains(
            "for (let __local_0 = __rsjs_range_start_0; __local_0 < __rsjs_range_end_0; __local_0 += typeof __local_0 === \"bigint\" ? 1n : 1) {"
        ),
        "dynamic range for loop should lower to structured JS, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(__local_0);"),
        "dynamic range for loop body should read the loop local, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_handler_local_reassignments() {
    let _ = render_to_string(HandlerLocalReassignmentCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handlerlocalreassignmentcounter_")
        })
        .expect("HandlerLocalReassignmentCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler local reassignment should declare the mutable local, got:\n{js}"
    );
    assert!(
        js.contains("__local_0 = checked_int_add(__local_0, 1n, \"i64\");"),
        "handler local reassignment should lower through checked i64 addition, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(__local_0);"),
        "handler local reassignment should keep later reads pointed at the local, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_handler_local_compound_assignments() {
    let _ = render_to_string(HandlerLocalCompoundAssignmentCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handlerlocalcompoundassignmentcounter_")
        })
        .expect("HandlerLocalCompoundAssignmentCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler local compound assignment should declare the mutable local, got:\n{js}"
    );
    assert!(
        js.contains("__local_0 = checked_int_add(__local_0, 4n, \"i64\");"),
        "handler local compound assignment should lower additive assignment through checked i64 addition, got:\n{js}"
    );
    assert!(
        js.contains("__local_0 = checked_int_rem(__local_0, 3n, \"i64\");"),
        "handler local compound assignment should lower remainder assignment through checked i64 remainder, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(__local_0);"),
        "handler local compound assignment should keep later reads pointed at the local, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_literal_match_inside_handler_bodies() {
    let _ = render_to_string(HandlerLiteralMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerliteralmatchcounter_"))
        .expect("HandlerLiteralMatchCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler literal match should evaluate the scrutinee once, got:\n{js}"
    );
    assert!(
        js.contains("if ((__local_0 === 0n)) {") && js.contains("if ((__local_0 === 1n)) {"),
        "handler literal match should lower literal arms to equality branches, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(1n);")
            && js.contains("__sig_0.set(2n);")
            && js.contains("__sig_0.set(3n);"),
        "handler literal match should lower every arm body, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_guarded_literal_match_inside_handler_bodies() {
    let _ = render_to_string(HandlerGuardedLiteralMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handlerguardedliteralmatchcounter_")
        })
        .expect("HandlerGuardedLiteralMatchCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler guarded literal match should evaluate the scrutinee once, got:\n{js}"
    );
    assert!(
        js.contains("if (((__local_0 === 0n) && (__sig_0.get() === 0n))) {"),
        "handler guarded literal match should combine literal test and guard, got:\n{js}"
    );
    assert!(
        js.contains("if ((__local_0 === 1n)) {"),
        "handler guarded literal match should keep subsequent literal arms, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(1n);")
            && js.contains("__sig_0.set(2n);")
            && js.contains("__sig_0.set(3n);"),
        "handler guarded literal match should lower every arm body, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_range_match_inside_handler_bodies() {
    let _ = render_to_string(HandlerRangeMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handlerrangematchcounter_"))
        .expect("HandlerRangeMatchCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler range match should evaluate the scrutinee once, got:\n{js}"
    );
    assert!(
        js.contains("if (((__local_0 >= 0n) && (__local_0 <= 2n))) {"),
        "inclusive range match should lower to >= and <= guards, got:\n{js}"
    );
    assert!(
        js.contains("if (((__local_0 >= 3n) && (__local_0 < 5n))) {"),
        "half-open range match should lower to >= and < guards, got:\n{js}"
    );
    assert!(
        js.contains("__sig_0.set(1n);")
            && js.contains("__sig_0.set(2n);")
            && js.contains("__sig_0.set(3n);"),
        "handler range match should lower every arm body, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_option_match_inside_handler_bodies() {
    let _ = render_to_string(HandlerOptionMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("handleroptionmatchcounter_"))
        .expect("HandlerOptionMatchCounter island should be registered")
        .js_source;

    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler option match should evaluate the scrutinee once, got:
{js}"
    );
    assert!(
        js.contains("if (rsjs.__is_present(__local_0)) {"),
        "Some(_) match arm should lower to an is-present check, got:
{js}"
    );
    assert!(
        js.contains("if ((rsjs.__is_present(__local_0) === false)) {"),
        "None match arm should lower to an inverted is-present check, got:
{js}"
    );
    assert!(
        js.contains("__sig_1.set(1n);")
            && js.contains("__sig_1.set(2n);")
            && js.contains("__sig_1.set(3n);"),
        "handler option match should lower every arm body, got:
{js}"
    );
}

#[test]
fn rsjs_lowers_option_payload_match_inside_handler_bodies() {
    let html = render_to_string(HandlerOptionPayloadMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handleroptionpayloadmatchcounter_")
        })
        .expect("HandlerOptionPayloadMatchCounter island should be registered")
        .js_source;

    assert!(
        html.contains("\"9007199254740993\""),
        "Option<i64>::Some must serialize its wide payload as canonical decimal text, got:\n{html}"
    );
    assert!(
        js.contains(
            "signal(signal_initial_typed(__rsjs_signal_initials, 0, null, {\"kind\":\"option\",\"inner\":{\"kind\":\"i64\"}}))"
        ),
        "Option<i64> must recursively decode a wide Some payload before matching, got:\n{js}"
    );
    assert!(
        js.contains("let __local_0 = __sig_0.get();"),
        "handler option payload match should evaluate the scrutinee once, got:
{js}"
    );
    assert!(
        js.contains("if (rsjs.__is_present(__local_0)) {")
            && js.contains("let __local_1 = __local_0;"),
        "Some(value) match arm should bind the option payload local, got:
{js}"
    );
    assert!(
        js.contains("__sig_1.set(__local_1);")
            && js.contains("__sig_1.set(2n);")
            && js.contains("__sig_1.set(3n);"),
        "handler option payload match should lower every arm body, got:
{js}"
    );
}

#[test]
fn rsjs_lowers_guarded_option_payload_match_inside_handler_bodies() {
    let _ = render_to_string(HandlerGuardedOptionPayloadMatchCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("handlerguardedoptionpayloadmatchcounter_")
        })
        .expect("HandlerGuardedOptionPayloadMatchCounter island should be registered")
        .js_source;

    assert!(
        js.contains("if ((rsjs.__is_present(__local_0) && (__local_0 > 0n))) {"),
        "Some(value) guard should read the scrutinee local before the arm payload binding, got:\n{js}"
    );
    assert!(
        js.contains("let __local_1 = __local_0;") && js.contains("__sig_1.set(__local_1);"),
        "guarded Some(value) arm should bind the option payload for the arm body, got:\n{js}"
    );
    assert!(
        js.contains("__sig_1.set(1n);") && js.contains("__sig_1.set(2n);"),
        "guarded option payload match should lower fallback arms, got:\n{js}"
    );
}

#[test]
fn rsjs_inlines_reactive_inline_closure_calls() {
    let _ = render_to_string(InlineClosureReactiveCounter);
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("inlineclosurereactivecounter_"))
        .expect("InlineClosureReactiveCounter island should be registered")
        .js_source;

    assert!(
        js.contains("checked_int_add(__sig_0.get(), 1n, \"i64\")"),
        "reactive inline closure call should substitute the argument into the body, got:\n{js}"
    );
}

pub struct ComponentCallbackInput {
    pub on_change: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentCallbackInput {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>
                <input on_input={|event| {
                    let value = event.value();
                    console::log(value);
                    self.on_change.call(value);
                }} />
            </div>
        }
        .render(cx)
    }
}

pub struct ComponentDateCallback {
    pub on_select: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentDateCallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button
                data_date="sample-day"
                on_click={|event| {
                self.on_select.call(event.target_date());
            }}
            >
                "Pick date"
            </button>
        }
        .render(cx)
    }
}

pub struct ParentWithDateCallback;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithDateCallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(String::new());
        let on_select = rsjs::handler(|date: String| selected.set(date));

        proa_macros::html_sync! {
            <section>
                {ComponentDateCallback { on_select }}
                <span>{selected.get()}</span>
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithComponentCallback;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithComponentCallback {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let on_change = rsjs::handler(|next: String| value.set(next));

        proa_macros::html_sync! {
            <section>
                <p>{value.get()}</p>
                {ComponentCallbackInput { on_change }}
            </section>
        }
        .render(cx)
    }
}

pub struct SecondParentWithComponentCallback;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for SecondParentWithComponentCallback
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let on_change = rsjs::handler(|next: String| value.set(next));

        proa_macros::html_sync! {
            <article>
                {ComponentCallbackInput { on_change }}
                <output>{value.get()}</output>
            </article>
        }
        .render(cx)
    }
}

pub struct ComponentOptionalCallbackInput {
    pub on_change: Option<rsjs::Handler<String>>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentOptionalCallbackInput {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <input on_input={|event| {
                self.on_change.call_if_some(event.value());
            }} />
        }
        .render(cx)
    }
}

pub struct ParentWithOptionalCallbackSome;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithOptionalCallbackSome {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let on_change = rsjs::handler(|next: String| value.set(next));

        proa_macros::html_sync! {
            <section>
                {ComponentOptionalCallbackInput {
                    on_change: Some(on_change),
                }}
                <p>{value.get()}</p>
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithOptionalCallbackNone;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithOptionalCallbackNone {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>
                {ComponentOptionalCallbackInput {
                    on_change: None,
                }}
            </section>
        }
        .render(cx)
    }
}

pub struct ComponentCallbackValue {
    pub value: String,
    pub on_change: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentCallbackValue {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let current = signal(self.value);

        proa_macros::html_sync! {
            <div>
                <span>{current.get()}</span>
                <input
                    value={current.get()}
                    on_input={|event| {
                    self.on_change.call(event.value());
                }}
                />
            </div>
        }
        .render(cx)
    }
}

pub struct ParentWithComponentProp;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithComponentProp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let on_change = rsjs::handler(|next: String| value.set(next));

        proa_macros::html_sync! {
            <section>
                {ComponentCallbackValue {
                    value: value.get(),
                    on_change,
                }}
            </section>
        }
        .render(cx)
    }
}

pub struct ComponentOptionalSignalProp {
    pub selected: Option<u32>,
    pub on_select: rsjs::Handler<u32>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentOptionalSignalProp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal::<u32>(self.selected.unwrap_or(0_u32));

        proa_macros::html_sync! {
            <div>
                {for day in 7_u32..=7_u32 {
                    proa_macros::html_sync! {
                        <button data_active={selected.get() == (day as u32)} on_click={|_| self.on_select.call(day as u32)}>
                            {day}
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct ParentWithOptionalSignalProp;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithOptionalSignalProp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(7_u32);
        let on_select = rsjs::handler(|next: u32| selected.set(next));

        proa_macros::html_sync! {
            <section>
                {ComponentOptionalSignalProp {
                    selected: Some(selected.get()),
                    on_select,
                }}
            </section>
        }
        .render(cx)
    }
}

pub struct ComponentServerLabel {
    pub label: &'static str,
    pub class: &'static str,
    pub on_press: rsjs::Handler<()>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentServerLabel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button
                type="button"
                class={rsjs::server_attr(self.class)}
                on_click={|_| self.on_press.call(())}
            >
                {rsjs::server_only(self.label)}
            </button>
        }
        .render(cx)
    }
}

pub struct NaturalServerOnlyPanel {
    pub label: &'static str,
    pub class: &'static str,
}

#[rsjs(component)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NaturalServerOnlyPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section class={self.class}>
                <h2>{self.label}</h2>
            </section>
        }
        .render(cx)
    }
}

pub struct ComponentInferredServerLabel {
    pub label: &'static str,
    pub class: &'static str,
    pub on_press: rsjs::Handler<()>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentInferredServerLabel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button type="button" class={self.class} on_click={|_| self.on_press.call(())}>
                {self.label}
            </button>
        }
        .render(cx)
    }
}

#[derive(Clone, Copy, rsjs::ClientProps)]
pub struct ServerUser {
    pub label: &'static str,
    pub class: &'static str,
}

pub struct ComponentInferredServerUser {
    pub user: ServerUser,
    pub on_press: rsjs::Handler<()>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentInferredServerUser {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button type="button" class={self.user.class} on_click={|_| self.on_press.call(())}>
                {self.user.label}
            </button>
        }
        .render(cx)
    }
}

pub struct ParentWithInferredServerProp;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithInferredServerProp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let on_press = rsjs::handler(|_| count.set(1));
        let label = "Press naturally";
        let class = "natural-action";

        proa_macros::html_sync! {
            <section>
                {ComponentInferredServerLabel {
                    label,
                    class,
                    on_press,
                }}
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithInferredServerPathProp;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ParentWithInferredServerPathProp
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let on_press = rsjs::handler(|_| count.set(1));
        let user = ServerUser {
            label: "Press through path",
            class: "path-action",
        };

        proa_macros::html_sync! {
            <section>
                {ComponentInferredServerUser {
                    user,
                    on_press,
                }}
            </section>
        }
        .render(cx)
    }
}

pub struct ComponentServerPreludeLocal {
    pub label: &'static str,
    pub class: &'static str,
    pub on_press: rsjs::Handler<()>,
}

struct ComponentServerPreludeClassNames {
    root: &'static str,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ComponentServerPreludeLocal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let class_names = ComponentServerPreludeClassNames {
            root: "base-action",
        };

        proa_macros::html_sync! {
            <button
                type="button"
                class={rsjs::server_attr(proa_macros::text!("{} {}", class_names.root, self.class))}
                on_click={|_| self.on_press.call(())}
            >
                {rsjs::server_only(self.label)}
            </button>
        }
        .render(cx)
    }
}

pub struct ServerAttrStyle;

#[rsjs(component)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerAttrStyle {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let swatch_style = "background:#fff;";

        proa_macros::html_sync! {
            <span data_swatch style={rsjs::server_attr(proa_core::style_ok(swatch_style))}></span>
        }
        .render(cx)
    }
}

pub struct ParentWithServerProp;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithServerProp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let pressed = signal(0_i64);
        let on_press = rsjs::handler(|_| pressed.set(1_i64));

        proa_macros::html_sync! {
            <section>
                {ComponentServerLabel {
                    label: "Press me",
                    class: "primary-action",
                    on_press,
                }}
                <span>{pressed.get()}</span>
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithServerPreludeLocalProp;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ParentWithServerPreludeLocalProp
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let pressed = signal(0_i64);
        let on_press = rsjs::handler(|_| pressed.set(1_i64));

        proa_macros::html_sync! {
            <section>
                {ComponentServerPreludeLocal {
                    label: "Press local",
                    class: "custom-action",
                    on_press,
                }}
                <span>{pressed.get()}</span>
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithExplicitChildIsland;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParentWithExplicitChildIsland {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let pressed = signal(0_i64);

        proa_macros::html_sync! {
            <section>
                {rsjs::island(ComponentServerLabel {
                    label: "Nested child island",
                    class: "child-boundary",
                    on_press: rsjs::noop_handler(),
                })}
                <span>{pressed.get()}</span>
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedEventButtons;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedEventButtons {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let labels = ["A", "B"];

        proa_macros::html_sync! {
            <div>
                {for label in labels {
                    proa_macros::html_sync! {
                        <button type="button" on_click={|_| count.update(|c| c + 1)}>
                            {rsjs::server_only(label)}
                        </button>
                    }
                }}
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct RepeatedPayloadButtons {
    pub on_change: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedPayloadButtons {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let labels = ["A", "B"];

        proa_macros::html_sync! {
            <div>
                {for label in labels {
                    proa_macros::html_sync! {
                        <button type="button" on_click={|_| {
                            console::log(label.to_string());
                            self.on_change.call(label.to_string());
                        }}>
                            {rsjs::server_only(label)}
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct RepeatedSubmitForms;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedSubmitForms {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(String::new());
        let labels = ["draft", "final"];

        proa_macros::html_sync! {
            <section>
                {for label in labels {
                    proa_macros::html_sync! {
                        <form on_submit={|event| {
                            event.prevent_default();
                            selected.set(label.to_string());
                        }}>
                            <button type="submit">{rsjs::server_only(label)}</button>
                        </form>
                    }
                }}
                <output>{selected.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedGlobalEventRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedGlobalEventRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let labels = ["A", "B"];

        proa_macros::html_sync! {
            <ul>
                {for label in labels {
                    proa_macros::html_sync! {
                        <li
                            on_document_keydown={|event: rsjs::KeyboardEvent<rsjs::web::dom::Document>| {
                                console::log(label.to_string(), event.key());
                            }}
                            on_window_resize={|_: rsjs::Event<rsjs::web::dom::Window>| {
                                console::log(label.to_string());
                            }}
                        >
                            {rsjs::server_only(label)}
                        </li>
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}

pub struct ParentWithRepeatedPayloadComponent;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ParentWithRepeatedPayloadComponent
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(String::new());
        let on_change = rsjs::handler(|next: String| selected.set(next));

        proa_macros::html_sync! {
            <section>
                {RepeatedPayloadButtons { on_change }}
                <output>{selected.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithTwoRepeatedPayloadComponents;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ParentWithTwoRepeatedPayloadComponents
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let first = signal(String::new());
        let second = signal(String::new());
        let on_first = rsjs::handler(|next: String| first.set(next));
        let on_second = rsjs::handler(|next: String| second.set(next));

        proa_macros::html_sync! {
            <section>
                {RepeatedPayloadButtons { on_change: on_first }}
                {RepeatedPayloadButtons { on_change: on_second }}
                <output>{first.get()}</output>
                <output>{second.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct ParentWithLoopedRepeatedPayloadComponents;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ParentWithLoopedRepeatedPayloadComponents
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(String::new());
        let on_change = rsjs::handler(|next: String| selected.set(next));

        proa_macros::html_sync! {
            <section>
                {for _index in 0..2 {
                    proa_macros::html_sync! {
                        {RepeatedPayloadButtons { on_change }}
                    }
                }}
                <output>{selected.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedDynamicRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedDynamicRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal("A".to_string());
        let labels = ["A", "B"];

        proa_macros::html_sync! {
            <div>
                {for label in labels {
                    proa_macros::html_sync! {
                        <button
                            type="button"
                            data_selected={selected.get() == label.to_string()}
                            on_click={|_| selected.set(label.to_string())}
                        >
                            <span>{label}</span>
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

#[derive(Clone, Copy, rsjs::ClientProps)]
pub struct RepeatedProjectedRow {
    pub key: u32,
    pub alternate_key: u32,
    pub label: &'static str,
    pub class: &'static str,
}

impl RepeatedProjectedRow {
    fn memo_key(&self) -> u32 {
        self.key
    }
}

pub struct RepeatedProjectedRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedProjectedRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(1_u32);
        let rows = [
            RepeatedProjectedRow {
                key: 1,
                alternate_key: 11,
                label: "Alpha",
                class: "row-alpha",
            },
            RepeatedProjectedRow {
                key: 2,
                alternate_key: 22,
                label: "Beta",
                class: "row-beta",
            },
        ];

        proa_macros::html_sync! {
            <div>
                {for row in rows {
                    proa_macros::html_sync! {
                        <button
                            type="button"
                            class={row.class}
                            data_selected={selected.get() == (row.key as u32)}
                            on_click={|_| selected.set(row.key as u32)}
                        >
                            {row.label}
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct RepeatedDerivedServerRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedDerivedServerRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(0_u32);
        let rows = [
            RepeatedProjectedRow {
                key: 1,
                alternate_key: 11,
                label: "Alpha",
                class: "row-alpha",
            },
            RepeatedProjectedRow {
                key: 2,
                alternate_key: 22,
                label: "Beta",
                class: "row-beta",
            },
        ];

        proa_macros::html_sync! {
            <section>
                {for row in rows {
                    let upper = row.label.to_uppercase();
                    proa_macros::html_sync! {
                        <button
                            type="button"
                            data_upper={row.label.to_uppercase()}
                            on_click={|_| selected.set(row.key as u32)}
                        >
                            <span>{upper}</span>
                        </button>
                    }
                }}
                <output>{selected.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedShadowedSignalRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedShadowedSignalRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let row = signal("client".to_string());
        let rows = ["Server A", "Server B"];

        proa_macros::html_sync! {
            <section>
                {for row in rows {
                    proa_macros::html_sync! {
                        <button type="button" on_click={|_| console::log(row.to_string())}>
                            {row}
                        </button>
                    }
                }}
                <output>{row.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedPayloadCseScopeRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedPayloadCseScopeRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(0_u32);
        let rows = [
            RepeatedProjectedRow {
                key: 1,
                alternate_key: 11,
                label: "Alpha",
                class: "row-alpha",
            },
            RepeatedProjectedRow {
                key: 2,
                alternate_key: 22,
                label: "Beta",
                class: "row-beta",
            },
        ];

        proa_macros::html_sync! {
            <section>
                {for row in rows {
                    proa_macros::html_sync! {
                        <div>
                            <button type="button" on_click={|_| {
                                selected.set(row.key as u32);
                                console::log(row.key as u32);
                                console::log(row.alternate_key as u32);
                                console::log(row.memo_key() as u32);
                                console::log(row.memo_key() as u32);
                            }}>
                                "Primary"
                            </button>
                            <button
                                type="button"
                                on_click={|_| selected.set(row.key as u32)}
                            >
                                "Secondary"
                            </button>
                        </div>
                    }
                }}
                <output>{selected.get()}</output>
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedInputRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedInputRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal("seed".to_string());
        let enabled = signal(true);

        proa_macros::html_sync! {
            <div>
                {for _idx in 0..2 {
                    proa_macros::html_sync! {
                        <label>
                            <input bind_value={value}/>
                            <input type="checkbox" bind_checked={enabled}/>
                        </label>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct RepeatedConditionalRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedConditionalRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal("A".to_string());
        let labels = ["A", "B"];

        proa_macros::html_sync! {
            <div>
                {for label in labels {
                    proa_macros::html_sync! {
                        <section>
                            {if selected.get() == label.to_string() {
                                proa_macros::html_sync! {
                                    <span>{rsjs::server_only(label)}</span>
                                }
                            } else {
                                proa_macros::html_sync! {
                                    <em>"Other"</em>
                                }
                            }}
                        </section>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct SuffixedSignalRows;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for SuffixedSignalRows {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(2_u32);

        proa_macros::html_sync! {
            <div>
                {for day in 1_u32..=2_u32 {
                    proa_macros::html_sync! {
                        <button
                            type="button"
                            data_selected={selected.get() == (day as u32)}
                            on_click={|_| selected.set(day as u32)}
                        >
                            {day}
                        </button>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

#[test]
fn rsjs_allows_event_only_template_loops() {
    let html = render_to_string(RepeatedEventButtons);
    assert!(
        html.contains(">A</button>") && html.contains(">B</button>"),
        "server loop should render both labels, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-handler=").count(),
        2,
        "loop-rendered buttons should carry repeated delegated handler markers, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedeventbuttons_"))
        .expect("RepeatedEventButtons island should be registered")
        .js_source;
    assert!(
        js.contains("bind_delegated_events(root, \"click\""),
        "loop event handlers should use delegated dispatch, got:\n{js}"
    );
    assert!(
        js.contains("root.querySelector('[data-rsjs-bind="),
        "islands with template loops should fall back to marker lookup for node bindings, got:\n{js}"
    );
}

#[test]
fn rsjs_allows_repeated_submit_handlers() {
    let html = render_to_string(RepeatedSubmitForms);
    assert!(
        html.contains(">draft</button>") && html.contains(">final</button>"),
        "server loop should render both submit forms, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-handler=").count(),
        2,
        "loop-rendered forms should carry repeated submit handler markers, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        2,
        "each repeated submit form should carry one loop-local payload attr, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedsubmitforms_"))
        .expect("RepeatedSubmitForms island should be registered")
        .js_source;
    assert!(
        js.contains("for (const __node_")
            && js.contains("of root.querySelectorAll('[data-rsjs-handler=")
            && js.contains("bind_submit(__node_"),
        "repeated submit handlers should bind every marker-matched form, got:\n{js}"
    );
    assert!(
        js.contains("event.preventDefault();")
            && js.contains("__sig_0.set(JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.target?.closest?.('[data-rsjs-payload-"),
        "submit handler body should preserve event ops and loop-local payloads, got:\n{js}"
    );
}

#[test]
fn rsjs_allows_repeated_global_event_handlers() {
    let html = render_to_string(RepeatedGlobalEventRows);
    assert!(
        html.contains(">A</li>") && html.contains(">B</li>"),
        "server loop should render both global event owners, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-handler=").count(),
        2,
        "loop-rendered global event owners should carry repeated handler markers, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        4,
        "method-call payloads may be effectful and must remain separately evaluated, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedglobaleventrows_"))
        .expect("RepeatedGlobalEventRows island should be registered")
        .js_source;
    assert!(
        js.contains("for (const __node_")
            && js.contains("of root.querySelectorAll('[data-rsjs-handler=")
            && js.contains("bind_document_event(__node_")
            && js.contains("bind_window_event(__node_"),
        "repeated global event handlers should bind every marker-matched owner, got:\n{js}"
    );
    assert!(
        js.contains("event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.target?.closest?.('[data-rsjs-payload-"),
        "loop-local global handlers should prefer owner payloads with compatibility and delegated fallbacks, got:\n{js}"
    );
}

#[test]
fn rsjs_serializes_loop_local_handler_payloads() {
    let html = render_to_string(RepeatedPayloadButtons {
        on_change: rsjs::noop_handler(),
    });
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        4,
        "each repeated method-call payload should retain its own evaluation, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedpayloadbuttons_"))
        .expect("RepeatedPayloadButtons island should be registered")
        .js_source;
    assert!(
        js.contains("JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.target?.closest?.('[data-rsjs-payload-"),
        "loop-local payloads should be read from event payload attrs, got:\n{js}"
    );
    assert!(
        js.contains("console.log(JSON.parse")
            && js.contains("root.__rsjs_callbacks.__callback_0(JSON.parse"),
        "console::log and self.on_change should both receive decoded payloads, got:\n{js}"
    );
}

#[test]
fn descriptor_compilation_preserves_child_loop_payloads_without_fragment_mounts() {
    let html = render_to_string(ParentWithRepeatedPayloadComponent);
    assert!(
        !html.contains("data-rsjs-component=")
            && !html.contains("data-rsjs-component-mount=")
            && html.contains(r#"data-rsjs-island-root="repeatedpayloadbuttons_"#),
        "the direct child should render as a provisional island without legacy component-mount \
         markers, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        4,
        "the direct child should preserve both method-call payload evaluations per repeated button, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedpayloadbuttons_"))
        .expect("RepeatedPayloadButtons should retain its provisional island artifact");
    let parent = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("parentwithrepeatedpayloadcomponent_")
        })
        .expect("ParentWithRepeatedPayloadComponent island should be registered");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("__callback_0")
            && !parent.js_source.contains("console.log(JSON.parse")
            && !parent
                .js_source
                .contains("bind_delegated_events(root, \"click\""),
        "the provisional parent graph must not copy the child's callback or loop-payload logic, \
         got:\n{}",
        parent.js_source
    );
    assert!(
        child
            .js_source
            .contains("JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-")
            && child
                .js_source
                .contains("event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-")
            && child
                .js_source
                .contains("root.__rsjs_callbacks?.__callback_0")
            && child
                .js_source
                .contains("bind_delegated_events(root, \"click\""),
        "the provisional child artifact should retain loop-payload decoding and its callback \
         slot, got:\n{}",
        child.js_source
    );
}

#[test]
fn descriptor_compilation_repeats_direct_children_without_mount_selectors() {
    let html = render_to_string(ParentWithTwoRepeatedPayloadComponents);
    assert_eq!(
        html.matches(r#"data-rsjs-island-root="repeatedpayloadbuttons_"#)
            .count(),
        2,
        "ordinary Rust rendering should produce two provisional child islands, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-component-mount=").count(),
        0,
        "descriptor compilation must not emit legacy fragment selectors, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("parentwithtworepeatedpayloadcomponents_")
        })
        .expect("ParentWithTwoRepeatedPayloadComponents island should be registered");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("data-rsjs-component-mount=")
            && !parent
                .js_source
                .contains("root.querySelectorAll('[data-rsjs-component-mount="),
        "the parent must preserve two linker edges without generating mount-selector JS, got:\n{}",
        parent.js_source
    );
}

#[test]
fn descriptor_compilation_preserves_repeated_lifecycle_without_mount_runtime() {
    let html = render_to_string(ParentWithLoopedRepeatedPayloadComponents);
    assert_eq!(
        html.matches(r#"data-rsjs-island-root="repeatedpayloadbuttons_"#)
            .count(),
        2,
        "the authored loop should render two provisional child owner instances, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-component-mount=").count(),
        0,
        "repeated direct component edges must not use legacy fragment markers, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("parentwithloopedrepeatedpayloadcomponents_")
        })
        .expect("looped parent island should be registered");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent
                .js_source
                .contains("root.querySelectorAll('[data-rsjs-component-mount=")
            && !parent
                .js_source
                .contains("for (const __rsjs_component_root_"),
        "the repeated edge lifecycle belongs to the linker; descriptor-stage parent JS must not \
         emit a fragment mount loop, got:\n{}",
        parent.js_source
    );
}

#[test]
fn rsjs_binds_repeated_text_and_attrs_with_loop_payloads() {
    let html = render_to_string(RepeatedDynamicRows);
    assert!(
        html.contains(r#"data-selected="true""#) && html.contains(r#"data-selected="false""#),
        "server-rendered repeated dynamic attrs should compute per row, got:\n{html}"
    );
    assert!(
        html.contains(">A</span>") && html.contains(">B</span>"),
        "server-rendered repeated text bindings should compute per row, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        4,
        "static loop text should remain SSR while each method-call payload keeps its own evaluation, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeateddynamicrows_"))
        .expect("RepeatedDynamicRows island should be registered")
        .js_source;
    assert!(
        js.contains("for (const __node_")
            && js.contains("root.querySelectorAll('[data-rsjs-attr=")
            && !js.contains("root.querySelectorAll('[data-rsjs-bind="),
        "only the signal-dependent repeated attribute should require a client lookup, got:\n{js}"
    );
    assert!(
        !js.contains("bind_text(__node_") && js.contains("bind_attr(__node_"),
        "signal-independent loop text must stay server-only, got:\n{js}"
    );
    assert!(
        js.contains("JSON.parse((__node_"),
        "repeated text/attr expressions should decode node-local payloads, got:\n{js}"
    );
}

#[test]
fn rsjs_interns_projected_loop_payloads_without_whole_object_scaffolding() {
    let html = render_to_string(RepeatedProjectedRows);
    assert!(
        html.contains(r#"class="row-alpha""#)
            && html.contains(r#"class="row-beta""#)
            && html.contains(">Alpha</button>")
            && html.contains(">Beta</button>"),
        "signal-independent projected text and attrs should remain SSR, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        2,
        "each row should encode its projected key once for both the binding and handler, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedprojectedrows_"))
        .expect("RepeatedProjectedRows island should be registered");
    assert!(
        !entry.js_source.contains("bind_text(")
            && entry.js_source.contains("bind_attr(__node_")
            && entry.js_source.contains("\"data-selected\"")
            && !entry.js_source.contains(", \"class\","),
        "only the signal-dependent data-selected attr should enter client bindings, got:\n{}",
        entry.js_source
    );

    let path = component_descriptor_path(entry.asset_id);
    let descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read descriptor {}: {error}", path.display())
        }))
        .expect("component descriptor should be valid JSON");
    let payload_tokens = descriptor["view_tree"]["nodes"]
        .as_array()
        .expect("view-tree nodes should be an array")
        .iter()
        .filter_map(|entry| entry["node"]["payload_attrs"].as_array())
        .flat_map(|attrs| attrs.iter())
        .map(|attr| {
            attr["tokens"]
                .as_str()
                .expect("payload tokens should be strings")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        payload_tokens,
        vec!["row.key as u32"],
        "the projected key should be interned per node without an orphan whole-row payload"
    );
}

#[test]
fn rsjs_keeps_signal_independent_derived_loop_values_on_the_server() {
    let html = render_to_string(RepeatedDerivedServerRows);
    assert!(
        html.contains(r#"data-upper="ALPHA""#)
            && html.contains(r#"data-upper="BETA""#)
            && html.contains("<span>ALPHA</span>")
            && html.contains("<span>BETA</span>"),
        "derived loop values and server-only prefix locals should render as SSR, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        2,
        "only each row's handler key should need a browser payload, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedderivedserverrows_"))
        .expect("RepeatedDerivedServerRows island should be registered");
    assert!(
        !entry.js_source.contains("\"data-upper\"")
            && entry.js_source.matches("bind_text(").count() == 1,
        "derived repeated attrs/text must stay out of Client IR; only the selected output binds, got:\n{}",
        entry.js_source
    );
}

#[test]
fn rsjs_loop_local_shadowing_signal_resolves_lexically() {
    let html = render_to_string(RepeatedShadowedSignalRows);
    assert!(
        html.contains(">Server A</button>") && html.contains(">Server B</button>"),
        "the repeated item must shadow the same-named outer signal in SSR text, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        2,
        "the handler should encode each shadowing loop item once, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedshadowedsignalrows_"))
        .expect("RepeatedShadowedSignalRows island should be registered");
    assert_eq!(
        entry.js_source.matches("bind_text(").count(),
        1,
        "only the outer signal output should bind; repeated `row` text is SSR, got:\n{}",
        entry.js_source
    );
}

#[test]
fn rsjs_payload_cse_keeps_distinct_tokens_on_one_node() {
    let html = render_to_string(RepeatedPayloadCseScopeRows);
    assert_eq!(
        html.matches("data-rsjs-payload-").count(),
        10,
        "per row: immutable `row.key` uses share on the first node, a distinct projection stays distinct, two method calls stay separately evaluated, and the second node owns its own key payload; got:\n{html}"
    );

    let payload_groups = payload_token_groups("repeatedpayloadcsescoperows_");

    assert!(
        payload_groups.iter().any(|attrs| {
            attrs.len() == 4
                && attrs.iter().filter(|tokens| *tokens == "row.key as u32").count() == 1
                && attrs
                    .iter()
                    .filter(|tokens| *tokens == "row.alternate_key as u32")
                    .count()
                    == 1
                && attrs
                    .iter()
                    .filter(|tokens| tokens.contains("memo_key"))
                    .count()
                    == 2
        }),
        "one node should intern the repeated immutable projection, retain the distinct projection, and not CSE method calls; got {payload_groups:?}"
    );
}

#[test]
fn rsjs_payload_cse_does_not_cross_nodes() {
    let _ = render_to_string(RepeatedPayloadCseScopeRows);
    let payload_groups = payload_token_groups("repeatedpayloadcsescoperows_");

    assert!(
        payload_groups
            .iter()
            .any(|attrs| attrs.as_slice() == ["row.key as u32"]),
        "the same key expression on a sibling node must own a separate payload attr; got {payload_groups:?}"
    );
}

#[test]
fn rsjs_binds_repeated_input_controls() {
    let html = render_to_string(RepeatedInputRows);
    assert_eq!(
        html.matches("data-rsjs-input-value=").count(),
        2,
        "loop-rendered value inputs should carry repeated input markers, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-input-checked=").count(),
        2,
        "loop-rendered checked inputs should carry repeated input markers, got:\n{html}"
    );
    assert_eq!(
        html.matches(r#"value="seed""#).count(),
        2,
        "loop-rendered value inputs should carry the signal initial value, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedinputrows_"))
        .expect("RepeatedInputRows island should be registered")
        .js_source;
    assert!(
        js.contains("for (const __node_")
            && js.contains("root.querySelectorAll('[data-rsjs-input-value=")
            && js.contains("root.querySelectorAll('[data-rsjs-input-checked="),
        "repeated input bindings should resolve every marker-matched control, got:\n{js}"
    );
    assert!(
        js.contains("bind_input(__node_")
            && js.contains(", \"value\", __sig_0)")
            && js.contains(", \"checked\", __sig_1)"),
        "repeated input controls should bind their signal/property pairs, got:\n{js}"
    );
    assert!(
        !js.contains("walk_path(root,"),
        "repeated-input-only islands should not import or call walk_path, got:\n{js}"
    );
}

#[test]
fn rsjs_binds_repeated_conditional_sections() {
    let html = render_to_string(RepeatedConditionalRows);
    assert_eq!(
        html.matches("data-rsjs-cond-then=").count(),
        2,
        "loop-rendered then branches should carry repeated conditional markers, got:\n{html}"
    );
    assert_eq!(
        html.matches("data-rsjs-cond-else=").count(),
        2,
        "loop-rendered else branches should carry repeated conditional markers, got:\n{html}"
    );
    assert!(
        html.matches("data-rsjs-payload-").count() >= 2,
        "loop-local conditional predicates should encode parent payload attrs, got:\n{html}"
    );
    assert_eq!(
        html.matches("display:none").count(),
        2,
        "SSR should hide the inactive branch for each repeated row, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("repeatedconditionalrows_"))
        .expect("RepeatedConditionalRows island should be registered")
        .js_source;
    assert!(
        js.contains("Array.from(root.querySelectorAll('[data-rsjs-cond-then=")
            && js.contains("Array.from(root.querySelectorAll('[data-rsjs-cond-else=")
            && js.contains("bind_conditional("),
        "repeated conditional branches should resolve every marker-matched row, got:\n{js}"
    );
    assert!(
        js.contains("JSON.parse((__cond_ctx_")
            && js.contains(".getAttribute('data-rsjs-payload-"),
        "loop-local conditional predicates should decode row payloads from the parent context, got:\n{js}"
    );
    assert!(
        !js.contains("walk_path(root,"),
        "repeated-conditional-only islands should not import or call walk_path, got:\n{js}"
    );
}

#[test]
fn rsjs_preserves_suffixed_numeric_signal_initializers() {
    let html = render_to_string(SuffixedSignalRows);
    assert!(
        html.contains(r#"data-selected="true""#),
        "server-rendered attr comparison should preserve the u32 signal initializer, got:\n{html}"
    );
    assert!(
        html.contains(">2</button>"),
        "server-rendered repeated text should preserve u32 loop payloads, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("suffixedsignalrows_"))
        .expect("SuffixedSignalRows island should be registered")
        .js_source;
    assert!(
        js.contains(
            "__sig_0.set(JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-"
        ) && js.contains("event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-")
            && js.contains("event.target?.closest?.('[data-rsjs-payload-"),
        "u32 loop payload should still lower through node-local payload decoding, got:\n{js}"
    );
}

#[test]
fn rsjs_lowers_self_callback_slot_invocations() {
    let _ = render_to_string(ComponentCallbackInput {
        on_change: rsjs::noop_handler(),
    });
    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentcallbackinput_"))
        .expect("ComponentCallbackInput island should be registered")
        .js_source;
    assert!(
        js.contains("let __local_0 = (event.target?.value ?? \"\");")
            && js.contains("console.log(__local_0);"),
        "handler-local alias should lower to a JS local used by console::log, got:\n{js}"
    );
    assert!(
        js.contains("root.__rsjs_callbacks?.__callback_0")
            && js.contains("root.__rsjs_callbacks.__callback_0(__local_0);"),
        "self.on_change.call(value) should lower to a callback-slot invocation, got:\n{js}"
    );
}

#[test]
fn rsjs_writes_component_descriptor_with_ir_sections() {
    let _ = render_to_string(ComponentCallbackInput {
        on_change: rsjs::noop_handler(),
    });
    let manifest = rsjs::manifest::manifest();
    let asset_id = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentcallbackinput_"))
        .expect("ComponentCallbackInput island should be registered")
        .asset_id;
    let path = component_descriptor_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read descriptor {}: {err}", path.display()));
    let descriptor: serde_json::Value =
        serde_json::from_str(&json).expect("component descriptor should be valid JSON");

    assert_eq!(descriptor["schema_version"], 9);
    assert_eq!(descriptor["format"], "rsjs.component_descriptor");
    assert_eq!(descriptor["client_ir_version"], 3);
    assert_descriptor_hash_shape(&descriptor["js_hash"]);
    assert_descriptor_hash_shape(&descriptor["descriptor_hash"]);
    assert_eq!(descriptor["callback_slots"][0]["name"], "on_change");
    assert_eq!(descriptor["fields"][0]["kind"], "handler");
    assert!(descriptor["handlers"][0].get("statements").is_none());
    assert_eq!(
        descriptor["handlers"][0]["body_ir"]["stmts"][0]["kind"],
        "let"
    );
    assert_eq!(
        descriptor["handlers"][0]["body_ir"]["stmts"][1]["kind"],
        "console"
    );
    assert_eq!(
        descriptor["handlers"][0]["body_ir"]["stmts"][2]["kind"],
        "callback_invoke"
    );
    assert_eq!(descriptor["bindings"]["events"][0]["event"], "input");
    assert_eq!(
        descriptor["view_tree"]["nodes"][0]["node"]["kind"],
        "element"
    );
    assert_eq!(descriptor["counts"]["callback_slots"], 1);

    let actual = normalize_descriptor_snapshot(descriptor);
    if update_descriptor_snapshot(
        &actual,
        "rsjs_conformance/snapshots/descriptor/component_callbacks_basic.descriptor.json",
    ) {
        return;
    }
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/descriptor/component_callbacks_basic.descriptor.json"
    ))
    .expect("descriptor snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

#[test]
fn rsjs_writes_component_descriptor_prop_fields() {
    let manifest = rsjs::manifest::manifest();
    let asset_id = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentcallbackvalue_"))
        .expect("ComponentCallbackValue island should be registered")
        .asset_id;
    let path = component_descriptor_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read descriptor {}: {err}", path.display()));
    let descriptor: serde_json::Value =
        serde_json::from_str(&json).expect("component descriptor should be valid JSON");

    let fields = descriptor["fields"]
        .as_array()
        .expect("fields should be an array");
    assert!(
        fields.iter().any(|field| field["kind"] == "prop"
            && field["name"] == "value"
            && field["value_id"] == 0
            && field["source"] == "signal_initializer"),
        "descriptor should record `signal(self.value)` as a symbolic prop value, got:\n{json}"
    );
    assert!(
        descriptor["inbound_inputs"]["prop_sites"]
            .as_array()
            .is_some_and(|sites| sites.iter().any(|site| {
                site["field_id"] == 0
                    && site["value_id"] == 0
                    && site["temporal"]
                        == serde_json::json!({
                            "kind": "seed_once",
                            "site_id": 0,
                        })
                    && site["target"]
                        == serde_json::json!({
                            "kind": "signal_seed",
                            "signal_id": 0,
                        })
            })),
        "descriptor should preserve the SeedOnce target for `signal(self.value)`, got:\n{json}"
    );
}

#[test]
fn rsjs_writes_component_descriptor_server_prop_fields() {
    let manifest = rsjs::manifest::manifest();
    let asset_id = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentserverlabel_"))
        .expect("ComponentServerLabel island should be registered")
        .asset_id;
    let path = component_descriptor_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read descriptor {}: {err}", path.display()));
    let descriptor: serde_json::Value =
        serde_json::from_str(&json).expect("component descriptor should be valid JSON");

    let fields = descriptor["fields"]
        .as_array()
        .expect("fields should be an array");
    assert!(
        fields.iter().any(|field| field["kind"] == "server_prop"
            && field["name"] == "label"
            && field["source"] == "server_only_usage"),
        "descriptor should record `server_only(self.label)` as a server prop, got:\n{json}"
    );
    assert!(
        fields.iter().any(|field| field["kind"] == "server_prop"
            && field["name"] == "class"
            && field["source"] == "server_only_usage"),
        "descriptor should record `server_attr(self.class)` as a server prop, got:\n{json}"
    );
}

#[test]
fn descriptor_stage_records_simple_self_fields_as_live_participation() {
    let html = render_to_string(NaturalServerOnlyPanel {
        label: "Server headline",
        class: "server-panel",
    });

    assert!(
        html.contains("Server headline") && html.contains("class=\"server-panel\""),
        "simple self fields should render as server HTML, got:\n{html}"
    );
    assert!(
        html.contains("<rsjs-island") && html.contains("data-rsjs-island"),
        "the descriptor stage retains provisional live-input scaffolding for final linking, \
         got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .any(|entry| entry.asset_id.starts_with("naturalserveronlypanel_")),
        "the provisional descriptor artifact should retain live self-field participation"
    );
}

#[test]
fn rsjs_writes_simple_self_fields_as_live_read_descriptor_roles() {
    let manifest = rsjs::manifest::manifest();
    let asset_id = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentinferredserverlabel_"))
        .expect("ComponentInferredServerLabel island should be registered")
        .asset_id;
    let path = component_descriptor_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read descriptor {}: {err}", path.display()));
    let descriptor: serde_json::Value =
        serde_json::from_str(&json).expect("component descriptor should be valid JSON");

    let fields = descriptor["fields"]
        .as_array()
        .expect("fields should be an array");
    assert!(
        fields.iter().any(|field| field["kind"] == "prop"
            && field["name"] == "label"
            && field["source"] == "live_read"),
        "descriptor should record natural `{{self.label}}` as live participation, got:\n{json}"
    );
    assert!(
        fields.iter().any(|field| field["kind"] == "prop"
            && field["name"] == "class"
            && field["source"] == "live_read"),
        "descriptor should record natural `class={{self.class}}` as live participation, got:\n{json}"
    );
    assert!(
        descriptor["inbound_inputs"]["prop_sites"]
            .as_array()
            .is_some_and(|sites| sites.iter().all(|site| {
                site["temporal"]["kind"] == "live_read" && site["target"]["kind"] == "live_signal"
            })),
        "every direct text/attribute use should retain normative LiveRead timing, got:\n{json}"
    );
}

#[test]
fn rsjs_writes_path_projection_descriptor_live_read_fields() {
    let manifest = rsjs::manifest::manifest();
    let asset_id = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentinferredserveruser_"))
        .expect("ComponentInferredServerUser island should be registered")
        .asset_id;
    let path = component_descriptor_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read descriptor {}: {err}", path.display()));
    let descriptor: serde_json::Value =
        serde_json::from_str(&json).expect("component descriptor should be valid JSON");

    let fields = descriptor["fields"]
        .as_array()
        .expect("fields should be an array");
    assert!(
        fields.iter().any(|field| field["kind"] == "prop"
            && field["name"] == "user"
            && field["source"] == "live_read"),
        "descriptor should record natural `{{self.user.label}}` / `class={{self.user.class}}` \
         as one live inbound value, got:\n{json}"
    );
    assert!(
        descriptor["exprs"].as_array().is_some_and(|exprs| exprs
            .iter()
            .any(|expr| expr["expr"]["path"] == "/label")
            && exprs.iter().any(|expr| expr["expr"]["path"] == "/class")),
        "the client graph should preserve both narrow projections from the one live value, \
         got:\n{json}"
    );
}

#[test]
fn rsjs_allows_server_rendered_style_attrs() {
    let html = render_to_string(ServerAttrStyle);
    assert!(
        html.contains(r#"style="background:#fff;""#),
        "server_attr style should be emitted by the server render body, got:\n{html}"
    );
    assert!(
        !html.contains("data-rsjs-attr"),
        "server_attr style should not produce a client-side attr binding, got:\n{html}"
    );
    assert!(
        !html.contains("<rsjs-island") && !html.contains("data-rsjs-island"),
        "fully server-rendered rsjs output should not emit island scaffolding, got:\n{html}"
    );
}

#[test]
fn descriptor_compilation_preserves_callback_child_as_a_direct_edge() {
    let html = render_to_string(ParentWithComponentCallback);
    assert!(
        html.contains("<input")
            && html.contains(r#"data-rsjs-island-root="componentcallbackinput_"#)
            && !html.contains("data-rsjs-component-mount="),
        "the direct child should render through ordinary Rust with its provisional island, \
         got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("parentwithcomponentcallback_"))
        .expect("the parent's own text binding should retain a provisional island");
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentcallbackinput_"))
        .expect("the callback child should retain its provisional island artifact");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("__rsjs_callbacks")
            && !parent.js_source.contains("console.log(__local_0);"),
        "descriptor compilation must not copy the child graph or callback adapter into the \
         parent, got:\n{}",
        parent.js_source
    );
    assert!(
        child.js_source.contains("console.log(__local_0);")
            && child
                .js_source
                .contains("root.__rsjs_callbacks?.__callback_0"),
        "the provisional child artifact should retain its own handler and an inert callback-slot \
         lookup for descriptor-stage SSR, got:\n{}",
        child.js_source
    );
}

#[test]
fn descriptor_compilation_does_not_import_shared_component_fragments() {
    let _ = render_to_string(ParentWithComponentCallback);
    let _ = render_to_string(SecondParentWithComponentCallback);

    let manifest = rsjs::manifest::manifest();
    let provisional_parents: Vec<_> = manifest
        .islands
        .iter()
        .filter(|entry| {
            entry.asset_id.starts_with("parentwithcomponentcallback_")
                || entry
                    .asset_id
                    .starts_with("secondparentwithcomponentcallback_")
        })
        .collect();
    assert_eq!(
        provisional_parents.len(),
        2,
        "both parents have their own text binding and should retain provisional islands"
    );
    for parent in provisional_parents {
        assert!(
            parent.component_fragment_imports.is_empty()
                && !parent
                    .js_source
                    .contains("component_componentcallbackinput_")
                && !parent.js_source.contains("__rsjs_callbacks"),
            "parent {} must contain only its own provisional graph, got:\n{}",
            parent.asset_id,
            parent.js_source
        );
    }

    let matching_child_islands = manifest
        .islands
        .iter()
        .filter(|entry| entry.asset_id.starts_with("componentcallbackinput_"))
        .count();
    assert_eq!(
        matching_child_islands, 1,
        "the shared child definition should retain one provisional island artifact"
    );
}

#[test]
fn descriptor_compilation_keeps_date_callback_child_graph_separate() {
    let html = render_to_string(ParentWithDateCallback);
    assert!(
        html.contains(r#"data-date="sample-day""#)
            && html.contains(r#"data-rsjs-island-root="componentdatecallback_"#),
        "the date callback child should render its SSR snapshot and provisional island, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("parentwithdatecallback_"))
        .expect("the parent's own selected-value binding should retain a provisional island");
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentdatecallback_"))
        .expect("ComponentDateCallback should retain its provisional island artifact");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("__callback_0")
            && !parent.js_source.contains("__sig_0.set(__payload)"),
        "descriptor compilation must not copy the child callback adapter into the parent, got:\n{}",
        parent.js_source
    );
    assert!(
        child
            .js_source
            .contains("root.__rsjs_callbacks?.__callback_0"),
        "the provisional child artifact should retain its callback slot for the final linker, \
         got:\n{}",
        child.js_source
    );
}

#[test]
fn descriptor_compilation_accepts_optional_callback_some_without_copying() {
    let html = render_to_string(ParentWithOptionalCallbackSome);
    assert!(
        html.contains("<input")
            && html.contains(r#"data-rsjs-island-root="componentoptionalcallbackinput_"#)
            && !html.contains("data-rsjs-component-mount="),
        "the optional callback child should render provisionally through the parent, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("parentwithoptionalcallbacksome_")
        })
        .expect("the parent's own text binding should retain a provisional island");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("__callback_0")
            && !parent.js_source.contains("__rsjs_callbacks"),
        "Some(handler) must remain a direct linker edge instead of becoming provisional \
         callback plumbing, got:\n{}",
        parent.js_source
    );
}

#[test]
fn descriptor_compilation_accepts_optional_callback_none_as_noop() {
    let html = render_to_string(ParentWithOptionalCallbackNone);
    assert!(
        html.contains("<input")
            && html.contains(r#"data-rsjs-island-root="componentoptionalcallbackinput_"#),
        "the optional callback child should render when the direct edge carries None, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.islands.iter().all(|entry| !entry
            .asset_id
            .starts_with("parentwithoptionalcallbacknone_")),
        "a parent whose only client-looking surface is a direct None callback edge should remain \
         server-only during descriptor compilation"
    );
}

#[test]
fn descriptor_compilation_snapshots_live_signal_prop_without_copying_child_graph() {
    let html = render_to_string(ParentWithComponentProp);
    assert!(
        html.contains("<span")
            && html.contains(r#"data-rsjs-island-root="componentcallbackvalue_"#)
            && html.contains(r#"{"signals":[""]}"#),
        "the direct child should receive the parent's initial SSR value and render its provisional \
         signal snapshot, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("parentwithcomponentprop_")),
        "the parent must not become a provisional island merely by copying the live child edge"
    );
    let child = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("componentcallbackvalue_"))
        .expect("ComponentCallbackValue should retain its provisional island artifact");
    assert!(
        child.js_source.contains("const __sig_0")
            && child
                .js_source
                .contains("root.__rsjs_callbacks?.__callback_0"),
        "the child should retain its own signal and callback slot until the linker fuses the \
         direct edge, got:\n{}",
        child.js_source
    );
}

#[test]
fn descriptor_compilation_snapshots_optional_signal_prop_without_copying() {
    let html = render_to_string(ParentWithOptionalSignalProp);
    assert!(
        html.contains("data-active=\"true\"")
            && html.contains(r#"data-rsjs-island-root="componentoptionalsignalprop_"#)
            && !html.contains("data-rsjs-component-mount="),
        "the optional live prop should supply the child's initial SSR snapshot, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("parentwithoptionalsignalprop_")),
        "the parent must not copy the optional child signal/callback graph"
    );
}

#[test]
fn descriptor_compilation_renders_server_props_without_copying_callback_graph() {
    let html = render_to_string(ParentWithServerProp);
    assert!(
        html.contains("Press me"),
        "server-only child prop should render with the parent value, got:\n{html}"
    );
    assert!(
        html.contains("class=\"primary-action\""),
        "server-only child attr prop should render with the parent value, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("parentwithserverprop_"))
        .expect("the parent's own pressed-value binding should retain a provisional island");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("__sig_0.set(1n);")
            && !parent.js_source.contains("__rsjs_callbacks"),
        "descriptor compilation must preserve the direct callback edge instead of copying its \
         adapter into the parent, got:\n{}",
        parent.js_source
    );
    assert!(
        html.contains(r#"data-rsjs-island-root="componentserverlabel_"#),
        "the child should retain a provisional island while its server props render normally, \
         got:\n{html}"
    );
}

#[test]
fn descriptor_compilation_renders_inferred_server_props_without_parent_graph() {
    let html = render_to_string(ParentWithInferredServerProp);
    assert!(
        html.contains("Press naturally"),
        "inferred server child prop should render with the parent value, got:\n{html}"
    );
    assert!(
        html.contains("class=\"natural-action\""),
        "inferred server child attr prop should render with the parent value, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest
            .islands
            .iter()
            .all(|entry| !entry.asset_id.starts_with("parentwithinferredserverprop_")),
        "a parent with only a direct child edge must remain server-only during descriptor \
         compilation"
    );
    assert!(
        html.contains(r#"data-rsjs-island-root="componentinferredserverlabel_"#),
        "the child should keep its provisional artifact while inferred props stay in SSR, \
         got:\n{html}"
    );
}

#[test]
fn descriptor_compilation_renders_inferred_path_props_without_parent_graph() {
    let html = render_to_string(ParentWithInferredServerPathProp);
    assert!(
        html.contains("Press through path"),
        "inferred server path child prop should render with the parent value, got:\n{html}"
    );
    assert!(
        html.contains("class=\"path-action\""),
        "inferred server path attr prop should render with the parent value, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.islands.iter().all(|entry| !entry
            .asset_id
            .starts_with("parentwithinferredserverpathprop_")),
        "a path-valued direct child edge must not create a copied parent graph"
    );
    assert!(
        html.contains(r#"data-rsjs-island-root="componentinferredserveruser_"#),
        "the child should keep its provisional artifact while path props stay in SSR, got:\n{html}"
    );
}

#[test]
fn descriptor_compilation_preserves_child_server_prelude_without_copying() {
    let html = render_to_string(ParentWithServerPreludeLocalProp);
    assert!(
        html.contains("Press local"),
        "server-only child prelude local should render with the parent value, got:\n{html}"
    );
    assert!(
        html.contains("base-action custom-action"),
        "server_attr child prelude local should render after descriptor inlining, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent = manifest
        .islands
        .iter()
        .find(|entry| {
            entry
                .asset_id
                .starts_with("parentwithserverpreludelocalprop_")
        })
        .expect("the parent's own pressed-value binding should retain a provisional island");

    assert!(
        parent.component_fragment_imports.is_empty()
            && !parent.js_source.contains("__sig_0.set(1n);")
            && !parent.js_source.contains("__rsjs_callbacks"),
        "the child prelude and callback graph must not be copied into provisional parent JS, \
         got:\n{}",
        parent.js_source
    );
    assert!(
        html.contains(r#"data-rsjs-island-root="componentserverpreludelocal_"#),
        "the child should render its server prelude in its provisional island, got:\n{html}"
    );
}

#[test]
fn rsjs_explicit_island_boundary_does_not_inline_child() {
    let html = render_to_string(ParentWithExplicitChildIsland);
    assert!(
        html.contains("Nested child island"),
        "explicit child island should still render server HTML, got:\n{html}"
    );
    assert!(
        html.matches("<rsjs-island").count() >= 2,
        "explicit child island should render a nested island boundary, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("parentwithexplicitchildisland_"))
        .expect("ParentWithExplicitChildIsland island should be registered")
        .js_source;

    assert!(
        !js.contains("__rsjs_callbacks"),
        "explicit child island must not inline child callback plumbing into parent JS, got:\n{js}"
    );
}

fn component_descriptor_path(asset_id: &str) -> std::path::PathBuf {
    let filename = format!("{asset_id}.json");
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .expect("rsjs_macros should live under the workspace root");
    let mut candidates = Vec::with_capacity(4);
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.push(
            std::path::PathBuf::from(target_dir)
                .join("rsjs")
                .join("components")
                .join(&filename),
        );
    }
    candidates.extend([
        workspace_dir
            .join("target")
            .join("rsjs")
            .join("components")
            .join(&filename),
        manifest_dir
            .join("target")
            .join("rsjs")
            .join("components")
            .join(&filename),
        std::path::PathBuf::from("target")
            .join("rsjs")
            .join("components")
            .join(&filename),
    ]);
    candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "could not find rsjs component descriptor for {asset_id}; tried: {}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn payload_token_groups(asset_prefix: &str) -> Vec<Vec<String>> {
    let manifest = rsjs::manifest::manifest();
    let entry = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with(asset_prefix))
        .unwrap_or_else(|| panic!("{asset_prefix} island should be registered"));
    let path = component_descriptor_path(entry.asset_id);
    let descriptor: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read descriptor {}: {error}", path.display())
        }))
        .expect("component descriptor should be valid JSON");
    descriptor["view_tree"]["nodes"]
        .as_array()
        .expect("view-tree nodes should be an array")
        .iter()
        .filter_map(|entry| entry["node"]["payload_attrs"].as_array())
        .map(|attrs| {
            attrs
                .iter()
                .map(|attr| {
                    attr["tokens"]
                        .as_str()
                        .expect("payload tokens should be strings")
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .filter(|attrs| !attrs.is_empty())
        .collect()
}

fn assert_descriptor_hash_shape(value: &serde_json::Value) {
    let hash = value.as_str().expect("hash field should be a string");
    let suffix = hash
        .strip_prefix("sha256:")
        .expect("hash should use a sha256: prefix");
    assert_eq!(suffix.len(), 64);
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
}

fn update_descriptor_snapshot(actual: &serde_json::Value, relative_path: &str) -> bool {
    if std::env::var_os("RSJS_UPDATE_SNAPSHOTS").is_none() {
        return false;
    }
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir
        .parent()
        .expect("rsjs_macros should live under the workspace root")
        .join(relative_path);
    let text = serde_json::to_string_pretty(actual)
        .unwrap_or_else(|err| panic!("failed to format descriptor snapshot JSON: {err}"));
    std::fs::write(&path, format!("{text}\n")).unwrap_or_else(|err| {
        panic!(
            "failed to update descriptor snapshot {}: {err}",
            path.display()
        )
    });
    true
}

fn normalize_descriptor_snapshot(mut descriptor: serde_json::Value) -> serde_json::Value {
    descriptor["asset_id"] = serde_json::json!("<asset_id>");
    descriptor["js_hash"] = serde_json::json!("sha256:<hash>");
    descriptor["descriptor_hash"] = serde_json::json!("sha256:<hash>");
    normalize_descriptor_source_anchors(&mut descriptor);
    descriptor
}

fn normalize_descriptor_source_anchors(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if map.contains_key("source_anchor") {
                map.insert(
                    "source_anchor".to_string(),
                    serde_json::json!({
                        "start_line": 0,
                        "start_column": 0
                    }),
                );
            }
            for child in map.values_mut() {
                normalize_descriptor_source_anchors(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                normalize_descriptor_source_anchors(child);
            }
        }
        _ => {}
    }
}
