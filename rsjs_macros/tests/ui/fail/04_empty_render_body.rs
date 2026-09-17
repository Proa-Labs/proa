// Category 4 (sync/async confusion proxy): an empty render body has
// no tail `html_sync!` / `html!` invocation. The same code path
// catches "tail is a non-expression statement" — see also
// `03_wrong_tail_macro.rs`.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, _cx: &mut WebContext<L, B>) -> Result<(), WriteError> {}
}

fn main() {}
