// `fallback = ...` belongs to the future explicit client-only mode. It should
// not be accepted independently on normal SSR-capable rsjs islands.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ClientOnlyFallbackCounter;

#[rsjs(component, client, fallback = "empty")]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ClientOnlyFallbackCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>"hi"</div>
        }
        .render(cx)
    }
}

fn main() {}
