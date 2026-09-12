use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct WrongEventInterface;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for WrongEventInterface {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let on_input =
            rsjs::event_handler(|_: rsjs::MouseEvent<rsjs::elem::Input>| {});

        proa_macros::html_sync! {
            <input on_input={on_input()} />
        }
        .render(cx)
    }
}

fn main() {}
