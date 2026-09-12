#![allow(unused_imports)]

use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct TypedSignalConditionalUnsupported;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B>
    for TypedSignalConditionalUnsupported
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let flag = rsjs::signal(false);
        let output = rsjs::signal::<i64>(0);
        proa_macros::html_sync! {
            <button on_click={|_| {
                output.set(if flag.get() { 1_u8 } else { 2 });
            }}>
                {output.get()}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
