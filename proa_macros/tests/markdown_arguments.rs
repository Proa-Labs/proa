// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![deny(warnings)]

use proa_core::{render_md_to_string_async, render_to_string as render_md_to_string};
use proa_macros::{md, md_sync};
use std::cell::RefCell;

#[test]
fn named_arguments_respect_rust_scopes_and_nested_macros() {
    let value = "caller";
    let output = render_md_to_string(md_sync! { r###"
        {value}
        {if true { let value = "branch"; value }}
        {for value in ["a", "b"] { md_sync! { "{value}" } }}
        {match Some("arm") { Some(value) => value, None => "" }}
        { { let f = |value| value; f("closure") } }
        { { let Some(value) = Some("let-else") else { return Ok(()) }; value } }
        {if let Some(value) = Some("if-let") { value }}
        { { let mut values = ["while"].into_iter(); while let Some(value) = values.next() { value } } }
        {if true { md_sync! { "{value}" } }}
        "###,
        value = value,
    });
    assert_eq!(
        output,
        "caller\nbranch\nab\narm\nclosure\nlet\\-else\nif\\-let\nwhile\ncaller\n"
    );
}

#[test]
fn arguments_evaluate_once_in_source_order_in_the_callers_scope() {
    let a = 10;
    let calls = RefCell::new(Vec::new());
    let fragment = md_sync! { "{b}/{a}/{a}{if false { c }}",
        a = { calls.borrow_mut().push("a"); 1 },
        b = { calls.borrow_mut().push("b"); a },
        c = { calls.borrow_mut().push("c"); 3 },
    };
    assert!(
        calls.borrow().is_empty(),
        "arguments are deferred until rendering"
    );
    assert_eq!(render_md_to_string(fragment), "10/1/1");
    assert_eq!(*calls.borrow(), ["a", "b", "c"]);

    let value = 7;
    assert_eq!(
        render_md_to_string(md_sync! {
            "{ { let value = 99; let _ = value; item } }", item = value
        }),
        "7",
        "a local must not capture an argument's free names"
    );
}

#[test]
fn argument_bindings_and_render_context_are_hygienic() {
    let cx = "caller";
    let __proa_md_cx = "context";
    assert_eq!(
        render_md_to_string(md_sync! { "{cx}/{__proa_md_cx}" }),
        "caller/context"
    );
    assert_eq!(
        render_md_to_string(md_sync! {
            "{cx}/{__proa_md_cx}", cx = "named", __proa_md_cx = "argument"
        }),
        "named/argument"
    );
    assert_eq!(
        render_md_to_string(md_sync! { "prefix " {cx} }),
        "prefix caller"
    );
}

#[tokio::test]
async fn async_named_arguments_keep_borrows_and_scopes_across_pending_slots() {
    let value = String::from("borrowed");
    let borrowed = &value;
    let output = render_md_to_string_async(md! { r###"
        {&value}
        { { tokio::task::yield_now().await; &value } }
        {if true { let value = "local"; md! { "{value}" } }}
        {if true { md! { "{&value}" } }}
        {cx}
        "###,
        value = borrowed,
        cx = "named",
    })
    .await;
    assert_eq!(output, "borrowed\nborrowed\nlocal\nborrowed\nnamed\n");
    assert_eq!(value, "borrowed");
}
