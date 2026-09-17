#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../../conformance/support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

mod fetch {
    pub fn get(_url: &str) {}
}

pub struct RawFetchCall;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RawFetchCall {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button on_click={|_| {
                fetch::get("/api/items");
            }}>
                "Load"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
