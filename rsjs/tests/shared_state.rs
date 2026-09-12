// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use proa_core::{
    Ctx, DataLoader, NoLoader, RenderOutcome, TwoSync, WebContext, WebRender, WebRenderSync,
    WriteBuf, WriteError,
};
use rsjs::{ClientDecode, ClientEncode, SharedState as _, SharedStateProvider};

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.workbench")]
pub struct WorkbenchState {
    pub select_mode: bool,
    pub button_bg: String,
    pub button_radius: u32,
}

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.workbench")]
pub struct ConflictingWorkbenchState {
    pub button_bg: String,
}

mod nested_v1 {
    #[derive(Clone, rsjs::ClientProps)]
    pub struct Settings {
        pub tone: String,
    }

    #[derive(rsjs::SharedState)]
    #[rsjs(type_key = "test.nested-schema")]
    pub struct State {
        pub settings: Settings,
    }
}

mod nested_v2 {
    #[derive(Clone, rsjs::ClientProps)]
    pub struct Settings {
        pub tone: String,
        pub radius: u32,
    }

    #[derive(rsjs::SharedState)]
    #[rsjs(type_key = "test.nested-schema")]
    pub struct State {
        pub settings: Settings,
    }
}

mod nested_renamed {
    #[derive(Clone, rsjs::ClientProps)]
    pub struct RenamedSettings {
        pub tone: String,
    }

    #[derive(rsjs::SharedState)]
    #[rsjs(type_key = "test.nested-schema")]
    pub struct State {
        pub settings: RenamedSettings,
    }
}

#[allow(dead_code)]
mod enum_schema_v1 {
    #[derive(Clone, Debug, PartialEq, rsjs::ClientProps)]
    pub enum Mode {
        View,
        Edit,
    }

    #[derive(Debug, PartialEq, rsjs::SharedState)]
    #[rsjs(type_key = "test.enum-schema")]
    pub struct State {
        pub mode: Mode,
    }
}

#[allow(dead_code)]
mod enum_schema_v2 {
    #[derive(Clone, Debug, PartialEq, rsjs::ClientProps)]
    pub enum Mode {
        View,
        Edit,
        Review,
    }

    #[derive(Debug, PartialEq, rsjs::SharedState)]
    #[rsjs(type_key = "test.enum-schema")]
    pub struct State {
        pub mode: Mode,
    }
}

#[test]
fn derive_emits_codec_schema_and_typed_field_handles() {
    let state = WorkbenchState {
        select_mode: true,
        button_bg: "#2f6b4f".into(),
        button_radius: 8,
    };
    let encoded = state.encode();
    assert_eq!(WorkbenchState::decode(&encoded).unwrap(), state);

    let meta = WorkbenchState::META;
    assert_eq!(meta.abi_version, rsjs::SHARED_STATE_ABI_VERSION);
    assert_eq!(meta.type_key, "test.workbench");
    assert!(meta.schema_hash().starts_with("sha256:"));
    assert_eq!(meta.fields.len(), 3);
    assert_eq!(meta.fields[0].name, "select_mode");
    assert_eq!(meta.fields[1].name, "button_bg");
    assert_eq!(meta.fields[2].name, "button_radius");

    let handle = WorkbenchState::__rsjs_handle_from_server(Rc::new(state));
    assert_eq!(handle.button_bg().get(), "#2f6b4f");
    assert_eq!(handle.button_radius().get(), 8);
}

#[test]
fn schema_hash_includes_nested_client_codec_shape() {
    assert_ne!(
        nested_v1::State::schema_hash(),
        nested_v2::State::schema_hash(),
        "changing a nested codec must invalidate the outer shared-state ABI"
    );
}

#[test]
fn schema_hash_excludes_non_protocol_nested_rust_names() {
    assert_eq!(
        nested_v1::State::schema_hash(),
        nested_renamed::State::schema_hash(),
        "renaming a nested Rust type must not change its wire-schema hash"
    );
}

#[test]
fn schema_hash_includes_fieldless_enum_variant_tags() {
    assert_ne!(
        enum_schema_v1::State::schema_hash(),
        enum_schema_v2::State::schema_hash(),
        "adding a fieldless-enum tag must invalidate the outer shared-state ABI"
    );

    let state = enum_schema_v1::State {
        mode: enum_schema_v1::Mode::Edit,
    };
    let wire = state.encode();
    assert_eq!(wire, r#"{"mode":"Edit"}"#);
    assert_eq!(enum_schema_v1::State::decode(&wire).unwrap(), state);
}

#[test]
fn schema_hash_distinguishes_empty_object_and_tagged_string_shapes() {
    let object = rsjs::ClientPropsMeta {
        name: "Shape",
        shape: rsjs::ClientPropsShape::Object,
        fields: &[],
    };
    let tagged = rsjs::ClientPropsMeta {
        name: "Shape",
        shape: rsjs::ClientPropsShape::TaggedString { variants: &[] },
        fields: &[],
    };

    assert_ne!(
        rsjs::__shared_state_schema_hash("test.shape", &object),
        rsjs::__shared_state_schema_hash("test.shape", &tagged),
        "the top-level wire-shape discriminator must participate in the ABI hash"
    );
}

type SeenScopes = Rc<RefCell<Vec<(&'static str, String, u64)>>>;

struct Probe {
    label: &'static str,
    seen: SeenScopes,
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Probe {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let (state, scope) = cx.rsjs_shared_state_acquire::<WorkbenchState>(
            WorkbenchState::TYPE_KEY,
            WorkbenchState::schema_hash(),
            WorkbenchState::META.rust_type,
            "SharedStateProbe",
        )?;
        self.seen
            .borrow_mut()
            .push((self.label, state.button_bg.clone(), scope));
        Ok(())
    }
}

struct AsyncProbe;

impl<B: WriteBuf, L: DataLoader> WebRender<L, B> for AsyncProbe {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            let (state, _) = cx.rsjs_shared_state_acquire::<WorkbenchState>(
                WorkbenchState::TYPE_KEY,
                WorkbenchState::schema_hash(),
                WorkbenchState::META.rust_type,
                "AsyncProbe",
            )?;
            WebRenderSync::render(
                proa_macros::html_sync! {
                    <output>{state.button_bg.as_str()}</output>
                },
                cx,
            )
        })
    }
}

fn state(color: &str) -> WorkbenchState {
    WorkbenchState {
        select_mode: true,
        button_bg: color.into(),
        button_radius: 8,
    }
}

// F2: wide-int fields travel as JSON strings, so the derive must record
// recursive codec paths and the page seed table must forward them as
// `wide_paths`; the browser runtime uses those paths to decode BigInt values.

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.cart")]
pub struct CartState {
    pub count: usize,
    pub total_cents: i64,
    pub coupon: Option<u64>,
    pub label: String,
}

#[derive(Debug, PartialEq, rsjs::ClientProps)]
pub struct LineItem {
    pub cents: i64,
    pub quantity: u32,
}

#[derive(Debug, PartialEq, rsjs::ClientProps)]
pub struct CartSummary {
    pub outstanding_cents: Option<i64>,
}

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.nested-cart")]
pub struct NestedCartState {
    pub items: Vec<LineItem>,
    pub summary: CartSummary,
}

struct CartSeedProbe;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for CartSeedProbe {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.rsjs_shared_state_acquire::<CartState>(
            CartState::TYPE_KEY,
            CartState::schema_hash(),
            CartState::META.rust_type,
            "CartSeedProbe",
        )?;
        Ok(())
    }
}

struct WorkbenchSeedProbe;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for WorkbenchSeedProbe {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        cx.rsjs_shared_state_acquire::<WorkbenchState>(
            WorkbenchState::TYPE_KEY,
            WorkbenchState::schema_hash(),
            WorkbenchState::META.rust_type,
            "WorkbenchSeedProbe",
        )?;
        Ok(())
    }
}

#[test]
fn recursive_wide_paths_cover_vectors_and_nested_client_props() {
    assert_eq!(
        NestedCartState::__rsjs_wide_paths_json(),
        r#"[["items","*","cents"],["summary","outstanding_cents"]]"#,
    );
}

#[test]
fn wide_int_fields_reach_the_page_seed_table() {
    assert_eq!(
        CartState::__rsjs_wide_paths_json(),
        r#"[["count"],["total_cents"],["coupon"]]"#,
        "derive must record every wide-int path, including Option forms"
    );
    assert_eq!(
        WorkbenchState::__rsjs_wide_paths_json(),
        "[]",
        "schemas without wide-int fields must have no wide paths"
    );

    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    TwoSync {
        first: SharedStateProvider {
            state: CartState {
                count: 8,
                total_cents: 12_050,
                coupon: None,
                label: "summer".into(),
            },
            children: CartSeedProbe,
        },
        second: SharedStateProvider {
            state: state("green"),
            children: WorkbenchSeedProbe,
        },
    }
    .render(&mut cx)
    .unwrap();

    let table = cx.rsjs_shared_state_seed_table_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&table).unwrap();
    let stores = parsed["stores"].as_array().unwrap();
    let cart = stores
        .iter()
        .find(|store| store["type_key"] == "test.cart")
        .expect("cart store present");
    assert_eq!(
        cart["wide_paths"],
        serde_json::json!([["count"], ["total_cents"], ["coupon"]]),
        "seed table must carry recursive wide-int paths: {table}"
    );
    assert_eq!(
        cart["state"]["count"],
        serde_json::Value::String("8".to_owned()),
        "wide-int wire form is a JSON string: {table}"
    );
    let workbench = stores
        .iter()
        .find(|store| store["type_key"] == "test.workbench")
        .expect("workbench store present");
    assert!(
        workbench.get("wide_paths").is_none(),
        "stores without wide-int fields must omit wide_paths: {table}"
    );
}

#[test]
fn nested_provider_uses_nearest_scope_and_restores_parent() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);

    SharedStateProvider {
        state: state("outer"),
        children: TwoSync {
            first: Probe {
                label: "before",
                seen: Rc::clone(&seen),
            },
            second: TwoSync {
                first: SharedStateProvider {
                    state: state("inner"),
                    children: Probe {
                        label: "inner",
                        seen: Rc::clone(&seen),
                    },
                },
                second: Probe {
                    label: "after",
                    seen: Rc::clone(&seen),
                },
            },
        },
    }
    .render(&mut cx)
    .unwrap();

    let seen = seen.borrow();
    assert_eq!(seen[0].0, "before");
    assert_eq!(seen[0].1, "outer");
    assert_eq!(seen[1].0, "inner");
    assert_eq!(seen[1].1, "inner");
    assert_ne!(seen[0].2, seen[1].2);
    assert_eq!(seen[2].0, "after");
    assert_eq!(seen[2].1, "outer");
    assert_eq!(seen[0].2, seen[2].2);

    let table = cx.rsjs_shared_state_seed_table_json().unwrap();
    assert!(table.contains("\"scope_id\":0"));
    assert!(table.contains("\"scope_id\":1"));
    assert!(table.contains("\"button_bg\":\"outer\""));
    assert!(table.contains("\"button_bg\":\"inner\""));
}

#[test]
fn sibling_providers_of_one_schema_have_independent_scopes() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);

    TwoSync {
        first: SharedStateProvider {
            state: state("left"),
            children: Probe {
                label: "left",
                seen: Rc::clone(&seen),
            },
        },
        second: SharedStateProvider {
            state: state("right"),
            children: Probe {
                label: "right",
                seen: Rc::clone(&seen),
            },
        },
    }
    .render(&mut cx)
    .unwrap();

    let seen = seen.borrow();
    assert_eq!(seen[0].0, "left");
    assert_eq!(seen[0].1, "left");
    assert_eq!(seen[1].0, "right");
    assert_eq!(seen[1].1, "right");
    assert_ne!(seen[0].2, seen[1].2);
}

#[tokio::test]
async fn async_provider_keeps_scope_active_across_yield() {
    let html = proa_core::render_to_string_async(SharedStateProvider {
        state: state("async-green"),
        children: AsyncProbe,
    })
    .await;

    assert_eq!(html, "<output>async-green</output>");
}

#[test]
fn missing_provider_error_names_island_type_key_and_fix() {
    let mut out = Vec::new();
    let cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    let error = cx
        .rsjs_shared_state_acquire::<WorkbenchState>(
            WorkbenchState::TYPE_KEY,
            WorkbenchState::schema_hash(),
            WorkbenchState::META.rust_type,
            "ThemePanel",
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("rsjs/shared-state-missing-provider"));
    assert!(message.contains("ThemePanel"));
    assert!(message.contains("test.workbench"));
    assert!(message.contains("SharedStateProvider"));
}

#[test]
fn conflicting_type_key_fails_before_rendering_children() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    let outer = Rc::new(state("outer"));
    let _outer_scope = cx
        .rsjs_shared_state_enter(
            WorkbenchState::TYPE_KEY,
            WorkbenchState::schema_hash(),
            WorkbenchState::META.rust_type,
            WorkbenchState::__rsjs_wide_paths_json(),
            outer.clone(),
            outer.encode(),
        )
        .unwrap();
    let conflicting = Rc::new(ConflictingWorkbenchState {
        button_bg: "conflict".into(),
    });
    let error = cx
        .rsjs_shared_state_enter(
            ConflictingWorkbenchState::TYPE_KEY,
            ConflictingWorkbenchState::schema_hash(),
            ConflictingWorkbenchState::META.rust_type,
            ConflictingWorkbenchState::__rsjs_wide_paths_json(),
            conflicting.clone(),
            conflicting.encode(),
        )
        .expect_err("conflicting protocol keys must fail");
    let message = error.to_string();
    assert!(message.contains("rsjs/shared-state-type-key-conflict"));
    assert!(message.contains("test.workbench"));
    assert!(message.contains("choose a unique"));
}

#[test]
fn provider_created_after_seed_publication_has_a_streaming_diagnostic() {
    let mut out = Vec::new();
    let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
    {
        let initial = state("shell");
        let encoded = initial.encode();
        let _scope = cx
            .rsjs_shared_state_enter(
                WorkbenchState::TYPE_KEY,
                WorkbenchState::schema_hash(),
                WorkbenchState::META.rust_type,
                WorkbenchState::__rsjs_wide_paths_json(),
                Rc::new(initial),
                encoded,
            )
            .unwrap();
        cx.rsjs_shared_state_acquire::<WorkbenchState>(
            WorkbenchState::TYPE_KEY,
            WorkbenchState::schema_hash(),
            WorkbenchState::META.rust_type,
            "ShellConsumer",
        )
        .unwrap();
    }
    assert!(cx.rsjs_shared_state_seed_table_json().is_some());

    let late = state("late");
    let encoded = late.encode();
    let error = match cx.rsjs_shared_state_enter(
        WorkbenchState::TYPE_KEY,
        WorkbenchState::schema_hash(),
        WorkbenchState::META.rust_type,
        WorkbenchState::__rsjs_wide_paths_json(),
        Rc::new(late),
        encoded,
    ) {
        Ok(_) => panic!("a provider created after publication must fail"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(message.contains("rsjs/shared-state-late-stream-provider"));
    assert!(message.contains("above the deferred/streaming boundary"));
}
