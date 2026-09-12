#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::WebRender;
use rsjs::{rsjs, QueryKey};
use support::*;

pub struct QueryServerFetcher;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for QueryServerFetcher {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let price = rsjs::query_with_initial(
                QueryKey::new(("stock", "AAPL")),
                rsjs::server_fetcher(|_cx| async move {
                    Ok::<_, WriteError>(serde_json::json!({
                        "value": 172.5,
                        "currency": "USD",
                    }))
                }),
                "/api/stocks/AAPL",
                rsjs::QueryOptions::default(),
            );

            proa_macros::html! {
                <span>{price.data().at("/value").as_f64()}</span>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

fn main() {}
