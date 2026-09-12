// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Coverage for [`ClientBrowserEffect::Declared`], the statement-side
//! mirror of `ClientBrowserRead::Declared`.
//!
//! A registry entry with a `DeclaredEffect` lowering is supposed to be the
//! entire compiler-side cost of a new simple browser side effect: no
//! dedicated IR variant, no JSON arm, no emitter arm. These tests pin the
//! three properties that promise depends on.

use rsjs_ir::browser_registry::{find_effect, find_read, HelperLowering};
use rsjs_ir::client::{client_block_json, emit_client_stmt_js, parse_client_block_json_str};
use rsjs_ir::{ClientBlock, ClientBrowserEffect, ClientExpr, ClientStmt};

fn set_root_class(class: &str, on: bool) -> ClientStmt {
    ClientStmt::BrowserEffect(ClientBrowserEffect::Declared {
        helper: "root.set_class".to_string(),
        args: vec![ClientExpr::string(class), ClientExpr::bool(on)],
    })
}

#[test]
fn a_declared_effect_emits_its_registry_js_template_with_args_spliced() {
    let js = emit_client_stmt_js(&set_root_class("dark", true)).expect("emit");

    // The `$0` / `$1` placeholders are replaced by the lowered args, in
    // source order, and the statement is terminated.
    assert!(
        js.contains("document.documentElement.classList.toggle(\"dark\", !!(true))"),
        "unexpected emitted JS: {js}"
    );
    assert!(
        !js.contains('$'),
        "template placeholder left unspliced: {js}"
    );
}

#[test]
fn the_emitted_js_survives_a_missing_dom() {
    // Island modules can be evaluated where `document` is absent. The
    // guard is part of the registry template, so assert it survives
    // emission rather than trusting the table by eye.
    let js = emit_client_stmt_js(&set_root_class("dark", false)).expect("emit");
    assert!(
        js.contains("typeof document !== \"undefined\""),
        "declared effect must guard a missing document: {js}"
    );
}

#[test]
fn the_root_helpers_are_registered_with_the_lowerings_they_rely_on() {
    let set_class = find_effect("root", "set_class").expect("root.set_class is a handler effect");
    assert!(
        matches!(set_class.lowering, HelperLowering::DeclaredEffect { .. }),
        "root.set_class must lower declaratively"
    );
    assert_eq!(set_class.args.len(), 2, "set_class(class, on)");

    let has_class = find_read("root", "has_class").expect("root.has_class is a reactive read");
    assert!(
        matches!(has_class.lowering, HelperLowering::DeclaredRead { .. }),
        "root.has_class must lower declaratively"
    );

    // Context is what keeps an effect out of value position and a read out
    // of statement position.
    assert!(find_read("root", "set_class").is_none());
    assert!(find_effect("root", "has_class").is_none());
}

#[test]
fn a_declared_effect_round_trips_through_descriptor_json() {
    let effect = ClientBrowserEffect::Declared {
        helper: "root.set_class".to_string(),
        args: vec![ClientExpr::string("dark"), ClientExpr::bool(true)],
    };
    let stmt = ClientStmt::BrowserEffect(effect.clone());

    let block = ClientBlock { stmts: vec![stmt] };

    let json = client_block_json(&block);
    let parsed = parse_client_block_json_str(&json).expect("declared effect JSON must parse back");

    assert_eq!(parsed, block, "round trip changed the effect");
}

#[test]
fn an_unknown_declared_helper_is_rejected_when_parsing() {
    let json = r#"{"stmts":[{"kind":"browser_effect","effect":{"kind":"declared","helper":"root.not_a_helper","args":[]}}]}"#;
    assert!(
        parse_client_block_json_str(json).is_err(),
        "a declared effect naming no registry entry must not parse"
    );
}
