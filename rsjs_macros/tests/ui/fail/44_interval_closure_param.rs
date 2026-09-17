// Category 44: `rsjs::interval(...)` closures take no parameters; a
// timer tick has no event to receive, so a parameter is rejected with
// guidance toward reading signals directly.

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

pub struct UptimeTicker;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for UptimeTicker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let tick = signal(0_i64);
        rsjs::interval(1000, |event| tick.update(|t| t + 1_i64));
        proa_macros::html_sync! {
            <div>
                <span>{tick.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
