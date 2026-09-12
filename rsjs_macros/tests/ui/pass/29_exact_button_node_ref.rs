use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ButtonRefParticipant {
    pub target: rsjs::NodeRef<rsjs::web::dom::HtmlButtonElement>,
}

#[rsjs(component)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ButtonRefParticipant {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button ref={self.target}>"Exact button"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
