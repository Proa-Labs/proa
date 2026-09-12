use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-div-int-type-witness")]
pub struct WideState {
    pub value: u64,
    pub divisor: u64,
}

pub struct WrongExplicitWidth;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for WrongExplicitWidth {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<WideState>();
        let value = state.value();
        let divisor = state.divisor();
        proa_macros::html_sync! {
            <button on_click={|_| {
                value.set(rsjs::div_int::<i32>(value.get(), divisor.get()));
            }}>
                {value.get()}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
