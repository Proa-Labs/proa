// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end coverage for sibling template conditionals under one parent.

use proa_core::render_to_string;
#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

#[allow(dead_code)]
impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }
}

pub struct SiblingConditionals;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for SiblingConditionals {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let a = signal(true);
        let b = signal(false);
        proa_macros::html_sync! {
            <div>
                {if a.get() {
                    proa_macros::html_sync! { <span>"A"</span> }
                } else {
                    proa_macros::html_sync! { <em>"X"</em> }
                }}
                {if b.get() {
                    proa_macros::html_sync! { <strong>"B"</strong> }
                } else {
                    proa_macros::html_sync! { <i>"Y"</i> }
                }}
            </div>
        }
        .render(cx)
    }
}

#[test]
fn sibling_conditionals_share_one_parent() {
    let html = render_to_string(SiblingConditionals);
    assert!(
        html.contains(r#"<span data-rsjs-cond-then="0:0">A</span>"#),
        "first then branch should use the first marker slot, got:\n{html}"
    );
    assert!(
        html.contains(r#"<em data-rsjs-cond-else="0:0" style="display:none">X</em>"#),
        "first else branch should use the first marker slot, got:\n{html}"
    );
    assert!(
        html.contains(r#"<strong data-rsjs-cond-then="0:1" style="display:none">B</strong>"#),
        "second then branch should use the second marker slot, got:\n{html}"
    );
    assert!(
        html.contains(r#"<i data-rsjs-cond-else="0:1">Y</i>"#),
        "second else branch should use the second marker slot, got:\n{html}"
    );

    let manifest = rsjs::manifest::manifest();
    let js = manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with("siblingconditionals_"))
        .expect("SiblingConditionals island should be registered")
        .js_source;
    assert_eq!(
        js.matches("bind_conditional(").count(),
        2,
        "both sibling conditionals should emit runtime bindings, got:\n{js}"
    );
    assert!(
        js.contains(r#"walk_path(root, [0, 0], "data-rsjs-cond-then", "0:0")"#)
            && js.contains(r#"walk_path(root, [0, 2], "data-rsjs-cond-then", "0:1")"#),
        "conditional branch markers should be cumulatively indexed, got:\n{js}"
    );
}
