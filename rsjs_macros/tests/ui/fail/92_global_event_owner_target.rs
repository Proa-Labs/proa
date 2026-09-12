use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct GlobalEventOwnerTarget;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for GlobalEventOwnerTarget {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>
                <div on_document_click={|event: rsjs::MouseEvent| {
                    let _owner_attribute = event.current_target_attr("data-menu-id");
                }}>
                    "Document handlers require an exact target"
                </div>
                <div on_document_click={|_: rsjs::MouseEvent<rsjs::web::dom::HtmlDivElement>| {}}>
                    "The owner element is not the document current target"
                </div>
                <div on_window_resize={|event: rsjs::Event| {
                    let _owner_id = event.current_target_id();
                }}>
                    "Window handlers require an exact target"
                </div>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
