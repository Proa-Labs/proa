use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct HandlerBorrowedTemporary;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for HandlerBorrowedTemporary {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = rsjs::signal(String::new());
        proa_macros::html_sync! {
            <input on_input={|event: rsjs::InputEvent<rsjs::web::dom::HtmlInputElement>| {
                let next = event.value().trim();
                query.set(next);
            }} />
        }
        .render(cx)
    }
}

fn main() {}
