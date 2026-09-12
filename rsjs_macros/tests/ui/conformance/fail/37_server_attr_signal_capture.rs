#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ServerAttrSignalCapture;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ServerAttrSignalCapture {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal("active");

        proa_macros::html_sync! {
            <button class={rsjs::server_attr(selected.get())}>"Save"</button>
        }
        .render(cx)
    }
}

fn main() {}
