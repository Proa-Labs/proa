use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ConditionalTypedNodeRef;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ConditionalTypedNodeRef {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let visible = rsjs::signal(true);
        let input = rsjs::node_ref::<rsjs::web::dom::HtmlInputElement>();

        proa_macros::html_sync! {
            <section>
                {if visible.get() {
                    proa_macros::html_sync! {
                        <button ref={input}>"Wrong target"</button>
                    }
                }}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
