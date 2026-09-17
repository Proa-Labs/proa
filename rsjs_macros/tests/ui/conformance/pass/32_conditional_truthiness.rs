#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ConditionalTruthiness;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ConditionalTruthiness {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let count_state = signal("unset");
        let text_state = signal("unset");

        proa_macros::html_sync! {
            <div>
                <button on_click={|_| {
                    if count.get() == 0_i64 {
                        count_state.set("zero");
                    } else {
                        count_state.set("nonzero");
                    }
                    count.set(count.get() + 1_i64);
                }}>
                    "Check count"
                </button>
                <input on_input={|event| {
                    let next = event.value();
                    if next == "" {
                        text_state.set("empty");
                    } else {
                        text_state.set("filled");
                    }
                }} />
                <span>{count_state.get()}</span>
                <span>{text_state.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
