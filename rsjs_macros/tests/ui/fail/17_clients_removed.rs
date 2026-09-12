// Component-level helper allowlists were removed. A helper's one
// `#[rsjs::client]` definition is sufficient, and normal Rust paths are
// resolved by the semantic linker.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component, client, clients(crate::helpers))]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>"hi"</div>
        }
        .render(cx)
    }
}

fn main() {}
