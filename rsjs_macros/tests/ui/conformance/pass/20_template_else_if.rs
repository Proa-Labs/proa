#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct TemplateElseIf;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for TemplateElseIf {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = signal("idle".to_string());

        proa_macros::html_sync! {
            <section>
                {if state.get() == "active" {
                    proa_macros::html_sync! {
                        <span>"Active"</span>
                    }
                } else if state.get() == "paused" {
                    proa_macros::html_sync! {
                        <em>"Paused"</em>
                    }
                } else {
                    proa_macros::html_sync! {
                        <strong>"Idle"</strong>
                    }
                }}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
