// Ordinary Rust statements remain part of the authored server program even
// when the analyzed render site has reachable browser behavior.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);
        println!("hello from render");
        proa_macros::html_sync! {
            <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
        }
        .render(cx)
    }
}

fn main() {}
