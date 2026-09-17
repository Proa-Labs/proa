// Category 46: a template `for` loop over a signal read lowers to a
// keyed list, and keyed lists require `key={...}` on the item's root
// element so the client can match DOM nodes to items across inserts,
// removes, and reorders.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> { Signal { initial } }
#[allow(dead_code)]
struct Signal<T> { initial: T }
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T { self.initial.clone() }
    pub fn set(&self, _value: T) {}
}

pub struct TodoList;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TodoList {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let items = signal(vec!["Alpha".to_string()]);
        proa_macros::html_sync! {
            <ul>
                {for item in items.get() {
                    proa_macros::html_sync! {
                        <li>{item}</li>
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}

fn main() {}
