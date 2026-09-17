#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerLocalCompoundAssignment;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for HandlerLocalCompoundAssignment
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    let mut next = count.get();
                    next += 4_i64;
                    next %= 3_i64;
                    count.set(next);
                }}>
                    "Apply"
                </button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
