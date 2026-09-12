#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::WebRender;
use rsjs::{rsjs, QueryKey};
use support::*;

#[derive(Debug, Clone, serde::Serialize)]
struct StockPrice {
    value: f64,
    currency: &'static str,
}

pub struct QueryMutationAuthoring;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for QueryMutationAuthoring {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let draft = signal(String::new()); // proa-perf-ok: conformance fixture exercises String mutation payload state
            let initial_price = StockPrice {
                value: 172.5,
                currency: "USD",
            };
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                QueryKey::new(("stock", "AAPL")),
                initial_price,
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

            proa_macros::html! {
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

fn main() {}
