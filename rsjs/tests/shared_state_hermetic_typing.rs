// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Regressions for the shared-state descriptor removal.
//!
//! Shared-field typing used to resolve a JSON descriptor sidecar under
//! `target/rsjs/shared_state/`, keyed on the unqualified struct name and
//! written by `#[derive(SharedState)]` at expansion time. Because proc macros
//! expand in source order and nothing ever garbage-collected the sidecars, that
//! made the *meaning of a program* depend on declaration order, on which crates
//! cargo happened to re-expand, and on the contents of `target/`:
//!
//! - C1 / C6 / C12: a clean build failed whenever the island expanded before
//!   the derive, and the immediate rebuild succeeded because the failed build
//!   had written the sidecar.
//! - C3 / C5 / C14 / C15 / C21 / C32: the namespace was the unqualified struct
//!   name, workspace-global and unpartitioned, so same-named types in different
//!   modules or different crates collided.
//! - C20 / C22 / C28: sidecars were never invalidated, so editing a field broke
//!   every consumer until `cargo clean`.
//! - C23: a crate with a `build.rs` published into a private `OUT_DIR` nothing
//!   scanned.
//!
//! The fix is to stop asking. A shared field types as `ClientType::Dyn` and its
//! arithmetic lowers to `num_*` helpers that dispatch on the representation the
//! page seed pinned. This file locks the observable consequences: every shape
//! below compiles from any declaration order, and the emitted JS is a function
//! of the island source alone.
//!
//! The cross-crate half of the matrix (a defining crate with a `build.rs`, a
//! bare `use` of another crate's state, a same-named type exported by an
//! unrelated crate) cannot be expressed inside one crate; it is covered by the
//! probe workspace described in this change's notes.

use proa_core::render::{WebContext, WebRenderSync};
use proa_core::shared::write_buf::{WriteBuf, WriteError};
use proa_core::{DataLoader, NoLoader};

type Ctx<B, L> = WebContext<L, B>;

// ---------------------------------------------------------------------------
// C1 / C6: the island is declared textually ABOVE the state it consumes, and
// above the nested `ClientProps` struct that state contains. Under the
// descriptor design this file did not compile on a clean `target/`.
// ---------------------------------------------------------------------------

pub struct LatePanel;

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for LatePanel {
    fn render(self, cx: &mut Ctx<B, L>) -> Result<(), WriteError> {
        let cart = rsjs::shared::<LateState>();
        let count = cart.count();
        let label = cart.label();
        proa_macros::html_sync! {
            <div>
                <output>{count.get()}</output>
                <span>{label.get()}</span>
                <button on_click={|_| count.update(|c| c + 1_u64)}>"inc"</button>
            </div>
        }
        .render(cx)
    }
}

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.hermetic.late")]
pub struct LateState {
    pub count: u64,
    pub label: String,
    pub row: LateRow,
}

// C15: the nested props struct is declared after the state that embeds it.
#[derive(Clone, Debug, PartialEq, rsjs::ClientProps)]
pub struct LateRow {
    pub id: u64,
}

// ---------------------------------------------------------------------------
// C3 / C5 / C21: same-named types in different modules of one crate. Under the
// descriptor design the second `Row` evicted the first from the registry and
// the island picked up whichever expanded last; a name collision between a
// plain `ClientProps` struct and a `SharedState` struct was an unfixable
// `rsjs/shared-state-schema-ambiguous`.
// ---------------------------------------------------------------------------

pub mod feed {
    #[derive(Clone, Debug, PartialEq, rsjs::ClientProps)]
    pub struct Row {
        pub id: u32,
    }
}

pub mod checkout {
    #[derive(Clone, Debug, PartialEq, rsjs::ClientProps)]
    pub struct Row {
        pub id: u64,
    }
}

// C14 / C21: an unrelated `ClientProps` struct sharing the state's name.
pub mod forms {
    #[derive(Clone, Debug, PartialEq, rsjs::ClientProps)]
    pub struct LateState {
        pub theme: String,
    }
}

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.hermetic.feed")]
pub struct FeedState {
    pub rows: Vec<feed::Row>,
    pub cart_rows: Vec<checkout::Row>,
    pub total: u64,
}

pub struct CollisionPanel;

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for CollisionPanel {
    fn render(self, cx: &mut Ctx<B, L>) -> Result<(), WriteError> {
        let feed = rsjs::shared::<FeedState>();
        let total = feed.total();
        proa_macros::html_sync! {
            <output>{total.get()}</output>
        }
        .render(cx)
    }
}

// ---------------------------------------------------------------------------
// Emitted-JS contract.
// ---------------------------------------------------------------------------

fn island_js(struct_name: &str) -> String {
    rsjs::manifest()
        .islands
        .iter()
        .find(|entry| entry.struct_name == struct_name)
        .unwrap_or_else(|| panic!("{struct_name} island registered"))
        .js_source
        .to_string()
}

/// C1 / C6: an island that expands before the derive of the state it consumes
/// still gets exact arithmetic from the explicit update-result suffix. Rust
/// checks that `1_u64` boundary against the real shared field type after macro
/// expansion, so the client can use the ordinary checked u64 path without an
/// out-of-band type registry.
#[test]
fn an_island_declared_above_its_state_uses_its_explicit_update_domain() {
    let js = island_js("LatePanel");
    assert!(
        js.contains("checked_int_add(__sig_0.get(), 1n, \"u64\")"),
        "the u64 update boundary must retain checked fixed-width arithmetic: {js}"
    );
    assert!(
        !js.contains("num_add(__sig_0.get(), 1)")
            && !js.contains(" + 1n")
            && !js.contains("(__sig_0.get() + 1)"),
        "the explicit domain must not fall back to dynamic or bare arithmetic: {js}"
    );
}

/// Every `num_*` helper the body calls must appear in the runtime import line.
/// A missing import is a `ReferenceError` at hydration with no compile-time
/// signal, which is why the import gate and the emitter share one reachability
/// walk (`for_each_reachable_client_root`).
#[test]
fn every_emitted_dyn_helper_is_imported() {
    for struct_name in ["LatePanel", "CollisionPanel"] {
        let js = island_js(struct_name);
        let import_line = js
            .lines()
            .next()
            .expect("emitted JS opens with its imports");
        let mut body = js.lines().skip(1).collect::<Vec<_>>().join("\n");
        // `num_` can only appear as a helper call in the body.
        for helper in [
            "num_add",
            "num_sub",
            "num_mul",
            "num_div_int",
            "num_div",
            "num_rem",
            "num_eq",
            "num_ne",
        ] {
            if body.contains(helper) {
                assert!(
                    import_line.contains(helper),
                    "{struct_name} calls {helper} without importing it: {import_line}"
                );
                body = body.replace(helper, "");
            }
        }
    }
}

/// C3 / C5 / C14 / C15 / C21 / C32: same-named `ClientProps` and `SharedState`
/// structs in different modules coexist, and the island that reads one of them
/// still compiles. Under the descriptor design this crate did not build.
#[test]
fn same_named_types_in_different_modules_do_not_collide() {
    let js = island_js("CollisionPanel");
    assert!(js.contains("acquire_shared_state"), "{js}");

    // The wide-path table is derived from the real `ClientPropsMeta` by Rust
    // name resolution, so it distinguishes the two `Row` types even though
    // their names are identical.
    let wide = <FeedState as rsjs::SharedState>::__rsjs_wide_paths_json();
    assert!(
        wide.contains(r#"["cart_rows","*","id"]"#),
        "checkout::Row.id is u64 and must be wide: {wide}"
    );
    assert!(
        !wide.contains(r#"["rows","*","id"]"#),
        "feed::Row.id is u32 and must not be wide: {wide}"
    );
    assert!(wide.contains(r#"["total"]"#), "{wide}");
}

/// The runtime, not the compiler, is the authority on width. This is the
/// channel that survived: it is a *type-checked reference* to the derive's
/// `__rsjs_codec_meta_<Ident>` rather than a lookup by name.
#[test]
fn wide_paths_come_from_the_real_codec_metadata() {
    let wide = <LateState as rsjs::SharedState>::__rsjs_wide_paths_json();
    assert!(wide.contains(r#"["count"]"#), "{wide}");
    assert!(
        wide.contains(r#"["row","id"]"#),
        "nested widths resolve: {wide}"
    );
    assert!(
        !wide.contains("label"),
        "a String field is not wide: {wide}"
    );
}

/// The SSR replay borrows string-like shared fields instead of cloning them.
/// Borrow-versus-value used to be decided from the descriptor's `ClientType`;
/// it is now decided by `#[derive(SharedState)]`, which has the real Rust type,
/// and emitted as a `__rsjs_replay_<field>()` accessor that rustc type-checks.
#[test]
fn a_string_shared_field_replays_by_reference() {
    let mut out = Vec::new();
    let mut cx = Ctx::<&mut Vec<u8>, NoLoader>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: LateState {
            count: 9_007_199_254_740_993,
            label: "checkout".to_string(),
            row: LateRow { id: 12 },
        },
        children: LatePanel,
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();
    assert!(html.contains(">9007199254740993<"), "{html}");
    assert!(html.contains(">checkout<"), "{html}");
}
