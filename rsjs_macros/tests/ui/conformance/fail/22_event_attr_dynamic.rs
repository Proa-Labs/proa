#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct EventAttrDynamic;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for EventAttrDynamic {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let attr = "data-id";

        proa_macros::html_sync! {
            <div>
                <button data_id="42" on_click={|event| value.set(event.closest_attr(attr))}>
                    "Read"
                </button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
