#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::UnsafeRaw;
use rsjs::rsjs;
use support::*;

pub struct ServerOnlySignalCapture;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ServerOnlySignalCapture {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <div>{rsjs::server_only(UnsafeRaw(count.get()))}</div>
        }
        .render(cx)
    }
}

fn main() {}
