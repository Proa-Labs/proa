// Shared-state fields obey ordinary Rust lexical ordering. An acquisition
// declared later in the prelude is not visible to an earlier field binding.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.shared-state-use-before")]
pub struct WorkbenchState {
    pub button_bg: String,
}

pub struct ThemePanel;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ThemePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let button_bg = workbench.button_bg();
        let workbench = rsjs::shared::<WorkbenchState>();
        proa_macros::html_sync! {
            <output>{button_bg.get()}</output>
        }
        .render(cx)
    }
}

fn main() {}
