// Category 43: memo cells are read-only; `.set(...)` (and
// `.update(...)`) on a derived cell is rejected with a pointer to the
// source signals.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> { Signal { initial } }
#[allow(dead_code)]
struct Signal<T> { initial: T }
impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T { self.initial.clone() }
    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}

pub struct CartTotal;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CartTotal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price = signal(10_i64);
        let qty = signal(2_i64);
        let total = rsjs::memo(|| price.get() * qty.get());
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| total.set(0)}>"Reset"</button>
                <span>{total.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
