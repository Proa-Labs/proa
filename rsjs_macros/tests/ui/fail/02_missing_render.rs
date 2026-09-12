// Category 2: `#[rsjs(component, client)]` rejects an impl block missing the `render` method.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn other(self) {}
}

fn main() {}
