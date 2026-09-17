// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Non-`signal(...)` prelude `let` bindings must be replayed verbatim into
//! the generated render body. The supported prelude contract includes shapes
//! such as `let user = cx.load::<User>(id).await?;`; before the fix the
//! analyser silently skipped any non-`signal(...)` local and the emitter never
//! replayed it.
//!
//! This test verifies behaviourally that a non-signal prelude `let`
//! actually runs at render time. We use a side-effecting helper that
//! appends to a thread-local; if the prelude let was dropped the side
//! effect would not fire.
//!
//! The companion unit test in `emit_rust::tests`
//! (`emit_rust_with_prelude_replays_extra_lets_verbatim`) verifies
//! the line appears verbatim in the emitted impl token stream.

use std::cell::RefCell;

#[allow(unused_imports)]
use proa_core::{render_to_string, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

// Parser marker replaced by the attribute macro before method resolution.
#[allow(dead_code)]
fn signal<T>(initial: T) -> T {
    initial
}

thread_local! {
    static PRELUDE_LET_RAN: RefCell<bool> = const { RefCell::new(false) };
}

/// Helper used in the prelude. The macro must replay
/// `let _user = some_helper(...);` verbatim into the emitted render
/// body so this side effect actually runs.
fn some_helper() -> i32 {
    PRELUDE_LET_RAN.with(|c| *c.borrow_mut() = true);
    42
}

pub struct WithPrelude;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for WithPrelude {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        // This let-binding is a non-`signal(...)` prelude let. The
        // analyser silently skipped it before the fix; the emitter
        // now replays it before the rsjs-managed signal locals.
        let _user = some_helper();
        // Real signal declaration: lowered into the IR.
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

#[test]
fn non_signal_prelude_let_is_replayed_into_render_body() {
    PRELUDE_LET_RAN.with(|c| *c.borrow_mut() = false);
    let _ = render_to_string(WithPrelude);
    let ran = PRELUDE_LET_RAN.with(|c| *c.borrow());
    assert!(
        ran,
        "expected the prelude `let _user = some_helper()` to run \
         during render; if this fails the analyser is silently \
         dropping non-signal prelude let bindings again"
    );
}
