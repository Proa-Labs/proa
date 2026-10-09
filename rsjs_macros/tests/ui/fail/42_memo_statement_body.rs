// Category 42: `memo(...)` closures must be a single reactive
// expression; bodies with statements (`let` bindings, multiple
// statements) are rejected with guidance toward event handlers.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> { Signal { initial } }
#[allow(dead_code)]
struct Signal<T> { initial: T }
impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T { self.initial.clone() }
}

pub struct CartTotal;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CartTotal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price = signal(10_i64);
        let qty = signal(2_i64);
        let total = rsjs::memo(|| {
            let subtotal = price.get() * qty.get();
            subtotal + 1
        });
        proa_macros::html_sync! {
            <div><span>{total.get()}</span></div>
        }
        .render(cx)
    }
}

fn main() {}
