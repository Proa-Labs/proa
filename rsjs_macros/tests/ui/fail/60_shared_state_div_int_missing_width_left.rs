#![allow(unused_imports)]

use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-div-int-missing-left")]
pub struct NumericState {
    pub value: i32,
}

pub struct MissingLeft;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for MissingLeft {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<NumericState>();
        let value = state.value();
        proa_macros::html_sync! {
            <button on_click={|_| {
                value.set(rsjs::div_int(value.get(), 2_i32));
            }}>
                {value.get()}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
