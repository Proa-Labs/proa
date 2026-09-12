use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct WrongInlineEventInterface;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for WrongInlineEventInterface {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button on_click={|_: rsjs::InputEvent<rsjs::elem::Button>| {}}>
                "Click"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
