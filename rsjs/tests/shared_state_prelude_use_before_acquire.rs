// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! C9 regression: `rsjs/shared-state-use-before-acquire` must key on the
//! binding in scope at the call site, not on the set of names bound to an
//! `rsjs::shared::<T>()` acquisition anywhere in the prelude.
//!
//! `state.len()` below is `String::len` on a plain server local. The name is
//! only rebound to a shared store afterwards, so the earlier call has nothing
//! to do with shared state and must compile. The genuine forward reference
//! (a receiver that is not bound at all yet) still fails, and its golden lives
//! at `rsjs_macros/tests/ui/fail/55_shared_state_use_before_acquire.rs`.

use proa_core::{Ctx, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.shared-prelude-use-before-acquire")]
struct PanelState {
    button_bg: String,
}

struct ShadowedReceiverPanel {
    label: &'static str,
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ShadowedReceiverPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = self.label.to_string();
        let n = state.len();
        let state = rsjs::shared::<PanelState>();
        let bg = state.button_bg();

        proa_macros::html_sync! {
            <output>
                <span>{n}</span>
                <b>{bg.get()}</b>
            </output>
        }
        .render(cx)
    }
}

#[test]
fn a_zero_arg_call_on_a_plain_local_is_not_a_shared_field_read() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: PanelState {
            button_bg: "SHARED-GREEN".into(),
        },
        children: ShadowedReceiverPanel { label: "abcde" },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">5</span>"),
        "`state.len()` must stay `String::len` on the plain local: {html}"
    );
    assert!(
        html.contains(">SHARED-GREEN</b>"),
        "the later acquisition must still bind the shared field: {html}"
    );
}
