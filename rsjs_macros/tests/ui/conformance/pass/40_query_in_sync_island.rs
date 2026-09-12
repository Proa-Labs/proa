#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::{rsjs, QueryKey};
use support::*;

pub struct QueryInSyncIsland;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for QueryInSyncIsland {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price: rsjs::Query<i64> = rsjs::query_with_initial(
            QueryKey::new(("stock", "AAPL")),
            172_i64,
            "/api/stocks/AAPL",
            rsjs::QueryOptions::default(),
        );

        proa_macros::html_sync! {
            <div>{if price.is_loading() { "loading" } else { "ready" }}</div>
        }
        .render(cx)
    }
}

fn main() {}
