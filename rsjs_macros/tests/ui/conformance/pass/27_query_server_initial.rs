#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::{LoadKey, WebRender};
use rsjs::{rsjs, QueryKey};
use support::*;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StockPrice {
    value: f64,
    currency: String,
}

pub struct ServerInitialQuery;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRender<L, B> for ServerInitialQuery {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL");
            let initial_price: StockPrice = cx
                .load_json(LoadKey::named("stock", symbol.as_str()))
                .await?;
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
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

            proa_macros::html! {
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

fn main() {}
