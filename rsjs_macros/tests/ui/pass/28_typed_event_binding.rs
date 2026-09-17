use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct TypedEvents;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TypedEvents {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = rsjs::signal(String::new());
        let aria_label = rsjs::signal(String::new());
        let width = rsjs::signal(0_i32);
        let input = rsjs::node_ref::<rsjs::web::dom::HtmlInputElement>();
        let on_input = rsjs::event_handler(
            |event: rsjs::InputEvent<rsjs::web::dom::HtmlInputElement>| {
                let input = event.current_target();
                value.set(input.value());
                aria_label.set(input.get_attribute("aria-label"));
                width.set(input.client_width());
            },
        );
        let on_click = rsjs::event_handler(
            |_: rsjs::MouseEvent<rsjs::web::dom::HtmlButtonElement>, _id: u32| {},
        );
        let on_document_click = rsjs::event_handler(
            |event: rsjs::MouseEvent<rsjs::web::dom::Document>| {
                let _document = event.current_target();
            },
        );
        let on_window_resize =
            rsjs::event_handler(|event: rsjs::Event<rsjs::web::dom::Window>| {
                let _window = event.current_target();
            });

        proa_macros::html_sync! {
            <div
                on_document_click={on_document_click()}
                on_window_resize={on_window_resize()}
            >
                <input ref={input} on_input={on_input()} />
                <button on_click={on_click(7_u32)}>"Save"</button>
                <span>{value.get()}</span>
                <span>{aria_label.get()}</span>
                <span>{width.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
