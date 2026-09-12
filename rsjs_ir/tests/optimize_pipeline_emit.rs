// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end regressions for the `default_passes()` pipeline.
//!
//! These tests deliberately assert on the *emitted JavaScript*, not on
//! IR shape alone. Both bugs they pin were invisible at the IR level in
//! isolation: the pass and the reachability analysis each looked
//! self-consistent, and only the JS the emitter produced from the
//! post-pass island revealed that a `__sig_<id>` was referenced without
//! ever being declared.
//!
//! The shared assertion is [`assert_no_undeclared_signal_vars`]: every
//! `__sig_<id>` the module mentions must have a matching declaration in
//! the same module. A violation is a `ReferenceError` in the browser.

use std::collections::BTreeSet;

use rsjs_ir::{
    default_passes, emit_js, ClientBlock, ClientExpr, ClientStmt, ComponentFragmentCallback,
    ComponentFragmentSignalAlias, EventName, Island, IslandBuilder, SignalId, Value,
};

/// Every `__sig_<id>` identifier the module mentions.
fn referenced_signal_vars(js: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = js;
    while let Some(at) = rest.find("__sig_") {
        let after = &rest[at + "__sig_".len()..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            found.insert(format!("__sig_{digits}"));
        }
        rest = &after[digits.len()..];
    }
    found
}

/// Every `__sig_<id>` identifier the module declares. The emitter only
/// ever introduces signal cells with `const __sig_<id> = ...`.
fn declared_signal_vars(js: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in js.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("const __sig_") else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            found.insert(format!("__sig_{digits}"));
        }
    }
    found
}

/// Assert the emitted module never reads a signal cell it did not declare.
fn assert_no_undeclared_signal_vars(js: &str) {
    let referenced = referenced_signal_vars(js);
    let declared = declared_signal_vars(js);
    let undeclared: Vec<_> = referenced.difference(&declared).cloned().collect();
    assert!(
        undeclared.is_empty(),
        "emitted island JS references undeclared signal cells {undeclared:?}\n\
         declared: {declared:?}\n\
         --- emitted module ---\n{js}"
    );
}

fn signal_var(id: SignalId) -> String {
    format!("__sig_{}", id.0)
}

/// C4 regression: a parent signal that is only ever handed to a mounted
/// child fragment (as a reactive prop alias) or written from a
/// parent-side callback adapter must survive `dead_signal_elimination`.
///
/// Before the fix the pass never walked `island.component_fragment_mounts`,
/// so both signals were dropped while `emit_component_fragment_mounts`
/// still emitted `__sig_<id>` by name.
#[test]
fn fragment_mount_keeps_alias_and_callback_signals_alive_through_default_passes() {
    let mut b = IslandBuilder::new();

    // `visible` is only ever forwarded to the child as a reactive prop.
    let visible = b.signal(Value::Bool(false));
    // `clicks` is only ever written from the parent-side callback adapter,
    // whose block lives on the mount rather than in `island.handlers`.
    let clicks = b.signal(Value::Int(0));
    // `keep` has an ordinary parent reader, so the island hydrates and the
    // test exercises the emitter rather than the server-only short circuit.
    let keep = b.signal(Value::Int(7));

    let root = b.element("section", None);
    let label = b.element("span", Some(root));
    let keep_read = b.expr(ClientExpr::signal_read(keep));
    b.bind_text(label, keep_read);

    let child_root = b.element("div", Some(root));
    let slot = b.callback_slot("on_pick");
    b.component_fragment_mount(
        child_root,
        "child_fragment".to_string(),
        vec![ComponentFragmentCallback::from_ir(
            slot,
            ClientBlock::from_stmts(vec![ClientStmt::signal_set(clicks, ClientExpr::int(5))]),
        )],
        vec![ComponentFragmentSignalAlias {
            child: SignalId(0),
            parent: visible,
            prop_name: Some("visible".to_string()),
            source_anchor: None,
        }],
        Vec::new(),
        vec![child_root],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    b.set_root(root);

    let mut island: Island = b.build();
    let before: Vec<SignalId> = island.signals.iter().map(|decl| decl.id).collect();
    default_passes().run_all(&mut island);
    let after: Vec<SignalId> = island.signals.iter().map(|decl| decl.id).collect();

    assert_eq!(
        after, before,
        "signals referenced only through a component fragment mount must survive the pass"
    );

    let js = emit_js::emit(&island, "MountParent");
    assert!(
        js.contains(&signal_var(visible)),
        "mount call should still hand the parent signal cell to the child:\n{js}"
    );
    assert!(
        js.contains(&signal_var(clicks)),
        "callback adapter should still write the parent signal:\n{js}"
    );
    assert_no_undeclared_signal_vars(&js);
}

/// The alias signal alone (no callbacks) is enough to keep a signal live.
#[test]
fn fragment_mount_signal_alias_alone_survives_default_passes() {
    let mut b = IslandBuilder::new();
    let visible = b.signal(Value::Bool(true));
    let keep = b.signal(Value::Int(1));

    let root = b.element("section", None);
    let label = b.element("span", Some(root));
    let keep_read = b.expr(ClientExpr::signal_read(keep));
    b.bind_text(label, keep_read);
    let child_root = b.element("div", Some(root));
    b.component_fragment_mount(
        child_root,
        "child_fragment".to_string(),
        Vec::<ComponentFragmentCallback>::new(),
        vec![ComponentFragmentSignalAlias {
            child: SignalId(0),
            parent: visible,
            prop_name: Some("visible".to_string()),
            source_anchor: None,
        }],
        Vec::new(),
        vec![child_root],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    b.set_root(root);

    let mut island = b.build();
    default_passes().run_all(&mut island);

    assert!(
        island.signals.iter().any(|decl| decl.id == visible),
        "aliased parent signal must not be eliminated"
    );
    let js = emit_js::emit(&island, "AliasOnlyParent");
    assert!(js.contains(&signal_var(visible)));
    assert_no_undeclared_signal_vars(&js);
}

/// C16 regression: `handler_dedup` preserves ids while removing merged
/// duplicates, so the handlers vector goes sparse. Reachability used to
/// index it by `EventHandlerId` as a position, which silently skipped the
/// surviving high-id handler and filtered away the signal declarations its
/// body needs.
#[test]
fn sparse_handler_ids_after_dedup_still_contribute_reachability_roots() {
    let mut b = IslandBuilder::new();
    let count = b.signal(Value::Int(0));
    let other = b.signal(Value::Int(0));

    // Two byte-identical handlers: dedup merges the second into the first
    // and removes it, leaving handler ids [0, 2].
    let first = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        count,
        ClientExpr::int(1),
    )]));
    let second = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        count,
        ClientExpr::int(1),
    )]));
    // The sole writer of `other`, and the only handler with a high id.
    let third = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::signal_set(
        other,
        ClientExpr::int(9),
    )]));

    let root = b.element("div", None);
    let span = b.element("span", Some(root));
    let count_read = b.expr(ClientExpr::signal_read(count));
    b.bind_text(span, count_read);
    let first_button = b.element("button", Some(root));
    let second_button = b.element("button", Some(root));
    let third_button = b.element("button", Some(root));
    b.bind_event(first_button, EventName::Click, first);
    b.bind_event(second_button, EventName::Click, second);
    b.bind_event(third_button, EventName::Click, third);
    b.set_root(root);

    let mut island = b.build();
    default_passes().run_all(&mut island);

    let handler_ids: Vec<u32> = island.handlers.iter().map(|handler| handler.id.0).collect();
    assert_eq!(
        handler_ids,
        vec![first.0, third.0],
        "dedup must leave the handlers vector sparse for this test to be meaningful"
    );

    let analysis = rsjs_ir::classify_render_strategy(&island);
    let reachable = analysis.reachable_signal_ids();
    assert!(
        reachable.contains(&other),
        "the surviving high-id handler's signal writes must be reachable, got {reachable:?}"
    );

    let js = emit_js::emit(&island, "SparseHandlers");
    assert!(
        js.contains(&format!("const {} =", signal_var(other))),
        "the signal written by the surviving high-id handler must be declared:\n{js}"
    );
    assert_no_undeclared_signal_vars(&js);
}
