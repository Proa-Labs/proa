// Category 45: the `delay_ms` argument of `rsjs::debounce(...)` (and
// `rsjs::interval(...)`) must be an integer literal; the window is
// baked into the island module at compile time, so runtime
// expressions are rejected.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> { Signal { initial } }
#[allow(dead_code)]
struct Signal<T> { initial: T }
impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T { self.initial.clone() }
    pub fn set(&self, _value: T) {}
}

const SEARCH_DEBOUNCE_MS: u32 = 300;

pub struct DebouncedSearch;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DebouncedSearch {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal(String::new());
        proa_macros::html_sync! {
            <div>
                <input on_input={rsjs::debounce(|event| query.set(event.value()), SEARCH_DEBOUNCE_MS)}/>
                <span>{query.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
