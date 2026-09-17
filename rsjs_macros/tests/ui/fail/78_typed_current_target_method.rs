use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct WrongCurrentTargetMethod;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for WrongCurrentTargetMethod {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let checked = rsjs::signal(false);
        let on_click =
            rsjs::event_handler(|event: rsjs::MouseEvent<rsjs::elem::Button>| {
                let button = event.current_target();
                checked.set(button.checked());
            });

        proa_macros::html_sync! {
            <button on_click={on_click()}>"Click"</button>
        }
        .render(cx)
    }
}

fn main() {}
