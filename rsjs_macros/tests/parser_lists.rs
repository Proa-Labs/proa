// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Keyed list rendering end-to-end through the proc-macro pipeline.
//!
//! A template `for` loop whose iterator is a direct signal read and
//! whose body root carries `key={...}` lowers to a keyed list:
//!
//! - SSR replays the initial items through the loop, each item root
//!   carrying `data-rsjs-key` so the client can adopt the
//!   server-rendered nodes instead of re-creating them.
//! - The emitted JS binds the container with
//!   `bind_list(container, () => __sig_N.get(), { key, create, adopt,
//!   deps })`, gating the `bind_list` runtime import behind use.
//! - Handlers can write the list with `signal.set(vec![...])` over
//!   literal elements.

#[allow(unused_imports)]
use proa_core::render_to_string;
#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs_macros::rsjs;

// Local stub mirroring parser_counter.rs: the macro discards the
// original render body, but the type-checker still resolves it.
#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

#[allow(dead_code)]
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }

    pub fn set(&self, _value: T) {}

    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}

pub struct TodoList;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TodoList {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let items = signal(vec!["Alpha".to_string(), "Beta".to_string()]); // proa-perf-ok: test fixture seeds a Vec signal
        proa_macros::html_sync! {
            <div>
                <button
                    type="button"
                    on_click={|_| {
                        items.set(vec!["Gamma".to_string(), "Alpha".to_string()]); // proa-perf-ok: test fixture writes a literal list
                    }}
                >
                    "Replace"
                </button>
                <ul>
                    {for item in items.get() {
                        proa_macros::html_sync! {
                            <li class="todo" key={item}>{item}</li>
                        }
                    }}
                </ul>
            </div>
        }
        .render(cx)
    }
}

fn island_js(asset_prefix: &str) -> &'static str {
    let manifest = rsjs::manifest::manifest();
    manifest
        .islands
        .iter()
        .find(|entry| entry.asset_id.starts_with(asset_prefix))
        .unwrap_or_else(|| panic!("{asset_prefix} island should be registered"))
        .js_source
}

#[test]
fn keyed_list_ssr_renders_items_with_keys_and_container_marker() {
    let html = render_to_string(TodoList);
    assert!(
        html.contains(r#"data-rsjs-list="#),
        "the container must carry the data-rsjs-list marker, got:\n{html}"
    );
    assert!(
        html.contains(r#"<li class="todo" data-rsjs-key="Alpha">Alpha</li>"#),
        "each item root must render data-rsjs-key plus the item text, got:\n{html}"
    );
    assert!(
        html.contains(r#"<li class="todo" data-rsjs-key="Beta">Beta</li>"#),
        "every initial item must be server-rendered, got:\n{html}"
    );
    assert!(
        html.contains(r#"{"signals":[["Alpha","Beta"]]}"#),
        "the seed blob must carry the initial Vec, got:\n{html}"
    );
}

#[test]
fn keyed_list_emits_bind_list_with_key_create_adopt_and_deps() {
    let js = island_js("todolist_");
    assert!(
        js.lines().next().unwrap_or_default().contains("bind_list"),
        "bind_list must be imported from the runtime, got:\n{js}"
    );
    assert!(
        js.contains("const __list_create_0 = (item) => {"),
        "expected the imperative create builder, got:\n{js}"
    );
    assert!(
        js.contains("document.createElement(\"li\")"),
        "create must build the item root imperatively, got:\n{js}"
    );
    assert!(
        js.contains(".setAttribute(\"class\", \"todo\");"),
        "static attributes must be re-applied in create, got:\n{js}"
    );
    assert!(
        js.contains("const __list_adopt_0 = (node, item) => {"),
        "expected the SSR adoption builder, got:\n{js}"
    );
    assert!(
        js.contains("key: (item) => String(item),"),
        "expected the key callback over the loop item, got:\n{js}"
    );
    assert!(
        js.contains("create: __list_create_0,") && js.contains("adopt: __list_adopt_0,"),
        "bind_list options must wire the builders, got:\n{js}"
    );
    assert!(
        js.contains("deps: [__sig_0],"),
        "the binding must declare an explicit dep on the list signal, got:\n{js}"
    );
    assert!(
        js.contains(", () => __sig_0.get(), {"),
        "the getter must read the driving signal, got:\n{js}"
    );
}

#[test]
fn keyed_list_handler_writes_literal_vec_through_array_lit() {
    let js = island_js("todolist_");
    assert!(
        js.contains(r#"__sig_0.set(["Gamma", "Alpha"]);"#),
        "signal.set(vec![...]) must lower to a JS array literal write, got:\n{js}"
    );
}

// `ClientProps` provides the `ClientEncode` impl the dehydrated state
// blob needs for a `Vec<Row>` signal.
#[derive(Clone, rsjs::ClientProps)]
pub struct Row {
    pub id: i64,
    pub title: String,
}

pub struct RowTable;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RowTable {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let rows = signal(vec![Row {
            id: 1,
            title: "First".to_string(),
        }]); // proa-perf-ok: test fixture seeds a Vec signal
        proa_macros::html_sync! {
            <ul>
                {for row in rows.get() {
                    proa_macros::html_sync! {
                        <li key={row.id}>
                            <span class="t">{row.title}</span>
                        </li>
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}

#[test]
fn keyed_list_field_reads_ssr_render_keys_and_nested_text() {
    let html = render_to_string(RowTable);
    assert!(
        html.contains(r#"<li data-rsjs-key="1"><span class="t">First</span></li>"#),
        "struct rows must SSR with the field key and nested slot text, got:\n{html}"
    );
}

#[test]
fn keyed_list_field_reads_lower_keys_and_nested_slots() {
    let js = island_js("rowtable_");
    assert!(
        js.contains("key: (item) => String(item.id),"),
        "field keys must lower to item.<field>, got:\n{js}"
    );
    assert!(
        js.contains(".textContent = String(item.title);"),
        "nested field slots must update through the item field, got:\n{js}"
    );
    assert!(
        js.contains(".children[0];"),
        "adopt must re-resolve the nested slot element by child index, got:\n{js}"
    );
}
