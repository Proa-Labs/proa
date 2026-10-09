#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NumericI64Precision;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NumericI64Precision {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // 9007199254740993 = 2^53 + 1, the first i64 a JS Number cannot
        // represent exactly.
        let big = signal(9007199254740993_i64);
        let successor = rsjs::memo(|| big.get() + 1_i64);

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    big.set(big.get() + 1_i64);
                }}>
                    "Increment"
                </button>
                <button on_click={|_| {
                    big.update(|b| b + 1_i64);
                }}>
                    "Increment via update"
                </button>
                <button on_click={|_| {
                    let snapshot = big.get();
                    big.set(snapshot + 1_i64);
                }}>
                    "Increment via local"
                </button>
                <button on_click={|_| {
                    big.set(successor.get() + 1_i64);
                }}>
                    "Increment via memo"
                </button>
                <span>{big.get()}</span>
                <span>{successor.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
