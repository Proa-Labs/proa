#![allow(unused_imports)]

use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-div-int-order-independent")]
pub struct NumericState {
    pub value: i32,
    pub divisor: i32,
}

pub struct OrderIndependent;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for OrderIndependent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<NumericState>();
        let value = state.value();
        let divisor = state.divisor();
        proa_macros::html_sync! {
            <button on_click={|_| {
                value.set(rsjs::div_int::<i32>(value.get(), divisor.get()));
                value.set(rsjs::div_int(value.get(), divisor.get()));
            }}>
                {value.get()}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
