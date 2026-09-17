#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct KeyedTodoList;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for KeyedTodoList {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let items = signal(vec!["Alpha".to_string(), "Beta".to_string()]);

        proa_macros::html_sync! {
            <div>
                <button
                    type="button"
                    on_click={|_| {
                        items.set(vec![
                            "Gamma".to_string(),
                            "Beta".to_string(),
                            "Alpha".to_string(),
                        ]);
                    }}
                >
                    "Shuffle"
                </button>
                <button
                    type="button"
                    on_click={|_| {
                        items.set(vec!["Gamma".to_string(), "Alpha".to_string()]);
                    }}
                >
                    "Remove"
                </button>
                <ul>
                    {for item in items.get() {
                        proa_macros::html_sync! {
                            <li class="todo" key={item}>{item}</li>
                        }
                    }}
                </ul>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
