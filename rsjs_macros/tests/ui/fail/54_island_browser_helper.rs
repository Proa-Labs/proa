// Browser helpers must live inside the child island or normal parent Client IR,
// not inside the server-rendered explicit island boundary expression.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct BoundaryChild<T> {
    pub value: T,
}

pub struct IslandBrowserHelper;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for IslandBrowserHelper {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>
                {rsjs::island(BoundaryChild { value: rsjs::window::inner_width() })}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
