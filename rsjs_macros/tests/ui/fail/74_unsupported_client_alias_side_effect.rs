#![allow(unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct UnsupportedAlias;

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for UnsupportedAlias {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);
        let _display = {
            println!("this server effect must not disappear");
            format!("{}", count.get())
        };

        proa_macros::html_sync! {
            <button on_click={|_| count.update(|value| value + 1)}>
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
