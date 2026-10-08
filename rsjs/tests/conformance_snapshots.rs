// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::cmp_owned)]
// Comparison is emitted by the attribute macro under test.

//! Descriptor-stage conformance snapshots for authored rsjs components.
//!
//! Reusable component fragments are covered here. Cross-component adapters,
//! owner fusion, explicit island cuts, and final manifest entries are linker
//! outputs covered by the executable pipeline tests under `proa_cli/tests/`;
//! this suite must not recreate or bless those decisions from macro sidecars.

#[allow(unused_imports)]
use rsjs::{console, QueryKey};

pub struct BasicHandlerSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for BasicHandlerSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let text = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String signal state

        ::proa_macros::html_sync! {
            <input
                value={text.get()}
                on_input={|event| {
                    let next = event.value();
                    text.set(next);
                }}
            />
        }
        .render(cx)
    }
}

pub struct BranchingHandlerSnapshot;

pub struct NamedEventHandlerSnapshot;

pub struct NodeRefsSnapshot;

pub struct QueryMutationSnapshot;

pub struct SyncQuerySnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for SyncQuerySnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let price: rsjs::Query<i64> = rsjs::query_with_initial(
            QueryKey::new(("stock", "AAPL")),
            172_i64,
            "/api/stocks/AAPL",
            rsjs::QueryOptions::default(),
        );

        ::proa_macros::html_sync! {
            <section>
                <span>{price.data()}</span>
                <button
                    type="button"
                    on_click={|_| {
                        console::log("refresh");
                        price.refetch();
                    }}
                >
                    "Refresh"
                </button>
            </section>
        }
        .render(cx)
    }
}

pub struct ServerDecoratedButtonSnapshot {
    pub label: &'static str,
    pub class: &'static str,
    pub on_press: rsjs::Handler<()>,
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for ServerDecoratedButtonSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        ::proa_macros::html_sync! {
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

pub struct BrowserEffectsSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for BrowserEffectsSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let hash = rsjs::signal("filters=open".to_string()); // proa-perf-ok: conformance fixture exercises String signal state
        let credential_options = rsjs::signal(r#"{"challenge":"passkey"}"#.to_string()); // proa-perf-ok: conformance fixture exercises String signal state
        let credential_json = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String signal state

        ::proa_macros::html_sync! {
            <div data-url={rsjs::location::href()}>
                <span>{rsjs::location::search()}</span>
                <button on_click={|_| {
                    hash.set(rsjs::local_storage::get_item_or(String::new(), String::new())); // proa-perf-ok: conformance fixture exercises String browser fallback state
                    console::log(rsjs::session_storage::get_item(String::new())); // proa-perf-ok: conformance fixture exercises String browser fallback state
                    console::log(
                        rsjs::local_storage::get_bool_or("enabled", false),
                        rsjs::local_storage::get_i64_or("count", 0_i64),
                        rsjs::session_storage::get_f64_or("ratio", 0.0_f64),
                    );
                    rsjs::window::scroll_to(0_i32, 100_i32);
                    rsjs::window::scroll_by(0_i32, 20_i32);
                    rsjs::window::print();
                    rsjs::location::set_hash(hash.get());
                    rsjs::location::assign("/catalog/filters");
                    rsjs::location::replace(hash.get());
                    rsjs::location::reload();
                    rsjs::history::push_state("/catalog#filters");
                    rsjs::history::replace_state(rsjs::location::pathname());
                    rsjs::history::back();
                    rsjs::history::forward();
                    rsjs::history::go(1_i32);
                    rsjs::document::dispatch_event("filters:changed");
                    rsjs::document::dispatch_event_with_detail("filters:changed", hash.get());
                    rsjs::document::set_title(hash.get());
                    rsjs::clipboard::write_text(hash.get());
                    rsjs::webauthn::create_json_into(credential_options.get(), &credential_json);
                    rsjs::webauthn::get_json_into_or(credential_options.get(), &credential_json, "unavailable");
                    rsjs::local_storage::set_item("filters", hash.get());
                    rsjs::local_storage::remove_item("filters:stale");
                    rsjs::session_storage::set_item("filters", hash.get());
                    rsjs::session_storage::remove_item("filters:stale");
                    rsjs::url::set_search_param("filters", hash.get());
                    rsjs::url::delete_search_param("stale");
                    rsjs::url::push_search_param("page", "2");
                    rsjs::url::push_delete_search_param("modal");
                    console::log(rsjs::encoding::base64url_encode(rsjs::encoding::utf8_bytes(hash.get())));
                }}>
                    "Apply"
                </button>
            </div>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRender<L, B>
    for QueryMutationSnapshot
{
    fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> ::proa_core::RenderOutcome<
        impl ::core::future::Future<Output = ::core::result::Result<(), ::proa_core::WriteError>>,
    > {
        ::proa_core::RenderOutcome::pending(async move {
            let draft = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String mutation payload state
            let price: rsjs::Query<QueryMutationStockPrice> = rsjs::query_with_initial(
                QueryKey::new(("stock", "AAPL")),
                QUERY_MUTATION_INITIAL_PRICE,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    method: Some(rsjs::HttpMethod::Post),
                    credentials: Some(rsjs::FetchCredentials::Include),
                    response: Some(rsjs::ResponseKind::Json),
                    headers: &[("Accept", "application/json"), ("X-RSJS", "1")],
                    body: Some(rsjs::RequestBody::Json(r#"{"symbol":"AAPL"}"#)),
                    timeout_ms: Some(1_500),
                },
                rsjs::QueryOptions {
                    interval_ms: None,
                    refetch_on_focus: true,
                    download_progress: true,
                    stream_text: false,
                    retry_count: 2,
                    stale_time_ms: 2_000,
                },
            );
            let feed: rsjs::Query<&'static str> = rsjs::query_with_initial(
                QueryKey::new(("feed",)),
                "ready",
                rsjs::FetchRequest {
                    url: "/api/feed",
                    method: None,
                    credentials: None,
                    response: Some(rsjs::ResponseKind::Text),
                    headers: &[("Accept", "text/plain")],
                    body: None,
                    timeout_ms: None,
                },
                rsjs::QueryOptions {
                    interval_ms: None,
                    refetch_on_focus: false,
                    download_progress: true,
                    stream_text: true,
                    retry_count: 0,
                    stale_time_ms: 0,
                },
            );
            let save = rsjs::mutation_with_options::<String, ()>(
                QueryKey::new(("update_cart", "AAPL")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Patch),
                    credentials: Some(rsjs::FetchCredentials::Omit),
                    response: Some(rsjs::ResponseKind::Empty),
                    headers: &[("X-Mutate", "1")],
                    body: None,
                    timeout_ms: Some(2_500),
                },
                (QueryKey::new(("stock", "AAPL")), QueryKey::new(("feed",))),
                rsjs::MutationOptions {
                    retry_count: 1,
                    on_success_invalidate_strategy: rsjs::InvalidationStrategy::Refetch,
                },
            );

            ::proa_macros::html! {
                <section>
                    <input value={draft.get()} on_input={|event| draft.set(event.value())} />
                    <span>{price.data().value}</span>
                    <span>{price.status()}</span>
                    <span>{price.download_loaded()}</span>
                    <span>{price.download_total()}</span>
                    <span>{price.download_percent()}</span>
                    <span>{price.fetch_count()}</span>
                    <span>{feed.data().as_str()}</span>
                    <span>{save.status()}</span>
                    <span hidden={!save.is_pending()}></span>
                    <button type="button" on_click={|_| save.run(draft.get())}></button>
                    <button type="button" on_click={|_| price.refetch()}></button>
                    <button type="button" on_click={|_| price.invalidate()}></button>
                </section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NodeRefsSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let input = rsjs::node_ref::<rsjs::HtmlInputElement>();
        let form = rsjs::node_ref::<rsjs::HtmlFormElement>();
        let dialog = rsjs::node_ref::<rsjs::HtmlDialogElement>();
        let popover = rsjs::node_ref::<rsjs::HtmlElement>();
        let current = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String node-ref state

        ::proa_macros::html_sync! {
            <div>
                <input ref={input} />
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
                    input.scroll_to(0_i32, 32_i32);
                    input.scroll_by(0_i32, 8_i32);
                    input.select_all();
                    input.set_value("");
                    input.set_selection_range(0_u32, 0_u32);
                    input.set_attribute("aria-expanded", "false");
                    input.remove_attribute("hidden");
                    input.set_style_property("outline-color", "transparent");
                    console::log(input.get_attribute("data-kind"), input.has_attribute("hidden"));
                    current.set(input.value());
                }}>
                    "Clear"
                </button>
                <span>{current.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct CallbackTextInputSnapshot {
    pub value: String,
    pub on_change: rsjs::Handler<String>,
}

pub struct LiveInboundFieldsSnapshot {
    pub label: String,
    pub class: String,
    pub enabled: bool,
    pub on_activate: rsjs::Handler<String>,
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for LiveInboundFieldsSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let activate = rsjs::event_handler(|_: rsjs::Event| {
            self.on_activate.call(self.label);
        });

        ::proa_macros::html_sync! {
            <section class={self.class}>
                <p>{self.label}</p>
                {if self.enabled {
                    ::proa_macros::html_sync! {
                        <button type="button" on_click={activate()}>
                            "Activate"
                        </button>
                    }
                }}
            </section>
        }
        .render(cx)
    }
}

pub struct RepeatedButtonsSnapshot {
    pub on_select: rsjs::Handler<String>,
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for RepeatedButtonsSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let labels = ["Today", "Tomorrow"];
        let draft = rsjs::signal("Today".to_string()); // proa-perf-ok: conformance fixture exercises String loop payload state

        ::proa_macros::html_sync! {
            <div>
                {for label in labels {
                    ::proa_macros::html_sync! {
                        <div>
                            <input bind_value={draft}/>
                            {if draft.get() == label.to_string() { // proa-perf-ok: conformance fixture exercises loop payload string conversion
                                ::proa_macros::html_sync! {
                                    <strong>{rsjs::server_only(label)}</strong>
                                }
                            } else {
                                ::proa_macros::html_sync! {
                                    <span>"Inactive"</span>
                                }
                            }}
                            <button type="button" on_click={|_| {
                                let selected = label.to_string(); // proa-perf-ok: conformance fixture exercises loop payload string conversion
                                console::log(selected);
                                self.on_select.call(selected);
                            }}>
                                {rsjs::server_only(label)}
                            </button>
                        </div>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for CallbackTextInputSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let current = rsjs::signal(self.value);

        ::proa_macros::html_sync! {
            <label>
                <span>{current.get()}</span>
                <input
                    value={current.get()}
                    on_input={|event| {
                    let next = event.value();
                    console::log(next);
                    self.on_change.call(next);
                }}
                />
            </label>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NamedEventHandlerSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let picked = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String event state
        let logged = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String event state
        let on_pick = rsjs::event_handler(|event: rsjs::Event, label: &'static str| {
            console::log(label);
            logged.set(label);
            picked.set(event.closest_attr("data-date"));
        });

        ::proa_macros::html_sync! {
            <section>
                <button data_date="sample-day" on_click={on_pick("today")}>"Pick"</button>
                <span>{picked.get()}</span>
                <span>{logged.get()}</span>
            </section>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for BranchingHandlerSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let selected = rsjs::signal("plain");

        ::proa_macros::html_sync! {
            <section>
                <button on_click={|event| {
                    if event.shift_key() {
                        if event.ctrl_key() {
                            selected.set("both");
                        } else {
                            selected.set("shift");
                        }
                    } else {
                        selected.set("plain");
                    }
                }}>
                    "Pick"
                </button>
                <span>{selected.get()}</span>
            </section>
        }
        .render(cx)
    }
}

pub struct StrategyVisibleSnapshot;

#[rsjs::rsjs(component, client, strategy = "visible")]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for StrategyVisibleSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let ready = rsjs::signal(0_i64);

        ::proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

pub struct StrategyIdleSnapshot;

#[rsjs::rsjs(component, client, strategy = "idle")]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for StrategyIdleSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let ready = rsjs::signal(0_i64);

        ::proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

pub struct StrategyInteractionSnapshot;

#[rsjs::rsjs(component, client, strategy = "interaction:click")]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for StrategyInteractionSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let ready = rsjs::signal(0_i64);

        ::proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

pub struct StrategyMediaSnapshot;

#[rsjs::rsjs(component, client, strategy = "media:(min-width: 768px)")]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for StrategyMediaSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let ready = rsjs::signal(0_i64);

        ::proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AsyncChildCompositionStockPrice {
    value: f64,
    currency: String,
}

pub struct AsyncChildCompositionDataChildSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRender<L, B>
    for AsyncChildCompositionDataChildSnapshot
{
    fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> ::proa_core::RenderOutcome<
        impl ::core::future::Future<Output = ::core::result::Result<(), ::proa_core::WriteError>>,
    > {
        ::proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL"); // proa-perf-ok: conformance fixture exercises server-local query/mutation keys
            let initial_price: AsyncChildCompositionStockPrice = cx
                .load_json(::proa_core::LoadKey::named("stock", symbol.as_str()))
                .await?;
            let price: rsjs::Query<AsyncChildCompositionStockPrice> = rsjs::query_with_initial(
                QueryKey::new(("child_stock", symbol.as_str())),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/child-stocks/AAPL",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            let save = rsjs::mutation_with_options::<&'static str, ()>(
                QueryKey::new(("child_update_cart", symbol.as_str())),
                rsjs::FetchRequest {
                    url: "/api/child-cart",
                    method: Some(rsjs::HttpMethod::Patch),
                    response: Some(rsjs::ResponseKind::Empty),
                    headers: &[("X-Mutate", "1")],
                    ..Default::default()
                },
                (QueryKey::new(("child_stock", symbol.as_str())),),
                rsjs::MutationOptions {
                    retry_count: 1,
                    on_success_invalidate_strategy: rsjs::InvalidationStrategy::Refetch,
                },
            );

            ::proa_macros::html! {
                <article>
                    <span data_currency={price.data().currency}>{price.data().value}</span>
                    <span>{save.status()}</span>
                    <span hidden={!save.is_pending()}></span>
                    <button
                        type="button"
                        on_click={|_| {
                        save.run("AAPL");
                        price.refetch();
                    }}
                    >
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

pub struct RepeatedGlobalEventsSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for RepeatedGlobalEventsSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let labels = ["A", "B"];

        ::proa_macros::html_sync! {
            <ul>
                {for label in labels {
                    ::proa_macros::html_sync! {
                        <li
                            on_document_keydown={|event: rsjs::KeyboardEvent<rsjs::web::dom::Document>| {
                                console::log(label.to_string(), event.key()); // proa-perf-ok: conformance fixture exercises loop payload string conversion
                            }}
                            on_window_resize={|_: rsjs::Event<rsjs::web::dom::Window>| {
                                console::log(label.to_string()); // proa-perf-ok: conformance fixture exercises loop payload string conversion
                            }}
                            on_window_scroll={|_: rsjs::Event<rsjs::web::dom::Window>| {
                                console::log(label.to_string(), rsjs::window::scroll_y()); // proa-perf-ok: conformance fixture exercises loop payload string conversion
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

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ServerInitialQueryStockPrice {
    value: f64,
    currency: String,
}

pub struct ServerInitialQuerySnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRender<L, B>
    for ServerInitialQuerySnapshot
{
    fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> ::proa_core::RenderOutcome<
        impl ::core::future::Future<Output = ::core::result::Result<(), ::proa_core::WriteError>>,
    > {
        ::proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL"); // proa-perf-ok: conformance fixture exercises server-local query keys
            let initial_price: ServerInitialQueryStockPrice = cx
                .load_json(::proa_core::LoadKey::named("stock", symbol.as_str()))
                .await?;
            let price: rsjs::Query<ServerInitialQueryStockPrice> = rsjs::query_with_initial(
                QueryKey::new(("stock", symbol.as_str())),
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

            ::proa_macros::html! {
                <section>
                    <span
                        data_currency={price.data().currency}
                        data_status={price.status()}
                    >
                        {price.data().value}
                    </span>
                    <button
                        type="button"
                        on_click={|_| {
                        console::log("refresh");
                        price.refetch();
                    }}
                    >
                        "Refresh"
                    </button>
                </section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

#[test]
fn basic_handler_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "BasicHandlerSnapshot")
        .expect("BasicHandlerSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/handler/basic_event_signal.island.js",
    );
}

#[test]
fn basic_handler_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "BasicHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/handler_basic_event_signal.body-ir.json",
    );
}

#[test]
fn basic_handler_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "BasicHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/handler_basic_event_signal.hir.json",
    );
}

#[test]
fn branching_handler_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "BranchingHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/handler_control_flow_if_else.body-ir.json",
    );
}

#[test]
fn branching_handler_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "BranchingHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/handler_control_flow_if_else.hir.json",
    );
}

#[test]
fn named_event_handler_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NamedEventHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/handler_named_event_refs.body-ir.json",
    );
}

#[test]
fn named_event_handler_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NamedEventHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/handler_named_event_refs.hir.json",
    );
}

#[test]
fn browser_effects_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "BrowserEffectsSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/browser_effects_p0.body-ir.json",
    );
}

#[test]
fn browser_effects_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "BrowserEffectsSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/browser_effects_p0.hir.json",
    );
}

#[test]
fn hydration_strategy_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "StrategyVisibleSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/hydration_strategies.body-ir.json",
    );
}

#[test]
fn hydration_strategy_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "StrategyVisibleSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/hydration_strategies.hir.json",
    );
}

#[test]
fn node_refs_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NodeRefsSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/node_refs_p0.body-ir.json",
    );
}

#[test]
fn node_refs_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NodeRefsSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/node_refs_p0.hir.json",
    );
}

#[test]
fn query_mutation_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "QueryMutationSnapshot",
        1,
        "rsjs_conformance/snapshots/client_ir/query_mutation_authoring.body-ir.json",
    );
}

#[test]
fn sync_query_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "SyncQuerySnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/query_sync_island.body-ir.json",
    );
}

#[test]
fn sync_query_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "SyncQuerySnapshot",
        0,
        "rsjs_conformance/snapshots/hir/query_sync_island.hir.json",
    );
}

#[test]
fn query_mutation_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "QueryMutationSnapshot",
        1,
        "rsjs_conformance/snapshots/hir/query_mutation_authoring.hir.json",
    );
}

#[test]
fn server_initial_query_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "ServerInitialQuerySnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/query_server_initial.body-ir.json",
    );
}

#[test]
fn server_initial_query_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "ServerInitialQuerySnapshot",
        0,
        "rsjs_conformance/snapshots/hir/query_server_initial.hir.json",
    );
}

#[test]
fn async_child_composition_mutation_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "AsyncChildCompositionDataChildSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/async_child_composition_mutation.body-ir.json",
    );
}

#[test]
fn async_child_composition_mutation_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "AsyncChildCompositionDataChildSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/async_child_composition_mutation.hir.json",
    );
}

#[test]
fn server_boundary_child_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "ServerDecoratedButtonSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/server_boundary_child.body-ir.json",
    );
}

#[test]
fn server_boundary_child_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "ServerDecoratedButtonSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/server_boundary_child.hir.json",
    );
}

#[test]
fn component_callback_child_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "CallbackTextInputSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/component_callback_basic.hir.json",
    );
}

#[test]
fn live_inbound_fields_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "LiveInboundFieldsSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/reactive_live_inbound_fields.body-ir.json",
    );
}

#[test]
fn live_inbound_fields_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "LiveInboundFieldsSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/reactive_live_inbound_fields.hir.json",
    );
}

#[test]
fn template_repetition_child_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "RepeatedButtonsSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/template_repetition_basic.hir.json",
    );
}

#[test]
fn repeated_global_events_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "RepeatedGlobalEventsSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/template_repeated_global_events.body-ir.json",
    );
}

#[test]
fn repeated_global_events_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "RepeatedGlobalEventsSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/template_repeated_global_events.hir.json",
    );
}

#[test]
fn template_repetition_fragment_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == "RepeatedButtonsSnapshot")
        .expect("RepeatedButtonsSnapshot fragment registered");
    assert_js_snapshot(
        fragment.js_source,
        "rsjs_conformance/snapshots/template/repetition.fragment.js",
    );
}

#[test]
fn repeated_global_events_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "RepeatedGlobalEventsSnapshot")
        .expect("RepeatedGlobalEventsSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/template/repeated_global_events.island.js",
    );
}

#[test]
fn template_repetition_child_descriptor_matches_conformance_snapshot() {
    let actual = normalize_descriptor_snapshot(component_descriptor_json(island_asset_id(
        "RepeatedButtonsSnapshot",
    )));
    if update_json_snapshot(
        &actual,
        "rsjs_conformance/snapshots/descriptor/template_repetition_child.descriptor.json",
        "descriptor",
    ) {
        return;
    }
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/descriptor/template_repetition_child.descriptor.json"
    ))
    .expect("descriptor snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

#[test]
fn browser_effects_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "BrowserEffectsSnapshot")
        .expect("BrowserEffectsSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/browser_api/browser_effects.island.js",
    );
}

#[test]
fn hydration_strategy_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let mut actual = String::new();
    for struct_name in [
        "StrategyVisibleSnapshot",
        "StrategyIdleSnapshot",
        "StrategyInteractionSnapshot",
        "StrategyMediaSnapshot",
    ] {
        let island = manifest
            .islands
            .iter()
            .find(|entry| entry.struct_name == struct_name)
            .unwrap_or_else(|| panic!("{struct_name} island registered"));
        actual.push_str("// ");
        actual.push_str(struct_name);
        actual.push('\n');
        actual.push_str(island.js_source);
        actual.push('\n');
    }
    assert_js_snapshot(
        &actual,
        "rsjs_conformance/snapshots/hydration/hydration_strategies.island.js",
    );
}

#[test]
fn server_boundary_fragment_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == "ServerDecoratedButtonSnapshot")
        .expect("ServerDecoratedButtonSnapshot fragment registered");
    assert_js_snapshot(
        fragment.js_source,
        "rsjs_conformance/snapshots/server_boundary/component_serverdecoratedbuttonsnapshot_8cfe6746.js",
    );
}

#[test]
fn server_boundary_child_descriptor_matches_conformance_snapshot() {
    let actual = normalize_descriptor_snapshot(component_descriptor_json(island_asset_id(
        "ServerDecoratedButtonSnapshot",
    )));
    if update_json_snapshot(
        &actual,
        "rsjs_conformance/snapshots/descriptor/server_boundary_child.descriptor.json",
        "descriptor",
    ) {
        return;
    }
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/descriptor/server_boundary_child.descriptor.json"
    ))
    .expect("descriptor snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

#[test]
fn node_refs_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NodeRefsSnapshot")
        .expect("NodeRefsSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/browser_api/node_refs.island.js",
    );
}

#[test]
fn component_callback_fragment_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == "CallbackTextInputSnapshot")
        .expect("CallbackTextInputSnapshot fragment registered");
    assert_js_snapshot(
        fragment.js_source,
        "rsjs_conformance/snapshots/component/component_callbacktextinputsnapshot_23897f3a.js",
    );
}

#[test]
fn live_inbound_fields_fragment_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == "LiveInboundFieldsSnapshot")
        .expect("LiveInboundFieldsSnapshot fragment registered");
    assert_js_snapshot(
        fragment.js_source,
        "rsjs_conformance/snapshots/component/live_inbound_fields.fragment.js",
    );
}

#[test]
fn live_inbound_fields_descriptor_matches_conformance_snapshot() {
    let asset_id = island_asset_id("LiveInboundFieldsSnapshot");
    let actual = normalize_descriptor_snapshot(component_descriptor_json(asset_id));
    if update_json_snapshot(
        &actual,
        "rsjs_conformance/snapshots/descriptor/reactive_live_inbound_fields.descriptor.json",
        "descriptor",
    ) {
        return;
    }
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/descriptor/reactive_live_inbound_fields.descriptor.json"
    ))
    .expect("descriptor snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

#[test]
fn named_event_handler_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NamedEventHandlerSnapshot")
        .expect("NamedEventHandlerSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/handler/named_event_handler_refs.island.js",
    );
}

#[test]
fn branching_handler_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "BranchingHandlerSnapshot")
        .expect("BranchingHandlerSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/handler/control_flow_if_else.island.js",
    );
}

#[test]
fn query_mutation_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "QueryMutationSnapshot")
        .expect("QueryMutationSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/query/query_mutation_authoring.island.js",
    );
}

#[test]
fn sync_query_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "SyncQuerySnapshot")
        .expect("SyncQuerySnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/query/sync_query.island.js",
    );
}

#[test]
fn server_initial_query_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "ServerInitialQuerySnapshot")
        .expect("ServerInitialQuerySnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/query/server_initial_query.island.js",
    );
}

#[test]
fn async_child_composition_data_fragment_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == "AsyncChildCompositionDataChildSnapshot")
        .expect("AsyncChildCompositionDataChildSnapshot fragment registered");
    assert_js_snapshot(
        fragment.js_source,
        "rsjs_conformance/snapshots/component/component_asyncchildcompositiondatachildsnapshot.js",
    );
}

#[test]
fn async_child_composition_data_descriptor_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == "AsyncChildCompositionDataChildSnapshot")
        .expect("AsyncChildCompositionDataChildSnapshot fragment registered");
    let actual = normalize_descriptor_snapshot(component_descriptor_json(fragment.asset_id));
    if update_json_snapshot(
        &actual,
        "rsjs_conformance/snapshots/descriptor/async_child_composition_data.descriptor.json",
        "descriptor",
    ) {
        return;
    }
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../rsjs_conformance/snapshots/descriptor/async_child_composition_data.descriptor.json"
    ))
    .expect("descriptor snapshot should be valid JSON");
    assert_eq!(actual, expected);
}

fn component_descriptor_json(asset_id: &str) -> serde_json::Value {
    let path = component_descriptor_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read descriptor {}: {err}", path.display()));
    serde_json::from_str(&json).expect("component descriptor should be valid JSON")
}

fn component_descriptor_path(asset_id: &str) -> std::path::PathBuf {
    let filename = format!("{asset_id}.json");
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .expect("rsjs should live under the workspace root");
    let mut candidates = Vec::new();
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
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

fn assert_client_ir_sidecar_handler_snapshot(
    struct_name: &str,
    handler_index: usize,
    relative_path: &str,
) {
    let asset_id = island_asset_id(struct_name);
    let sidecar = client_ir_sidecar_json(asset_id);
    assert_eq!(sidecar["format"], "rsjs.client_ir");
    assert_eq!(sidecar["client_ir_version"], 3);
    let actual = sidecar["handlers"][handler_index]["body_ir"].clone();
    if update_json_snapshot(&actual, relative_path, "Client IR") {
        return;
    }
    let expected = client_ir_snapshot_value(relative_path);
    assert_eq!(actual, expected);
}

fn assert_client_ir_sidecar_interval_snapshot(
    struct_name: &str,
    interval_index: usize,
    relative_path: &str,
) {
    let asset_id = island_asset_id(struct_name);
    let sidecar = client_ir_sidecar_json(asset_id);
    assert_eq!(sidecar["format"], "rsjs.client_ir");
    assert_eq!(sidecar["client_ir_version"], 3);
    let actual = sidecar["intervals"][interval_index]["body_ir"].clone();
    if update_json_snapshot(&actual, relative_path, "Client IR") {
        return;
    }
    let expected = client_ir_snapshot_value(relative_path);
    assert_eq!(actual, expected);
}

fn assert_hir_sidecar_handler_snapshot(
    struct_name: &str,
    handler_index: usize,
    relative_path: &str,
) {
    let asset_id = island_asset_id(struct_name);
    let sidecar = hir_sidecar_json(asset_id);
    assert_eq!(sidecar["format"], "rsjs.hir");
    assert_eq!(sidecar["hir_version"], 1);
    let mut actual = sidecar["handlers"][handler_index]["body_hir"].clone();
    normalize_hir_snapshot_spans(&mut actual);
    if update_json_snapshot(&actual, relative_path, "HIR") {
        return;
    }
    let expected = hir_snapshot_value(relative_path);
    assert_eq!(actual, expected);
}

fn assert_hir_sidecar_interval_snapshot(
    struct_name: &str,
    interval_index: usize,
    relative_path: &str,
) {
    let asset_id = island_asset_id(struct_name);
    let sidecar = hir_sidecar_json(asset_id);
    assert_eq!(sidecar["format"], "rsjs.hir");
    assert_eq!(sidecar["hir_version"], 1);
    let mut actual = sidecar["intervals"][interval_index]["body_hir"].clone();
    normalize_hir_snapshot_spans(&mut actual);
    if update_json_snapshot(&actual, relative_path, "HIR") {
        return;
    }
    let expected = hir_snapshot_value(relative_path);
    assert_eq!(actual, expected);
}

/// Rebases span line numbers in a serialized HIR body so the earliest
/// spanned line becomes line 1.
///
/// Invariant: HIR snapshots under `rsjs_conformance/snapshots/hir/` must
/// stay byte-stable when unrelated fixture islands in this file shift an
/// island's source up or down. The HIR sidecar records absolute
/// line/column positions in this shared fixture file, so without rebasing
/// every edit above an island forces a noisy re-bless of snapshots whose
/// HIR did not actually change. Rebasing keeps columns and relative line
/// distances intact, so the span shape and granularity checks in
/// `rsjs_conformance/report.mjs` (including distinct effect operand spans)
/// keep working. This normalization applies to the snapshot serialization
/// only; the real compiler diagnostics path keeps absolute spans.
fn normalize_hir_snapshot_spans(value: &mut serde_json::Value) {
    let Some(first_line) = min_hir_span_line(value) else {
        return;
    };
    rebase_hir_span_lines(value, first_line - 1);
}

fn min_hir_span_line(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .filter_map(|(key, entry)| {
                if key == "span" {
                    if let Some(line) = span_position_line(entry, "start") {
                        return Some(line);
                    }
                }
                min_hir_span_line(entry)
            })
            .min(),
        serde_json::Value::Array(items) => items.iter().filter_map(min_hir_span_line).min(),
        _ => None,
    }
}

fn rebase_hir_span_lines(value: &mut serde_json::Value, offset: u64) {
    if offset == 0 {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if key == "span" && rebase_span_object(entry, offset) {
                    continue;
                }
                rebase_hir_span_lines(entry, offset);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                rebase_hir_span_lines(item, offset);
            }
        }
        _ => {}
    }
}

fn span_position_line(span: &serde_json::Value, position: &str) -> Option<u64> {
    span.get(position)?.get("line")?.as_u64()
}

fn rebase_span_object(span: &mut serde_json::Value, offset: u64) -> bool {
    let (Some(start), Some(end)) = (
        span_position_line(span, "start"),
        span_position_line(span, "end"),
    ) else {
        return false;
    };
    let (Some(start), Some(end)) = (start.checked_sub(offset), end.checked_sub(offset)) else {
        return false;
    };
    span["start"]["line"] = serde_json::Value::from(start);
    span["end"]["line"] = serde_json::Value::from(end);
    true
}

fn update_json_snapshot(actual: &serde_json::Value, relative_path: &str, label: &str) -> bool {
    if std::env::var_os("RSJS_UPDATE_SNAPSHOTS").is_none() {
        return false;
    }
    let path = workspace_dir().join(relative_path);
    let text = serde_json::to_string_pretty(actual)
        .unwrap_or_else(|err| panic!("failed to format {label} snapshot JSON: {err}"));
    std::fs::write(&path, format!("{text}\n")).unwrap_or_else(|err| {
        panic!(
            "failed to update {label} snapshot {}: {err}",
            path.display()
        )
    });
    true
}

fn client_ir_snapshot_value(relative_path: &str) -> serde_json::Value {
    let path = workspace_dir().join(relative_path);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&text).expect("Client IR snapshot should be valid JSON")
}

fn hir_snapshot_value(relative_path: &str) -> serde_json::Value {
    let path = workspace_dir().join(relative_path);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&text).expect("HIR snapshot should be valid JSON")
}

fn island_asset_id(struct_name: &str) -> &'static str {
    let manifest = rsjs::manifest();
    if let Some(entry) = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == struct_name)
    {
        return entry.asset_id;
    }
    manifest
        .component_fragments
        .iter()
        .find(|entry| entry.struct_name == struct_name)
        .unwrap_or_else(|| panic!("{struct_name} island or component fragment registered"))
        .asset_id
}

fn client_ir_sidecar_json(asset_id: &str) -> serde_json::Value {
    let path = client_ir_sidecar_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read sidecar {}: {err}", path.display()));
    serde_json::from_str(&json).expect("Client IR sidecar should be valid JSON")
}

fn hir_sidecar_json(asset_id: &str) -> serde_json::Value {
    let path = hir_sidecar_path(asset_id);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read sidecar {}: {err}", path.display()));
    serde_json::from_str(&json).expect("HIR sidecar should be valid JSON")
}

fn client_ir_sidecar_path(asset_id: &str) -> std::path::PathBuf {
    sidecar_path(asset_id, "client_ir", "Client IR")
}

fn hir_sidecar_path(asset_id: &str) -> std::path::PathBuf {
    sidecar_path(asset_id, "hir", "HIR")
}

fn sidecar_path(asset_id: &str, subdir: &str, label: &str) -> std::path::PathBuf {
    let filename = format!("{asset_id}.json");
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .expect("rsjs should live under the workspace root");
    let mut candidates = Vec::new();
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(
            std::path::PathBuf::from(target_dir)
                .join("rsjs")
                .join(subdir)
                .join(&filename),
        );
    }
    candidates.extend([
        workspace_dir
            .join("target")
            .join("rsjs")
            .join(subdir)
            .join(&filename),
        manifest_dir
            .join("target")
            .join("rsjs")
            .join(subdir)
            .join(&filename),
        std::path::PathBuf::from("target")
            .join("rsjs")
            .join(subdir)
            .join(&filename),
    ]);
    candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "could not find rsjs {label} sidecar for {asset_id}; tried: {}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn normalize_descriptor_snapshot(mut descriptor: serde_json::Value) -> serde_json::Value {
    descriptor["asset_id"] = serde_json::json!("<asset_id>");
    descriptor["js_hash"] = serde_json::json!("sha256:<hash>");
    descriptor["descriptor_hash"] = serde_json::json!("sha256:<hash>");
    normalize_descriptor_snapshot_source_anchors(&mut descriptor);
    descriptor
}

/// Rebases descriptor source-anchor lines so snapshots stay stable when an
/// unrelated fixture above the component moves.
///
/// Descriptor anchors only record a start position, unlike full HIR spans.
/// Keeping the earliest real source line at 1 preserves columns and relative
/// distances while the compiler's on-disk descriptor retains absolute lines.
fn normalize_descriptor_snapshot_source_anchors(value: &mut serde_json::Value) {
    let Some(first_line) = min_descriptor_source_anchor_line(value) else {
        return;
    };
    if first_line == 0 {
        return;
    }
    rebase_descriptor_source_anchor_lines(value, first_line - 1);
}

fn min_descriptor_source_anchor_line(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .filter_map(|(key, entry)| {
                if key == "source_anchor" {
                    if let Some(line) = entry.get("start_line").and_then(serde_json::Value::as_u64)
                    {
                        if line > 0 {
                            return Some(line);
                        }
                    }
                }
                min_descriptor_source_anchor_line(entry)
            })
            .min(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(min_descriptor_source_anchor_line)
            .min(),
        _ => None,
    }
}

fn rebase_descriptor_source_anchor_lines(value: &mut serde_json::Value, offset: u64) {
    if offset == 0 {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if key == "source_anchor" {
                    if let Some(line) = entry.get("start_line").and_then(serde_json::Value::as_u64)
                    {
                        if line > 0 {
                            let Some(rebased) = line.checked_sub(offset) else {
                                continue;
                            };
                            entry["start_line"] = serde_json::Value::from(rebased);
                        }
                    }
                    continue;
                }
                rebase_descriptor_source_anchor_lines(entry, offset);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rebase_descriptor_source_anchor_lines(item, offset);
            }
        }
        _ => {}
    }
}

fn assert_js_snapshot(actual: &str, relative_path: &str) {
    let path = workspace_dir().join(relative_path);
    if std::env::var_os("RSJS_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual)
            .unwrap_or_else(|err| panic!("failed to update JS snapshot {}: {err}", path.display()));
        return;
    }

    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read JS snapshot {}: {err}", path.display()));
    assert_eq!(actual, expected);
}

fn workspace_dir() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("rsjs should live under the workspace root")
        .to_path_buf()
}

pub struct NumericI64DivisionModuloSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NumericI64DivisionModuloSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let neg_lhs = rsjs::signal(-7_i64);
        let pos_rhs = rsjs::signal(3_i64);
        let pos_lhs = rsjs::signal(7_i64);
        let neg_rhs = rsjs::signal(-3_i64);
        let quotient = rsjs::signal(0_i64);
        let remainder = rsjs::signal(0_i64);
        let doubled = rsjs::memo(|| neg_lhs.get() * 2_i64);

        ::proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    quotient.set(neg_lhs.get() / pos_rhs.get());
                    remainder.set(neg_lhs.get() % pos_rhs.get());
                }}>
                    "Negative dividend"
                </button>
                <button on_click={|_| {
                    quotient.set(pos_lhs.get() / neg_rhs.get());
                    remainder.set(pos_lhs.get() % neg_rhs.get());
                }}>
                    "Negative divisor"
                </button>
                <button on_click={|_| {
                    let dividend = neg_lhs.get();
                    let mut scaled = dividend / pos_rhs.get();
                    scaled /= 2_i64;
                    quotient.set(scaled);
                    remainder.set(doubled.get() / 4_i64);
                }}>
                    "Local division"
                </button>
                <span>{quotient.get()}</span>
                <span>{remainder.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct NumericI64PrecisionSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NumericI64PrecisionSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        // 9007199254740993 = 2^53 + 1, the first i64 a JS Number cannot
        // represent exactly.
        let big = rsjs::signal(9007199254740993_i64);
        let successor = rsjs::memo(|| big.get() + 1_i64);

        ::proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    big.set(big.get() + 1_i64);
                }}>
                    "Increment"
                </button>
                <button on_click={|_| {
                    big.update(|b| b + 1_i64);
                }}>
                    "Increment via update"
                </button>
                <button on_click={|_| {
                    let snapshot = big.get();
                    big.set(snapshot + 1_i64);
                }}>
                    "Increment via local"
                </button>
                <button on_click={|_| {
                    big.set(successor.get() + 1_i64);
                }}>
                    "Increment via memo"
                </button>
                <span>{big.get()}</span>
                <span>{successor.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct NumericF64SpecialSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NumericF64SpecialSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let zero = rsjs::signal(0.0_f64);
        let one = rsjs::signal(1.0_f64);
        let ratio = rsjs::signal(0.0_f64);
        let sum = rsjs::signal(0.0_f64);

        ::proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    ratio.set(zero.get() / zero.get());
                }}>
                    "NaN"
                </button>
                <button on_click={|_| {
                    ratio.set(one.get() / zero.get());
                }}>
                    "Infinity"
                </button>
                <button on_click={|_| {
                    sum.set(0.1_f64 + 0.2_f64);
                }}>
                    "Sum"
                </button>
                <span>{ratio.get()}</span>
                <span>{sum.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct NumericI32OverflowSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NumericI32OverflowSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let count = rsjs::signal(2147483647_i32);

        ::proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    count.set(count.get() + 1_i32);
                }}>
                    "Overflow"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct ConditionalTruthinessSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for ConditionalTruthinessSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let count = rsjs::signal(0_i64);
        let count_state = rsjs::signal("unset");
        let text_state = rsjs::signal("unset");

        ::proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    if count.get() == 0_i64 {
                        count_state.set("zero");
                    } else {
                        count_state.set("nonzero");
                    }
                    count.set(count.get() + 1_i64);
                }}>
                    "Check count"
                </button>
                <input on_input={|event| {
                    let next = event.value();
                    if next == "" {
                        text_state.set("empty");
                    } else {
                        text_state.set("filled");
                    }
                }} />
                <span>{count_state.get()}</span>
                <span>{text_state.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct MemoSignalsSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for MemoSignalsSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let price = rsjs::signal(10_i64);
        let qty = rsjs::signal(2_i64);
        let total = rsjs::memo(|| price.get() * qty.get());

        ::proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    qty.set(qty.get() + 1_i64);
                }}>
                    "Add one"
                </button>
                <span>{total.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn memo_signals_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "MemoSignalsSnapshot")
        .expect("MemoSignalsSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/signals/memo_signals.island.js",
    );
}

#[test]
fn memo_signals_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "MemoSignalsSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/signals_memo_basic.body-ir.json",
    );
}

#[test]
fn memo_signals_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "MemoSignalsSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/signals_memo_basic.hir.json",
    );
}

pub struct TimerIntervalSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for TimerIntervalSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let tick = rsjs::signal(0_i64);
        // 20ms keeps the conformance runtime test fast: it awaits a
        // handful of real timer ticks before disposing the island.
        rsjs::interval(20, || tick.update(|t| t + 1_i64));

        ::proa_macros::html_sync! {
            <div>
                <span>{tick.get()}</span>
            </div>
        }
        .render(cx)
    }
}

pub struct DebouncedHandlerSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for DebouncedHandlerSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let query = rsjs::signal(String::new());
        let calls = rsjs::signal(0_i64);

        ::proa_macros::html_sync! {
            <div>
                <input on_input={rsjs::debounce(|event| {
                    query.set(event.value());
                    calls.update(|c| c + 1_i64);
                }, 30)} />
                <span>{query.get()}</span>
                <span>{calls.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn timer_interval_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "TimerIntervalSnapshot")
        .expect("TimerIntervalSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/timers/timer_interval.island.js",
    );
}

#[test]
fn timer_interval_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_interval_snapshot(
        "TimerIntervalSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/timers_interval_basic.body-ir.json",
    );
}

#[test]
fn timer_interval_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_interval_snapshot(
        "TimerIntervalSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/timers_interval_basic.hir.json",
    );
}

#[test]
fn debounced_handler_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "DebouncedHandlerSnapshot")
        .expect("DebouncedHandlerSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/timers/debounced_handler.island.js",
    );
}

#[test]
fn debounced_handler_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "DebouncedHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/timers_debounce_basic.body-ir.json",
    );
}

#[test]
fn debounced_handler_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "DebouncedHandlerSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/timers_debounce_basic.hir.json",
    );
}

#[test]
fn numeric_i64_division_modulo_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NumericI64DivisionModuloSnapshot")
        .expect("NumericI64DivisionModuloSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/semantics/numeric_i64_division_modulo.island.js",
    );
}

#[test]
fn numeric_i64_division_modulo_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NumericI64DivisionModuloSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/semantics_numeric_i64_division_modulo.body-ir.json",
    );
}

#[test]
fn numeric_i64_division_modulo_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NumericI64DivisionModuloSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/semantics_numeric_i64_division_modulo.hir.json",
    );
}

#[test]
fn numeric_i64_precision_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NumericI64PrecisionSnapshot")
        .expect("NumericI64PrecisionSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/semantics/numeric_i64_precision.island.js",
    );
}

#[test]
fn numeric_i64_precision_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NumericI64PrecisionSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/semantics_numeric_i64_precision.body-ir.json",
    );
}

#[test]
fn numeric_i64_precision_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NumericI64PrecisionSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/semantics_numeric_i64_precision.hir.json",
    );
}

#[test]
fn numeric_f64_special_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NumericF64SpecialSnapshot")
        .expect("NumericF64SpecialSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/semantics/numeric_f64_special.island.js",
    );
}

#[test]
fn numeric_f64_special_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NumericF64SpecialSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/semantics_numeric_f64_special.body-ir.json",
    );
}

#[test]
fn numeric_f64_special_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NumericF64SpecialSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/semantics_numeric_f64_special.hir.json",
    );
}

#[test]
fn numeric_i32_overflow_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NumericI32OverflowSnapshot")
        .expect("NumericI32OverflowSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/semantics/numeric_i32_overflow.island.js",
    );
}

#[test]
fn numeric_i32_overflow_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NumericI32OverflowSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/semantics_numeric_i32_overflow.body-ir.json",
    );
}

#[test]
fn numeric_i32_overflow_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NumericI32OverflowSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/semantics_numeric_i32_overflow.hir.json",
    );
}

#[test]
fn conditional_truthiness_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "ConditionalTruthinessSnapshot")
        .expect("ConditionalTruthinessSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/semantics/conditional_truthiness.island.js",
    );
}

#[test]
fn conditional_truthiness_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "ConditionalTruthinessSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/semantics_conditional_truthiness.body-ir.json",
    );
}

#[test]
fn conditional_truthiness_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "ConditionalTruthinessSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/semantics_conditional_truthiness.hir.json",
    );
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
struct QueryMutationStockPrice {
    value: f64,
    currency: &'static str,
}

const QUERY_MUTATION_INITIAL_PRICE: QueryMutationStockPrice = QueryMutationStockPrice {
    value: 172.5,
    currency: "USD",
};

pub struct KeyedListSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for KeyedListSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let items = rsjs::signal(vec!["Alpha".to_string(), "Beta".to_string()]); // proa-perf-ok: conformance fixture seeds a Vec signal

        ::proa_macros::html_sync! {
            <div>
                <button
                    type="button"
                    on_click={|_| {
                        items.set(vec![
                            "Gamma".to_string(),
                            "Beta".to_string(),
                            "Alpha".to_string(),
                        ]); // proa-perf-ok: conformance fixture writes a literal list
                    }}
                >
                    "Shuffle"
                </button>
                <button
                    type="button"
                    on_click={|_| {
                        items.set(vec!["Gamma".to_string(), "Alpha".to_string()]); // proa-perf-ok: conformance fixture writes a literal list
                    }}
                >
                    "Remove"
                </button>
                <ul>
                    {for item in items.get() {
                        ::proa_macros::html_sync! {
                            <li class="todo" key={item}>{item}</li>
                        }
                    }}
                </ul>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn keyed_list_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "KeyedListSnapshot")
        .expect("KeyedListSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/template/list_keyed.island.js",
    );
}

#[test]
fn keyed_list_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "KeyedListSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/template_list_keyed.body-ir.json",
    );
}

#[test]
fn keyed_list_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "KeyedListSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/template_list_keyed.hir.json",
    );
}

pub struct NavigatorLanguageSnapshot;

#[rsjs::rsjs(component, client)]
impl<L: ::proa_core::DataLoader, B: ::proa_core::WriteBuf> ::proa_core::WebRenderSync<L, B>
    for NavigatorLanguageSnapshot
{
    fn render(
        self,
        cx: &mut ::proa_core::WebContext<L, B>,
    ) -> ::core::result::Result<(), ::proa_core::WriteError> {
        let detected = rsjs::signal(String::new()); // proa-perf-ok: conformance fixture exercises String signal state

        ::proa_macros::html_sync! {
            <div>
                <span>{rsjs::navigator::language()}</span>
                <button
                    type="button"
                    on_click={|_| {
                    detected.set(rsjs::navigator::language());
                }}
                >
                    "Detect"
                </button>
                <span>{detected.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn navigator_language_island_js_matches_conformance_snapshot() {
    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "NavigatorLanguageSnapshot")
        .expect("NavigatorLanguageSnapshot island registered");
    assert_js_snapshot(
        island.js_source,
        "rsjs_conformance/snapshots/browser_api/navigator_language.island.js",
    );
}

#[test]
fn navigator_language_client_ir_sidecar_matches_conformance_snapshot() {
    assert_client_ir_sidecar_handler_snapshot(
        "NavigatorLanguageSnapshot",
        0,
        "rsjs_conformance/snapshots/client_ir/browser_navigator_language.body-ir.json",
    );
}

#[test]
fn navigator_language_hir_sidecar_matches_conformance_snapshot() {
    assert_hir_sidecar_handler_snapshot(
        "NavigatorLanguageSnapshot",
        0,
        "rsjs_conformance/snapshots/hir/browser_navigator_language.hir.json",
    );
}
