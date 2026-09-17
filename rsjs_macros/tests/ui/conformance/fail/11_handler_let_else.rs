#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerLetElse;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerLetElse {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let last_value = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <button on_click={|event| {
                    let value = event.value() else {
                        return;
                    };
                    last_value.set(value);
                }}>
                    "Save"
                </button>
                <span>{last_value.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
