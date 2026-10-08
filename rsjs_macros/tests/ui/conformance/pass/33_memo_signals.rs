#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct MemoSignals;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for MemoSignals {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price = signal(10_i64);
        let qty = signal(2_i64);
        let total = rsjs::memo(|| price.get() * qty.get());

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    qty.set(qty.get() + 1_i64);
                }}>
                    "Add one"
                </button>
                <span>{total.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
