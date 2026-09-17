// `#[rsjs(component, client, render = "client")]` is intentionally reserved until the
// compiler has an explicit fallback/no-JS contract. The parser should not
// treat client-only as an inferred or partially supported mode.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ClientOnlyCounter;

#[rsjs(component, client, render = "client")]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ClientOnlyCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>"hi"</div>
        }
        .render(cx)
    }
}

fn main() {}
