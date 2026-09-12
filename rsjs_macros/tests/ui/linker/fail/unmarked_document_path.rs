#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../../conformance/support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

mod document {
    pub fn query_selector(_selector: &str) {}
}

pub struct RawBrowserGlobal;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for RawBrowserGlobal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button on_click={|_| {
                document::query_selector("[data-menu]");
            }}>
                "Open"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
