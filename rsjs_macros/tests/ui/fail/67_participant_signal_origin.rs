// A participant-only component may consume live values, but it cannot
// originate writable client state without the explicit `client` capability.

#[allow(unused_imports)]
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);
        proa_macros::html_sync! {
            <output>{count.get()}</output>
        }
        .render(cx)
    }
}

fn main() {}
