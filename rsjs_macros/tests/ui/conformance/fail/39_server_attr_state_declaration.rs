#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ServerAttrStateDeclaration;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerAttrStateDeclaration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button class={rsjs::server_attr(signal("active").get())}>"Save"</button>
        }
        .render(cx)
    }
}

fn main() {}
