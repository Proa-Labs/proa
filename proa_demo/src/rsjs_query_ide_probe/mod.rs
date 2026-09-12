// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Small real-crate rsjs query fixture for rust-analyzer testing.

#[allow(unused_imports)]
use proa_core::{DataLoader, LoadKey, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError};
#[allow(unused_imports)]
use rsjs::elem;
use rsjs::QueryKey;
use rsjs_macros::rsjs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockPrice {
    pub value: f64,
    pub currency: String,
}

pub struct StockTicker;

#[allow(dead_code)]
pub async fn stock_query_handle_ide_probe<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    let symbol = "AAPL";
    let initial_price: StockPrice = cx.load_json(LoadKey::named("stock", symbol)).await?;

    let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
        QueryKey::new(("stock", symbol)),
        initial_price,
        rsjs::FetchRequest {
            url: "/api/stocks/AAPL",
            response: Some(rsjs::ResponseKind::Json),
            ..Default::default()
        },
        rsjs::QueryOptions {
            stale_time_ms: 2_000,
            refetch_on_focus: true,
            ..Default::default()
        },
    );

    let _currency = &price.data().currency;
    let _value = price.data().value;
    price.refetch();
    Ok(())
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for StockTicker {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let symbol = "AAPL";
            let initial_price: StockPrice = cx.load_json(LoadKey::named("stock", symbol)).await?;

            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
                QueryKey::new(("stock", symbol)),
                initial_price,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    response: Some(rsjs::ResponseKind::Json),
                    ..Default::default()
                },
                rsjs::QueryOptions {
                    stale_time_ms: 2_000,
                    refetch_on_focus: true,
                    ..Default::default()
                },
            );

            let refresh_on_click = rsjs::event_handler(|_: rsjs::MouseEvent<elem::Button>| {
                price.refetch();
            });

            proa_macros::html! {
                <section data-stock-ticker>
                    <span data-currency={price.data().currency}>{price.data().value}</span>
                    <button type="button" on_click={refresh_on_click()}>"Refresh"</button>
                </section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
