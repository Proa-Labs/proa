#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct BranchingChild {
    pub on_pick: rsjs::Handler<&'static str>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BranchingChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button on_click={|event| {
                if event.shift_key() {
                    if event.ctrl_key() {
                        self.on_pick.call("both");
                    } else {
                        self.on_pick.call("shift");
                    }
                } else {
                    self.on_pick.call("plain");
                }
            }}>
                "Pick"
            </button>
        }
        .render(cx)
    }
}

pub struct BranchingParent;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BranchingParent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal("plain");
        let on_pick = rsjs::handler(|next: &'static str| {
            if next == "both" {
                selected.set("priority");
            } else {
                selected.set(next);
            }
        });

        proa_macros::html_sync! {
            <section>
                {BranchingChild { on_pick }}
                <span>{selected.get()}</span>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
