// Category 3: `#[rsjs(component, client)]` rejects a render body whose tail is a macro
// other than `html_sync!` / `html!`.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        println!("hello").render(_cx)
    }
}

fn main() {}
