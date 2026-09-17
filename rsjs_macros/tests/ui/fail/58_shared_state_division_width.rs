// A shared field enters the island macro without a proven Rust integer width.
// Representation metadata can choose Number versus BigInt at hydration, but
// cannot preserve width-specific overflow or signedness. With no explicit
// typed update boundary, rsjs rejects arithmetic instead of guessing.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-state-division-width")]
pub struct CartState {
    pub total: u64,
    pub count: u64,
}

pub struct AveragePanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for AveragePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let cart = rsjs::shared::<CartState>();
        let total = cart.total();
        let count = cart.count();
        proa_macros::html_sync! {
            <output>{total.get() / count.get()}</output>
        }
        .render(cx)
    }
}

fn main() {}
