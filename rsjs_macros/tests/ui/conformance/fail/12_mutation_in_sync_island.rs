#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct MutationInSyncIsland;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for MutationInSyncIsland {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let save = rsjs::mutation::<(), ()>("stock:AAPL", "/api/stocks/AAPL", ["stock:AAPL"]);

        proa_macros::html_sync! {
            <div>{save.is_idle()}</div>
        }
        .render(cx)
    }
}

fn main() {}
