// `rsjs::island(...)` is an explicit independent island boundary. Parent
// client primitives cannot be declared or read through the boundary expression.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct BoundaryChild<T> {
    pub value: T,
}

pub struct IslandClientPrimitive;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for IslandClientPrimitive {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>
                {rsjs::island(BoundaryChild { value: rsjs::memo(|| 1_i64) })}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
