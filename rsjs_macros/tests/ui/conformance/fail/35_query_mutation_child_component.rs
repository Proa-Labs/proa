#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::{rsjs, QueryKey};
use support::*;

pub struct QueryChild;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> proa_core::WebRender<L, B> for QueryChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            use proa_core::WebRender as _;

            let price: rsjs::Query<f64> = rsjs::query_with_initial(
                QueryKey::new(("stock", "AAPL")),
                172.5_f64,
                "/api/stocks/AAPL",
                rsjs::QueryOptions::default(),
            );

            proa_macros::html! {
                <span>{price.data().as_f64()}</span>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct Parent;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Parent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        use proa_core::WebRenderSync as _;

        proa_macros::html_sync! {
            <section>
                {QueryChild {}}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
