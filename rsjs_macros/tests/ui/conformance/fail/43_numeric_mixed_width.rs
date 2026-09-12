#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NumericMixedWidth;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for NumericMixedWidth {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // `big` is an exact `i64`/BigInt cell while `count` is an exact
        // `i32`/Number cell. Rust does not implicitly combine those domains,
        // so rsjs rejects the mixed-width arithmetic at compile time too.
        let big = signal(9007199254740993_i64);
        let count = signal(0_i32);

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    big.set(big.get() + count.get());
                }}>"Mix widths"</button>
                <span>{big.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
