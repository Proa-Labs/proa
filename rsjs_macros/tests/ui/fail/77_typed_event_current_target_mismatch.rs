use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct WrongCurrentTarget;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for WrongCurrentTarget {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let on_input =
            rsjs::event_handler(|_: rsjs::InputEvent<rsjs::elem::Button>| {});

        proa_macros::html_sync! {
            <input on_input={on_input()} />
        }
        .render(cx)
    }
}

fn main() {}
