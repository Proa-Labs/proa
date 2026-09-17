#![allow(unused_imports)]

use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-div-int-missing-right")]
pub struct NumericState {
    pub value: i32,
}

pub struct MissingRight;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for MissingRight {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<NumericState>();
        let value = state.value();
        proa_macros::html_sync! {
            <button on_click={|_| {
                value.set(rsjs::div_int(2_i32, value.get()));
            }}>
                {value.get()}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
