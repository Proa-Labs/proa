use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct WrongLocalNodeRef;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for WrongLocalNodeRef {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let input = rsjs::node_ref::<rsjs::web::dom::HtmlInputElement>();

        proa_macros::html_sync! {
            <button ref={input}>"Not an input"</button>
        }
        .render(cx)
    }
}

fn main() {}
