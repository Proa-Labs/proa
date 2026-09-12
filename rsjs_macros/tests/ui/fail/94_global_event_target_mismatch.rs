use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct GlobalEventTargetMismatch;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B>
    for GlobalEventTargetMismatch
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div on_window_resize={|_: rsjs::Event<rsjs::web::dom::Document>| {}}>
                "Document is not Window"
            </div>
        }
        .render(cx)
    }
}

fn main() {}
