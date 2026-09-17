use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ConditionalTypedEvent;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ConditionalTypedEvent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let visible = rsjs::signal(true);

        proa_macros::html_sync! {
            <section>
                {if visible.get() {
                    proa_macros::html_sync! {
                        <button on_click={|_: rsjs::MouseEvent<rsjs::web::dom::HtmlInputElement>| {}}>
                            "Wrong target"
                        </button>
                    }
                }}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
