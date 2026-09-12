// Category 2: `#[rsjs(component, client)]` rejects an impl block with two `render` methods.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

fn main() {}
