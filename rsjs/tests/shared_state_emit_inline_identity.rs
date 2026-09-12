// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Store identity must survive component inlining.
//!
//! A parent island that inlines a descriptor-backed child replays the child's
//! shared-state acquisition into its own module, where the recorded Rust type
//! name is resolved again. The store the inlined markup binds to, and the
//! `type_key` its hydration blob carries, must still be the child's, not
//! whatever the parent's scope makes of that name.

use proa_core::{Ctx, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.emit.inline-identity")]
pub struct InlineIdentityState {
    pub label: String,
}

pub struct InlineIdentityBadge;

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for InlineIdentityBadge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<InlineIdentityState>();
        let label = state.label();

        proa_macros::html_sync! {
            <output on_click={|_| label.set("clicked".to_string())}>{label.get()}</output>
        }
        .render(cx)
    }
}

pub struct InlineIdentityPage;

#[rsjs::rsjs(component)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for InlineIdentityPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>{InlineIdentityBadge {}}</section>
        }
        .render(cx)
    }
}

fn render(component: impl WebRenderSync<NoLoader, Vec<u8>>) -> String {
    let mut cx = Ctx::<NoLoader, Vec<u8>>::with_buffer(Vec::new());
    component.render(&mut cx).expect("island renders");
    String::from_utf8(cx.into_output()).expect("utf-8 html")
}

#[test]
fn an_inlined_child_binds_the_state_it_acquired() {
    let html = render(rsjs::SharedStateProvider {
        state: InlineIdentityState {
            label: "provider-value".into(),
        },
        children: InlineIdentityPage,
    });

    assert!(html.contains(">provider-value</output>"), "{html}");
    assert!(
        html.contains("\"type_key\":\"test.emit.inline-identity\""),
        "{html}"
    );
}

#[test]
fn the_child_binds_the_same_state_when_rendered_directly() {
    let html = render(rsjs::SharedStateProvider {
        state: InlineIdentityState {
            label: "direct-value".into(),
        },
        children: InlineIdentityBadge,
    });

    assert!(html.contains(">direct-value</output>"), "{html}");
    assert!(
        html.contains("\"type_key\":\"test.emit.inline-identity\""),
        "{html}"
    );
}
