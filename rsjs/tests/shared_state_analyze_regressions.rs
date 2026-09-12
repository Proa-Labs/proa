// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Analyzer regressions around shared-state prelude collection.
//!
//! 1. A runtime `signal(...)` that shadows an earlier shared-field
//!    local keeps its own initializer. The initializer must bind to the
//!    id allocated for that exact declaration, never to the first
//!    same-named cell (the shared field) in declaration order.
//! 2. Foreign functions that happen to be named `shared` are user code:
//!    `features::shared()` and `ecs::shared::<Position>()` must fall
//!    through as ordinary server locals and run during render instead
//!    of being claimed by shared-state detection.

use proa_core::{Ctx, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.shared-analyze-regressions")]
struct RegressionState {
    button_bg: String,
}

struct ShadowedFieldLocal {
    custom_code: u32,
}

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ShadowedFieldLocal {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<RegressionState>();
        let bg = workbench.button_bg();
        let bg = rsjs::signal(self.custom_code);

        proa_macros::html_sync! {
            <output>{bg.get()}</output>
        }
        .render(cx)
    }
}

#[test]
fn runtime_signal_shadowing_a_shared_field_local_keeps_its_initializer() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: RegressionState {
            button_bg: "provider-green".into(),
        },
        children: ShadowedFieldLocal { custom_code: 4242 },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(
        html.contains(">4242</output>"),
        "the shadowing signal's runtime initializer must render, not a null placeholder: {html}"
    );
    assert!(
        html.contains("4242"),
        "the hydration seed must carry the runtime initializer value: {html}"
    );
}

mod features {
    use std::cell::RefCell;

    thread_local! {
        pub static SHARED_HELPER_RAN: RefCell<bool> = const { RefCell::new(false) };
    }

    /// A user helper that merely happens to be called `shared`.
    pub fn shared() -> u32 {
        SHARED_HELPER_RAN.with(|c| *c.borrow_mut() = true);
        7
    }
}

mod ecs {
    use std::cell::RefCell;

    thread_local! {
        pub static SHARED_HELPER_RAN: RefCell<bool> = const { RefCell::new(false) };
    }

    #[allow(dead_code)]
    #[derive(Default)]
    pub struct Position {
        pub x: u32,
    }

    /// A user helper with a turbofish that merely happens to be called
    /// `shared`.
    pub fn shared<T: Default>() -> T {
        SHARED_HELPER_RAN.with(|c| *c.borrow_mut() = true);
        T::default()
    }
}

struct ForeignSharedNames;

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ForeignSharedNames {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let _flags = features::shared();
        let _world = ecs::shared::<ecs::Position>();
        let count = rsjs::signal(0_i64);

        proa_macros::html_sync! {
            <output>{count.get()}</output>
        }
        .render(cx)
    }
}

#[test]
fn foreign_shared_functions_stay_ordinary_server_locals() {
    features::SHARED_HELPER_RAN.with(|c| *c.borrow_mut() = false);
    ecs::SHARED_HELPER_RAN.with(|c| *c.borrow_mut() = false);

    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    ForeignSharedNames.render(&mut cx).unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(html.contains(">0</output>"), "{html}");
    assert!(
        features::SHARED_HELPER_RAN.with(|c| *c.borrow()),
        "`features::shared()` must be replayed as an ordinary server local"
    );
    assert!(
        ecs::SHARED_HELPER_RAN.with(|c| *c.borrow()),
        "`ecs::shared::<Position>()` must be replayed as an ordinary server local"
    );
}
