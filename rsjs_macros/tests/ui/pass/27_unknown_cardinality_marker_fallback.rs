// An arbitrary server renderable has unknown DOM cardinality. It may precede a
// browser-owned binding because the fast index walk verifies its marker and
// falls back to an owner-scoped marker lookup when the server output shifts the
// expected path.

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct Counter;

fn server_fragment() -> &'static str {
    "server fragment"
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);
        proa_macros::html_sync! {
            <main>
                {server_fragment()}
                <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
            </main>
        }
        .render(cx)
    }
}

fn main() {}
