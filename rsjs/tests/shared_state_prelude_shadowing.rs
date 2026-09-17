// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! C8 regression: the server replay must drop the exact `let` shared-state
//! lowering consumed, not every prelude `let` that happens to share its name.
//!
//! Both shadowing directions are covered:
//!
//! 1. A server local declared *after* a shared-field local keeps its own
//!    meaning: the template must render the server value, not the shared one.
//! 2. A server local declared *before* a shared-field local of the same name
//!    survives into the replay, so later server locals that read it still
//!    compile.

use proa_core::{Ctx, NoLoader, TwoSync, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.shared-prelude-shadowing")]
struct ShadowState {
    button_bg: String,
}

/// Direction 1: `bg` is bound to a shared field and then deliberately
/// shadowed by a value derived from the inbound `force` prop.
struct ServerOverridePanel {
    force: bool,
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerOverridePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<ShadowState>();
        let bg = workbench.button_bg();
        let bg = if self.force { "FORCED" } else { "PLAIN" };

        proa_macros::html_sync! {
            <output>{bg}</output>
        }
        .render(cx)
    }
}

/// Direction 2: an ordinary server local named `bg` is read by a later server
/// local, and only then is the name rebound to a shared field.
struct EarlyServerLocalPanel {
    title: &'static str,
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for EarlyServerLocalPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let bg = self.title;
        let label = format!("[{bg}]");
        let workbench = rsjs::shared::<ShadowState>();
        let bg = workbench.button_bg();

        proa_macros::html_sync! {
            <output>
                <span>{label}</span>
                <b>{bg.get()}</b>
            </output>
        }
        .render(cx)
    }
}

#[test]
fn a_server_local_shadowing_a_shared_field_wins_at_the_template() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: ShadowState {
            button_bg: "SHARED-GREEN".into(),
        },
        children: ServerOverridePanel { force: true },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">FORCED</output>"),
        "the server override must be replayed and rendered: {html}"
    );
    assert!(
        !html.contains("SHARED-GREEN"),
        "the shadowed shared field must not reach the template: {html}"
    );
    assert!(
        html.contains("<output data-rsjs-bind"),
        "the replacement value remains live through the inbound `force` prop: {html}"
    );
}

#[test]
fn a_server_local_shadowed_by_a_shared_field_survives_the_replay() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: ShadowState {
            button_bg: "SHARED-GREEN".into(),
        },
        children: EarlyServerLocalPanel { title: "hello" },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">[hello]</span>"),
        "the earlier server local must still be in scope for `label`: {html}"
    );
    assert!(
        html.contains(">SHARED-GREEN</b>"),
        "the later shared-field binding must still be reactive: {html}"
    );
}

#[test]
fn both_shadowing_directions_share_one_provider_scope() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: ShadowState {
            button_bg: "SHARED-GREEN".into(),
        },
        children: TwoSync {
            first: ServerOverridePanel { force: false },
            second: EarlyServerLocalPanel { title: "hi" },
        },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(html.contains(">PLAIN</output>"), "{html}");
    assert!(html.contains(">[hi]</span>"), "{html}");
    assert!(html.contains(">SHARED-GREEN</b>"), "{html}");
}
