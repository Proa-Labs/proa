// Chaining the acquisition straight into a field accessor is rejected
// with a targeted diagnostic at the chained call. The store handle must
// be its own prelude local so field bindings resolve against a named
// acquisition.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-state-chained-acquire")]
pub struct WorkbenchState {
    pub mode: String,
}

pub struct ThemePanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ThemePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mode = rsjs::shared::<WorkbenchState>().mode();
        proa_macros::html_sync! {
            <output>{mode.get()}</output>
        }
        .render(cx)
    }
}

fn main() {}
