#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NumericF64Special;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NumericF64Special {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let zero = signal(0.0_f64);
        let one = signal(1.0_f64);
        let ratio = signal(0.0_f64);
        let sum = signal(0.0_f64);

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    ratio.set(zero.get() / zero.get());
                }}>
                    "NaN"
                </button>
                <button on_click={|_| {
                    ratio.set(one.get() / zero.get());
                }}>
                    "Infinity"
                </button>
                <button on_click={|_| {
                    sum.set(0.1_f64 + 0.2_f64);
                }}>
                    "Sum"
                </button>
                <span>{ratio.get()}</span>
                <span>{sum.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
