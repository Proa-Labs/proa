// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! M7h: trybuild diagnostic golden suite.
//!
//! Each fixture under `tests/ui/fail/` is a standalone Rust file that
//! is expected to fail compilation. The matching `<fixture>.stderr`
//! file is a recorded snapshot of the compiler's error output. Changes
//! to the user-facing message surface require an intentional update to
//! the recorded golden (re-run with `TRYBUILD=overwrite cargo test
//! -p rsjs_macros --test ui`).
//!
//! The fixtures cover these twelve diagnostic categories:
//!
//! 1. Wrong trait — `#[rsjs(component, client)]` on a non-`WebRender(Sync)` impl.
//!    Trait aliases are accepted because the generated hidden witness lets
//!    rustc resolve the actual trait identity.
//! 2. Method shape — missing or duplicated `render` method.
//! 3. Tail expression — render body whose tail isn't a chained
//!    `html(_sync)!` invocation.
//! 4. Sync vs async confusion — covered structurally by the macro
//!    detecting `.await` and selecting `WebRender`; the v0.1 surface
//!    has no "explicit sync, then `.await`" rejection beyond what
//!    rustc's borrow-checker catches, so the fixture exercises a
//!    related shape: missing prelude declaration before the awaited
//!    expression.
//! 5. Unknown signal — reactive expression naming an undeclared
//!    signal.
//! 6. Unsupported method call — `.frobnicate()` on a signal.
//! 7. Unsupported expression shape — closure embedded in a reactive
//!    expression.
//! 8. Multi `bind_*` on one node.
//! 9. Unknown event handler — unsupported `on_*` attributes fail
//!    before they can be mistaken for ordinary dynamic attributes.
//! 10. `#[derive(ClientProps)]` rejections — tuple struct, data-carrying
//!     enum, union, nested `Option<Option<T>>`.
//!
//! Missing `#[rsjs::client]` definitions are intentionally not proc-macro
//! compile-fail cases. Rust resolves helper paths and aliases during descriptor
//! compilation; the linker fixtures under `tests/ui/linker/` cover the later
//! unresolved-edge diagnostic.

mod support;

#[test]
fn ui() {
    support::configure_trybuild_registry();
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    t.compile_fail("tests/ui/fail/*.rs");
}
