#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NumericI64DivisionModulo;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NumericI64DivisionModulo {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let neg_lhs = signal(-7_i64);
        let pos_rhs = signal(3_i64);
        let pos_lhs = signal(7_i64);
        let neg_rhs = signal(-3_i64);
        let quotient = signal(0_i64);
        let remainder = signal(0_i64);
        let doubled = rsjs::memo(|| neg_lhs.get() * 2_i64);

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    quotient.set(neg_lhs.get() / pos_rhs.get());
                    remainder.set(neg_lhs.get() % pos_rhs.get());
                }}>
                    "Negative dividend"
                </button>
                <button on_click={|_| {
                    quotient.set(pos_lhs.get() / neg_rhs.get());
                    remainder.set(pos_lhs.get() % neg_rhs.get());
                }}>
                    "Negative divisor"
                </button>
                <button on_click={|_| {
                    let dividend = neg_lhs.get();
                    let mut scaled = dividend / pos_rhs.get();
                    scaled /= 2_i64;
                    quotient.set(scaled);
                    remainder.set(doubled.get() / 4_i64);
                }}>
                    "Local division"
                </button>
                <span>{quotient.get()}</span>
                <span>{remainder.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
