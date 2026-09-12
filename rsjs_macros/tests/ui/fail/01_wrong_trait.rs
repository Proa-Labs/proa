// Category 1: a canonical-looking render method on an unrelated trait fails
// the hidden rustc-resolved Proa render-trait witness.

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

trait SomeOtherTrait<B: WriteBuf, L: DataLoader> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>;
}

pub struct Counter;

#[rsjs(component)]
impl<B: WriteBuf, L: DataLoader> SomeOtherTrait<B, L> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>"hi"</div>
        }
        .render(cx)
    }
}

fn main() {}
