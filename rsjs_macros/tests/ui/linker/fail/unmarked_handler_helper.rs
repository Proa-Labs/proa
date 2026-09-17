//! Link-stage rejection: the Rust call resolves, but the function has no
//! `#[rsjs::client]` definition record.

#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../../conformance/support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

fn ordinary_helper(value: i64) -> i64 {
    value + 1
}

pub struct UnmarkedHelper;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for UnmarkedHelper {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <button on_click={|_| {
                count.set(ordinary_helper(count.get()));
            }}>
                "Apply"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
