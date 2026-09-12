#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct DynamicUrlAttr;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for DynamicUrlAttr {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let url = signal("javascript:alert(1)");

        proa_macros::html_sync! {
            <div>
                <a href={url.get()}>"Bad link"</a>
                <button on_click={|_| url.set("https://example.com/safe")}>"Use safe link"</button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
