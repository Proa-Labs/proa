// Category 52: browser helpers must stay in client-reachable IR.
// `server_only(...)` is static SSR output and does not hydrate.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ServerOnlyBrowserHelper;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B>
    for ServerOnlyBrowserHelper
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>{rsjs::server_only(rsjs::window::inner_width())}</div>
        }
        .render(cx)
    }
}

fn main() {}
