// Category 51: `rsjs::server_only(...)` is explicit SSR-only content
// and must not declare or read client primitives inside the boundary.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ServerOnlyClientPrimitive;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ServerOnlyClientPrimitive
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>{rsjs::server_only(rsjs::memo(|| 1_i64))}</div>
        }
        .render(cx)
    }
}

fn main() {}
