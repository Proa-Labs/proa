// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Keyed-list emission: a hand-built island with one [`ListDecl`]
//! must emit
//!
//! - a `bind_list` runtime import, gated on the list being present,
//! - a `walk_path(root, ..., "data-rsjs-list", "<container>")` lookup
//!   for the container element,
//! - a `create` builder that constructs the item subtree imperatively
//!   and returns a `{ node, update }` record,
//! - an `adopt` builder that reuses the server-rendered child, and
//! - the `bind_list(container, getter, { key, create, adopt, deps })`
//!   call with an explicit dep array over the driving signal.

use rsjs_ir::lists::{ListDecl, ListItemExpr, ListSource, ListTemplateNode};
use rsjs_ir::{emit_js, ClientExpr, IslandBuilder, QueryAccessor, QueryOptions, Value};

/// `<ul>{for item in items.get() { html_sync! { <li key={item}>{item}</li> } }}</ul>`
fn build_keyed_list_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    // `signal(vec![...])` lowers with a `Value::Null` fallback; the
    // real initial value rides the dehydrated state blob.
    let items = b.signal(Value::Null);

    let root = b.element("div", None);
    let container = b.element("ul", Some(root));
    b.static_attr(container, "data-rsjs-list", &container.0.to_string());
    let loop_node = b.for_loop(container, "", "", "item", "items . get ()", "");
    let item_root = b.element("li", Some(loop_node));
    b.server_only_node(item_root, "(item) . clone ()");

    b.keyed_list(ListDecl {
        container,
        loop_node,
        item_root,
        source: ListSource::signal(items),
        key: ListItemExpr::Item,
        key_tokens: "item".to_string(),
        item: ListTemplateNode::Element {
            tag: "li".to_string(),
            static_attrs: Vec::new(),
            dynamic_attrs: Vec::new(),
            children: vec![ListTemplateNode::Slot(ListItemExpr::Item)],
        },
    });
    b.set_root(root);
    b.build()
}

/// Item template with a dynamic href, a nested slot-bearing element,
/// and a struct field key:
/// `<li key={row.id} href={row.url}><span class="t">{row.title}</span></li>`.
fn build_field_list_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();
    let rows = b.signal(Value::Null);

    let root = b.element("div", None);
    let container = b.element("ul", Some(root));
    b.static_attr(container, "data-rsjs-list", &container.0.to_string());
    let loop_node = b.for_loop(container, "", "", "row", "rows . get ()", "");
    let item_root = b.element("li", Some(loop_node));
    let span = b.element("span", Some(item_root));
    b.static_attr(span, "class", "t");
    b.server_only_node(span, "(row . title) . clone ()");

    b.keyed_list(ListDecl {
        container,
        loop_node,
        item_root,
        source: ListSource::signal(rows),
        key: ListItemExpr::ItemField("id".to_string()),
        key_tokens: "row . id".to_string(),
        item: ListTemplateNode::Element {
            tag: "li".to_string(),
            static_attrs: Vec::new(),
            dynamic_attrs: vec![(
                "href".to_string(),
                ListItemExpr::ItemField("url".to_string()),
            )],
            children: vec![ListTemplateNode::Element {
                tag: "span".to_string(),
                static_attrs: vec![("class".to_string(), "t".to_string())],
                dynamic_attrs: Vec::new(),
                children: vec![ListTemplateNode::Slot(ListItemExpr::ItemField(
                    "title".to_string(),
                ))],
            }],
        },
    });
    b.set_root(root);
    b.build()
}

/// Query-backed list source: `<ul>{for row in rows.data() { ... }}</ul>`.
fn build_query_list_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let key_expr = b.expr(ClientExpr::Literal(Value::String("rows".to_string())));
    let server_initial = b.expr(ClientExpr::Literal(Value::Null));
    let client_fetcher = b.expr(ClientExpr::Literal(Value::String("/api/rows".to_string())));
    let rows_query = b.query(
        key_expr,
        server_initial,
        client_fetcher,
        QueryOptions::default(),
    );
    let rows_expr = b.expr(ClientExpr::QueryRead {
        id: rows_query,
        accessor: QueryAccessor::Data,
    });

    let root = b.element("div", None);
    let container = b.element("ul", Some(root));
    b.static_attr(container, "data-rsjs-list", &container.0.to_string());
    let loop_node = b.for_loop(container, "", "", "row", "rows . data ()", "");
    let item_root = b.element("li", Some(loop_node));
    b.server_only_node(item_root, "(row . title) . clone ()");

    b.keyed_list(ListDecl {
        container,
        loop_node,
        item_root,
        source: ListSource::expr(rows_expr),
        key: ListItemExpr::ItemField("id".to_string()),
        key_tokens: "row . id".to_string(),
        item: ListTemplateNode::Element {
            tag: "li".to_string(),
            static_attrs: Vec::new(),
            dynamic_attrs: Vec::new(),
            children: vec![ListTemplateNode::Slot(ListItemExpr::ItemField(
                "title".to_string(),
            ))],
        },
    });
    b.set_root(root);
    b.build()
}

#[test]
fn keyed_list_emits_bind_list_with_create_adopt_and_deps() {
    let island = build_keyed_list_island();
    let js = emit_js::emit(&island, "TodoList");

    assert!(
        js.contains("bind_list"),
        "expected a bind_list runtime import and call; got:\n{js}"
    );
    assert!(
        js.lines().next().unwrap_or_default().contains("bind_list"),
        "bind_list must be imported from the runtime; got:\n{js}"
    );
    // The view tree contains a ForLoop, which disables static index
    // walks; the container resolves through the marker selector.
    assert!(
        js.contains("root.querySelector('[data-rsjs-list=\"1\"]')"),
        "container lookup must use the data-rsjs-list marker; got:\n{js}"
    );
    assert!(
        js.contains("const __list_create_0 = (item) => {"),
        "expected the create builder; got:\n{js}"
    );
    assert!(
        js.contains("document.createElement(\"li\")"),
        "create must build the item root imperatively; got:\n{js}"
    );
    assert!(
        js.contains("__li_0_0.textContent = String(item);"),
        "update must re-apply the slot textContent; got:\n{js}"
    );
    assert!(
        js.contains("const __list_adopt_0 = (node, item) => {"),
        "expected the adopt builder; got:\n{js}"
    );
    assert!(
        js.contains("key: (item) => String(item),"),
        "expected the key callback over the item; got:\n{js}"
    );
    assert!(
        js.contains("deps: [__sig_0],"),
        "expected the explicit dep array; got:\n{js}"
    );
    assert!(
        js.contains("__rsjs_disposers.push(bind_list(__node_1, () => __sig_0.get(), {"),
        "expected the bind_list call over the container and getter; got:\n{js}"
    );
}

#[test]
fn keyed_list_field_template_resolves_nested_slots_by_child_index() {
    let island = build_field_list_island();
    let js = emit_js::emit(&island, "RowList");

    assert!(
        js.contains("key: (item) => String(item.id),"),
        "field keys must lower to item.<field>; got:\n{js}"
    );
    assert!(
        js.contains(".setAttribute(\"class\", \"t\");"),
        "static attributes must be re-applied in create; got:\n{js}"
    );
    assert!(
        js.contains("__li_0_0.setAttribute(\"href\", String(item.url));"),
        "dynamic item attributes must update from item fields; got:\n{js}"
    );
    assert!(
        js.contains("__li_0_1.textContent = String(item.title);"),
        "nested slot must update through its own element var; got:\n{js}"
    );
    assert!(
        js.contains("const __li_0_1 = node.children[0];"),
        "adopt must re-resolve the nested slot element by child index; got:\n{js}"
    );
}

#[test]
fn keyed_list_expr_source_emits_getter_deps_and_extra_handles() {
    let island = build_query_list_island();
    let js = emit_js::emit(&island, "QueryRows");

    assert!(
        js.contains("__rsjs_disposers.push(bind_list(__node_1, () => __rsjs_query_0.data(), {"),
        "expected list getter to read the query accessor; got:\n{js}"
    );
    assert!(
        js.contains("deps: [],"),
        "query-only list source should have an empty signal dep array; got:\n{js}"
    );
    assert!(
        js.contains("extra_handles: [__rsjs_query_0],"),
        "query-backed list source must subscribe to the query handle; got:\n{js}"
    );
}

#[test]
fn keyed_list_keeps_signal_alive_through_dead_signal_elimination() {
    let mut island = build_keyed_list_island();
    rsjs_ir::default_passes().run_all(&mut island);
    assert_eq!(
        island.signals.len(),
        1,
        "the list-driving signal must survive dead-signal elimination"
    );
}

#[test]
fn keyed_list_expr_source_keeps_nested_signal_alive() {
    let mut b = IslandBuilder::new();
    let items = b.signal(Value::Null);
    let items_expr = b.expr(ClientExpr::SignalRead(items));

    let root = b.element("div", None);
    let container = b.element("ul", Some(root));
    let loop_node = b.for_loop(container, "", "", "item", "items . get ()", "");
    let item_root = b.element("li", Some(loop_node));

    b.keyed_list(ListDecl {
        container,
        loop_node,
        item_root,
        source: ListSource::expr(items_expr),
        key: ListItemExpr::Item,
        key_tokens: "item".to_string(),
        item: ListTemplateNode::Element {
            tag: "li".to_string(),
            static_attrs: Vec::new(),
            dynamic_attrs: Vec::new(),
            children: vec![ListTemplateNode::Slot(ListItemExpr::Item)],
        },
    });
    b.set_root(root);

    let mut island = b.build();
    rsjs_ir::default_passes().run_all(&mut island);
    assert_eq!(
        island.signals.len(),
        1,
        "signals read by expression-backed list sources must survive DSE"
    );
}

/// A fragment whose *root element* carries reactive bindings while the
/// tree contains a for-loop. Mirrors the marketing site's SearchModal:
/// `<dialog data-open={open.get()} on_document_keydown={...}>` wrapping
/// a keyed results list. For-loop trees disable static index walks, so
/// every node resolves through the marker-selector fallback; that
/// fallback must consider the fragment root itself, because
/// `querySelector` matches descendants only and the fragment mount
/// receives the marked root element directly.
fn build_root_bound_list_island() -> rsjs_ir::Island {
    use rsjs_ir::{AttrBindingKind, ClientBlock, ClientSimpleClosure, ClientStmt, EventName};

    let mut b = IslandBuilder::new();
    let open = b.signal(Value::Bool(false));
    let items = b.signal(Value::Null);

    let root = b.element("dialog", None);
    let open_expr = b.expr(ClientExpr::SignalRead(open));
    b.bind_attr(root, "data-open", AttrBindingKind::Value, open_expr);
    let keydown_handler =
        b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_update(
            open,
            ClientSimpleClosure::new("_c", ClientExpr::Literal(Value::Bool(true))),
        )]));
    b.bind_event(root, EventName::DocumentKeyDown, keydown_handler);

    let container = b.element("ul", Some(root));
    b.static_attr(container, "data-rsjs-list", &container.0.to_string());
    let loop_node = b.for_loop(container, "", "", "item", "items . get ()", "");
    let item_root = b.element("li", Some(loop_node));
    b.server_only_node(item_root, "(item) . clone ()");

    b.keyed_list(ListDecl {
        container,
        loop_node,
        item_root,
        source: ListSource::signal(items),
        key: ListItemExpr::Item,
        key_tokens: "item".to_string(),
        item: ListTemplateNode::Element {
            tag: "li".to_string(),
            static_attrs: Vec::new(),
            dynamic_attrs: Vec::new(),
            children: vec![ListTemplateNode::Slot(ListItemExpr::Item)],
        },
    });
    b.set_root(root);
    b.build()
}

#[test]
fn fragment_root_lookup_falls_back_to_root_inclusive_selector() {
    let island = build_root_bound_list_island();
    let js = emit_js::emit_fragment(&island, "searchdialog_00000000");

    // The root element's own lookups must be able to resolve `root`
    // itself, not only its descendants.
    assert!(
        js.contains("(root.matches('[data-rsjs-ref=\"0\"]') ? root : root.querySelector('[data-rsjs-ref=\"0\"]'))")
            || js.contains("(root.matches('[data-rsjs-attr=\"0\"]') ? root : root.querySelector('[data-rsjs-attr=\"0\"]'))"),
        "the root node's marker lookup must consider root itself; got:\n{js}"
    );
    assert!(
        js.contains("bind_document_event"),
        "the document keydown binding must be emitted; got:\n{js}"
    );
}

#[test]
fn island_wrapper_root_lookup_keeps_selector_fallback_reachable() {
    let island = build_root_bound_list_island();
    let js = emit_js::emit(&island, "SearchDialog");

    // Island modules receive the `<rsjs-island>` wrapper as `root`; the
    // wrapper never carries binding markers, so the root-inclusive form
    // must still fall through to the descendant search that finds the
    // user-authored root.
    assert!(
        js.contains("root.querySelector('["),
        "island wrapper mode must keep the descendant selector fallback; got:\n{js}"
    );
}
