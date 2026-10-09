// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{Ctx, NoLoader, TwoSync, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.shared-island-workbench")]
struct WorkbenchState {
    button_bg: String,
    button_radius: u32,
}

struct ThemePanel;

struct CanvasPreview;

struct DirectMemoPreview;

struct AliasedFieldPreview;

struct ShadowedRuntimeSignals {
    #[allow(dead_code)] // The fixture verifies that this field is shadowed safely.
    first: u32,
    second: u32,
}

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.shared-island-wide")]
struct WideState {
    count: i64,
    selected_node: Option<u64>,
}

struct WidePanel;

struct WideSetPanel;

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.shared-island-division")]
struct DivisionState {
    value: i64,
    divisor: i64,
}

struct DivisionPanel;

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for WidePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<WideState>();
        let count = state.count();
        let selected_node = state.selected_node();

        proa_macros::html_sync! {
            <button on_click={|_| rsjs::batch(|| {
                count.update(|value| value + 1_i64);
                selected_node.set(None);
            })}>
                {count.get()}
            </button>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for WideSetPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<WideState>();
        let count = state.count();

        proa_macros::html_sync! {
            <button on_click={|_| count.set(count.get() + 1_i64)}>{count.get()}</button>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DivisionPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<DivisionState>();
        let value = state.value();
        let divisor = state.divisor();

        proa_macros::html_sync! {
            <button on_click={|_| {
                value.set(rsjs::div_int::<i64>(value.get(), divisor.get()));
            }}>
                {value.get()}
            </button>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ThemePanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();
        let button_bg = workbench.button_bg();
        let button_radius = workbench.button_radius();

        proa_macros::html_sync! {
            <button on_click={|_| button_radius.update(|radius| radius + 1_u32)}>
                <span>{button_bg.get()}</span>
                <span>{button_radius.get()}</span>
            </button>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CanvasPreview {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();
        let button_bg = workbench.button_bg();

        proa_macros::html_sync! {
            <output>{button_bg.get()}</output>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DirectMemoPreview {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();
        let label = rsjs::memo(|| workbench.button_bg().get());

        proa_macros::html_sync! {
            <output on_click={|_| rsjs::batch(|| {
                workbench
                    .button_radius()
                    .update(|radius| radius + 1_u32);
                workbench.button_bg().set("clicked".to_string());
            })}>
                {label.get()}
            </output>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for AliasedFieldPreview {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();
        let first = workbench.button_radius();
        let second = workbench.button_radius();

        proa_macros::html_sync! {
            <output>
                <span>{first.get()}</span>
                <span>{second.get()}</span>
            </output>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for ShadowedRuntimeSignals {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(self.first);
        let count = rsjs::signal(self.second);

        proa_macros::html_sync! {
            <output>{count.get()}</output>
        }
        .render(cx)
    }
}

#[test]
fn island_resolves_provider_and_emits_exact_scope_binding() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WorkbenchState {
            button_bg: "#2f6b4f".into(),
            button_radius: 8,
        },
        children: ThemePanel,
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(html.contains("#2f6b4f"), "{html}");
    assert!(html.contains(">8</span>"), "{html}");
    assert!(html.contains("\"shared\":[{\"scope_id\":0"));
    assert!(html.contains("\"type_key\":\"test.shared-island-workbench\""));
    assert!(html.contains("\"schema_hash\":\"sha256:"));
}

#[test]
fn independent_islands_under_one_provider_bind_the_same_scope() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WorkbenchState {
            button_bg: "green".into(),
            button_radius: 8,
        },
        children: TwoSync {
            first: ThemePanel,
            second: CanvasPreview,
        },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert_eq!(html.matches("\"scope_id\":0").count(), 2, "{html}");
    assert!(html.contains("data-rsjs-island-root=\"themepanel_"));
    assert!(html.contains("data-rsjs-island-root=\"canvaspreview_"));
}

#[test]
fn nested_provider_resolves_to_a_distinct_nearest_scope() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WorkbenchState {
            button_bg: "outer".into(),
            button_radius: 8,
        },
        children: TwoSync {
            first: CanvasPreview,
            second: rsjs::SharedStateProvider {
                state: WorkbenchState {
                    button_bg: "inner".into(),
                    button_radius: 12,
                },
                children: CanvasPreview,
            },
        },
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert_eq!(html.matches("\"scope_id\":0").count(), 1, "{html}");
    assert_eq!(html.matches("\"scope_id\":1").count(), 1, "{html}");
    assert!(html.contains(">outer</output>"), "{html}");
    assert!(html.contains(">inner</output>"), "{html}");
}

#[test]
fn direct_field_chains_work_in_memos_and_handlers() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WorkbenchState {
            button_bg: "memo-green".into(),
            button_radius: 8,
        },
        children: DirectMemoPreview,
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(html.contains(">memo-green</output>"), "{html}");
    assert!(html.contains("\"shared\":[{\"scope_id\":0"), "{html}");
}

#[test]
fn repeated_locals_for_one_shared_field_reuse_the_cell() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WorkbenchState {
            button_bg: "green".into(),
            button_radius: 8,
        },
        children: AliasedFieldPreview,
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert_eq!(html.matches(">8</span>").count(), 2, "{html}");
}

#[test]
fn shadowed_runtime_signals_keep_their_declaration_identity() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    ShadowedRuntimeSignals {
        first: 111,
        second: 222,
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();

    assert!(html.contains(">222</output>"), "{html}");
    assert!(html.contains("\"signals\":[null,222]"), "{html}");
}

#[test]
fn missing_provider_is_a_targeted_render_error() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    let error = ThemePanel
        .render(&mut cx)
        .expect_err("a shared-state consumer without a provider must fail");
    let message = error.to_string();
    assert!(message.contains("rsjs/shared-state-missing-provider"));
    assert!(message.contains("themepanel"));
    assert!(message.contains("SharedStateProvider"));
}

/// Regression (C1/C3/C5/C6/C12/C14/C15/C20/C21/C22/C23/C28/C32): an explicit
/// result suffix on a shared-field update supplies an exact arithmetic domain.
///
/// The macro cannot resolve `WideState::count` from the field-accessor tokens,
/// but Rust type-checks `value + 1_i64` against `SharedField<i64>::update`.
/// That boundary lets the emitted client operation preserve i64 overflow and
/// BigInt semantics without any build-order-dependent type registry.
#[test]
fn shared_field_update_uses_its_explicit_i64_domain() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WideState {
            count: 9_007_199_254_740_993,
            selected_node: Some(7),
        },
        children: WidePanel,
    }
    .render(&mut cx)
    .unwrap();
    let html = String::from_utf8(out).unwrap();
    assert!(html.contains(">9007199254740993</button>"), "{html}");

    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "WidePanel")
        .expect("WidePanel island registered");
    assert!(
        island
            .js_source
            .contains("checked_int_add(__sig_0.get(), 1n, \"i64\")"),
        "shared-field update must retain checked i64 arithmetic: {}",
        island.js_source
    );
    assert!(
        !island.js_source.contains("num_add(__sig_0.get(), 1)")
            && !island.js_source.contains(" + 1n"),
        "the compiler must not fall back to dynamic or bare arithmetic: {}",
        island.js_source
    );
    // A helper that is emitted but not imported is a `ReferenceError` at
    // hydration with no compile-time signal, so the import line is part of the
    // contract, not an implementation detail.
    let import_line = island
        .js_source
        .lines()
        .next()
        .expect("emitted island JS starts with its runtime import line");
    assert!(
        import_line.contains("checked_int_add"),
        "the emitted checked helper must be imported: {import_line}"
    );
    assert!(
        island.js_source.contains(".set(null)"),
        "Option::None must lower to client null: {}",
        island.js_source
    );
}

/// A direct shared-field write must establish its domain before the value is
/// lowered. Otherwise the shared read remains `Dyn`, and `MAX + 1_i64` would
/// become representation-only arithmetic instead of a checked Rust i64 add.
#[test]
fn shared_field_set_uses_checked_i64_arithmetic() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: WideState {
            count: i64::MAX,
            selected_node: None,
        },
        children: WideSetPanel,
    }
    .render(&mut cx)
    .unwrap();

    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "WideSetPanel")
        .expect("WideSetPanel island registered");
    assert!(
        island
            .js_source
            .contains("__sig_0.set(checked_int_add(__sig_0.get(), 1n, \"i64\"));"),
        "direct shared set must use checked i64 arithmetic so MAX + 1 traps: {}",
        island.js_source
    );
    assert!(
        island
            .js_source
            .lines()
            .next()
            .is_some_and(|line| line.contains("checked_int_add")),
        "the direct-set overflow helper must be imported: {}",
        island.js_source
    );
}

#[test]
fn explicit_shared_div_int_emits_checked_i64_division_without_legacy_fallback() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    rsjs::SharedStateProvider {
        state: DivisionState {
            value: i64::MIN,
            divisor: -1,
        },
        children: DivisionPanel,
    }
    .render(&mut cx)
    .unwrap();

    let manifest = rsjs::manifest();
    let island = manifest
        .islands
        .iter()
        .find(|entry| entry.struct_name == "DivisionPanel")
        .expect("DivisionPanel island registered");
    assert!(
        island
            .js_source
            .contains("__sig_0.set(checked_int_div(__sig_0.get(), __sig_1.get(), \"i64\"));"),
        "explicit shared i64 division must lower to the checked helper: {}",
        island.js_source
    );
    assert!(
        !island.js_source.contains("num_div_int"),
        "accepted generated source must not retain domainless integer division: {}",
        island.js_source
    );
    assert!(
        island
            .js_source
            .lines()
            .next()
            .is_some_and(|line| line.contains("checked_int_div")),
        "checked shared division helper must be imported: {}",
        island.js_source
    );
}
