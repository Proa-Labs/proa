#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NumericI32Overflow;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NumericI32Overflow {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(2147483647_i32);

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    count.set(count.get() + 1_i32);
                }}>
                    "Overflow"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
