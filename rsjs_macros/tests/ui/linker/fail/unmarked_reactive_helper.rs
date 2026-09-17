#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../../conformance/support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

mod pricing {
    pub fn subtotal(value: i64, quantity: i64) -> i64 {
        value * quantity
    }
}

pub struct UnmarkedReactiveHelper;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for UnmarkedReactiveHelper {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <div><span>{pricing::subtotal(count.get(), 2)}</span></div>
        }
        .render(cx)
    }
}

fn main() {}
