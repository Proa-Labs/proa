#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::WebRender;
use rsjs::{rsjs, QueryKey};
use support::*;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StockPrice {
    value: f64,
    currency: String,
}

async fn async_child_label() -> Result<&'static str, WriteError> {
    Ok("Async composition child")
}

pub struct AsyncPreludeChild;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for AsyncPreludeChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
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

pub struct DataQueryMutationChild;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for DataQueryMutationChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let symbol = String::from("AAPL"); // proa-perf-ok: conformance fixture exercises server-local query/mutation keys
            let initial_price: StockPrice = cx
                .load_json(proa_core::LoadKey::named("stock", symbol.as_str()))
                .await?;
            let price: rsjs::Query<StockPrice> = rsjs::query_with_initial(
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

            proa_macros::html! {
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

pub struct AsyncCompositionParent;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for AsyncCompositionParent {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            proa_macros::html! {
                <section>{AsyncPreludeChild {}}{DataQueryMutationChild {}}</section>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

fn main() {}
