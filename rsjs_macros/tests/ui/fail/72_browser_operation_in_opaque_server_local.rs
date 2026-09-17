#![allow(unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct BrowserRead;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BrowserRead {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let _ = rsjs::location::href();

        proa_macros::html_sync! {
            <main>"server"</main>
        }
        .render(cx)
    }
}

fn main() {}
