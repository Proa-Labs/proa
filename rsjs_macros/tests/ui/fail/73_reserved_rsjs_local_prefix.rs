#![allow(unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ReservedLocal;

#[rsjs(component)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for ReservedLocal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let __rsjs_initial_signal_0 = "authored";

        proa_macros::html_sync! {
            <main>{__rsjs_initial_signal_0}</main>
        }
        .render(cx)
    }
}

fn main() {}
