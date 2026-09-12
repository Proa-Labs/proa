// `#[rsjs(component, client, client_only)]` is a possible future compatibility alias, but it
// must not compile before the fallback/no-JS policy exists.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct ClientOnlyAliasCounter;

#[rsjs(component, client, client_only)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ClientOnlyAliasCounter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>"hi"</div>
        }
        .render(cx)
    }
}

fn main() {}
