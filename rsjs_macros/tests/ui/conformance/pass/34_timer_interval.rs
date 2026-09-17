#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct TimerInterval;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TimerInterval {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let tick = signal(0_i64);
        rsjs::interval(1000, || tick.update(|t| t + 1_i64));

        proa_macros::html_sync! {
            <div>
                <span>{tick.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
