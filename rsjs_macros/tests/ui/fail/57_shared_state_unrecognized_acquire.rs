// An `rsjs::shared::<T>()` acquisition in a shape the analyzer cannot claim
// (here wrapped in a block) has no named store handle to bind to. Without a
// compile-time rejection the `let` would be replayed verbatim into the
// generated server render, where `rsjs::shared` is a stub that panics on
// every request.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-state-unrecognized-acquire")]
pub struct WorkbenchState {
    pub button_bg: String,
}

pub struct ThemePanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ThemePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = { rsjs::shared::<WorkbenchState>() };
        let button_bg = workbench.button_bg();
        proa_macros::html_sync! {
            <output>{button_bg.get()}</output>
        }
        .render(cx)
    }
}

fn main() {}
