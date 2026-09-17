use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct GlobalCurrentTargetElementMethod;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for GlobalCurrentTargetElementMethod
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let inspect_document = rsjs::event_handler(
            |event: rsjs::MouseEvent<rsjs::web::dom::Document>| {
                let _id = event.current_target().id();
            },
        );
        proa_macros::html_sync! {
            <div on_document_click={inspect_document()}>"Inspect"</div>
        }
        .render(cx)
    }
}

fn main() {}
