#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NavigatorLanguage;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for NavigatorLanguage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let detected = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <span>{rsjs::navigator::language()}</span>
                <button
                    type="button"
                    on_click={|_| {
                    detected.set(rsjs::navigator::language());
                }}
                >
                    "Detect"
                </button>
                <span>{detected.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
