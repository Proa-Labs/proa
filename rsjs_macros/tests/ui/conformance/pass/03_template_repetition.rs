#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct RepeatedButtons {
    pub on_select: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for RepeatedButtons {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let labels = ["Today", "Tomorrow"];
        let draft = signal("Today".to_string());

        proa_macros::html_sync! {
            <div>
                {for label in labels {
                    proa_macros::html_sync! {
                        <div>
                            <input bind_value={draft}/>
                            {if draft.get() == label.to_string() {
                                proa_macros::html_sync! {
                                    <strong>{rsjs::server_only(label)}</strong>
                                }
                            } else {
                                proa_macros::html_sync! {
                                    <span>"Inactive"</span>
                                }
                            }}
                            <button type="button" on_click={|_| {
                                let selected = label.to_string();
                                console::log(selected);
                                self.on_select.call(selected);
                            }}>
                                {rsjs::server_only(label)}
                            </button>
                        </div>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

pub struct Parent;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Parent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(String::new());
        let on_select = rsjs::handler(|next: String| selected.set(next));

        proa_macros::html_sync! {
            <section>
                {RepeatedButtons { on_select }}
                <p>{selected.get()}</p>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
