// Category 47: event handlers inside keyed list bodies are rejected
// in v0.1 (client-created items have no per-item delegation scope),
// with guidance toward container-level handlers or a static template
// loop.

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
                        <li key={item}>
                            <button on_click={|_| items.set(vec![])}>{item}</button>
                        </li>
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}

fn main() {}
