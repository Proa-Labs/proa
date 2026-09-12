// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Reviewer A's gap notes flagged that `ClientExpr::Project` over
//! `QueryAccessor::Error`, [`ClientExpr::IsPresent`], and [`ClientExpr::Neq`] had
//! no end-to-end emit_js coverage. This file hand-builds an island
//! exercising each of those shapes and asserts the emitted JS
//! contains the expected runtime helper call.
//!
//! Substring asserts only; once the runtime API stabilises (M9), the
//! shapes become byte-stable goldens the same way Counter does.

use rsjs_ir::{
    emit_js, ClientExpr, IslandBuilder, JsonPointer, QueryAccessor, QueryOptions, Value,
};

/// Build a tiny query-bearing island that wires every shape we want
/// to cover:
///
/// - `ClientExpr::Project { target: QueryRead { accessor: Error }, .. }`
///   over a JSON Pointer (the error projection chain that v0.1
///   surfaces for typed error payloads);
/// - `ClientExpr::IsPresent { target: SignalRead(_) }` (the presence
///   predicate over a signal, which lowers to `rsjs.__is_present(...)`);
/// - `ClientExpr::Neq { target, value }` (the structural-inequality
///   terminator, which lowers to `rsjs.__neq(...)`).
fn build_projection_chain_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();

    let signal = b.signal(Value::String(String::new()));
    let key = b.expr(ClientExpr::Literal(Value::String("k".to_string())));
    let server = b.expr(ClientExpr::Literal(Value::Null));
    let client = b.expr(ClientExpr::Literal(Value::String("/api/k".to_string())));
    let q = b.query(
        key,
        server,
        client,
        QueryOptions {
            interval_ms: None,
            refetch_on_focus: false,
            download_progress: false,
            stream_text: false,
            retry_count: 0,
            stale_time_ms: 0,
        },
    );

    // Project { target: QueryRead { accessor: Error }, path: "/code" }
    let error_project = b.expr(ClientExpr::Project {
        target: Box::new(ClientExpr::QueryRead {
            id: q,
            accessor: QueryAccessor::Error,
        }),
        path: JsonPointer::new("/code"),
    });

    // IsPresent { target: SignalRead(signal) }
    let is_present = b.expr(ClientExpr::IsPresent {
        target: Box::new(ClientExpr::SignalRead(signal)),
    });

    // Neq { target: SignalRead(signal), value: "ok" }
    let neq = b.expr(ClientExpr::Neq {
        target: Box::new(ClientExpr::SignalRead(signal)),
        value: Value::String("ok".to_string()),
    });

    let root = b.element("div", None);
    let error_span = b.element("span", Some(root));
    let presence_span = b.element("span", Some(root));
    let neq_span = b.element("span", Some(root));

    b.bind_text(error_span, error_project);
    b.bind_text(presence_span, is_present);
    b.bind_text(neq_span, neq);
    b.set_root(root);

    b.build()
}

#[test]
fn project_over_query_error_lowers_to_runtime_helper() {
    let island = build_projection_chain_island();
    let js = emit_js::emit(&island, "projection_chain");
    assert!(
        js.contains("rsjs.__project("),
        "expected rsjs.__project call site for Project node; got:\n{js}"
    );
    // The Error accessor lowers via the runtime's error() method.
    assert!(
        js.contains(".error()"),
        "expected QueryAccessor::Error to lower as `.error()`; got:\n{js}"
    );
    // The path segment from the JSON Pointer must reach the runtime.
    assert!(
        js.contains("\"code\""),
        "expected projected path segment `\"code\"` to reach the emitted JS; got:\n{js}"
    );
}

#[test]
fn is_present_lowers_to_runtime_helper() {
    let island = build_projection_chain_island();
    let js = emit_js::emit(&island, "projection_chain");
    assert!(
        js.contains("rsjs.__is_present("),
        "expected rsjs.__is_present call site; got:\n{js}"
    );
}

#[test]
fn neq_lowers_to_runtime_helper() {
    let island = build_projection_chain_island();
    let js = emit_js::emit(&island, "projection_chain");
    assert!(
        js.contains("rsjs.__neq("),
        "expected rsjs.__neq call site; got:\n{js}"
    );
}

/// Build an island with no queries and no mutations, but with an
/// `IsPresent` projection-helper expression bound to a signal. The
/// import gate must widen to include the rsjs runtime even though
/// neither queries nor mutations are present, otherwise the emitted
/// `rsjs.__is_present` call would `ReferenceError` at hydration.
fn build_helpers_only_island() -> rsjs_ir::Island {
    let mut b = IslandBuilder::new();
    let signal = b.signal(Value::String(String::new()));
    let is_present = b.expr(ClientExpr::IsPresent {
        target: Box::new(ClientExpr::SignalRead(signal)),
    });
    let root = b.element("div", None);
    let span = b.element("span", Some(root));
    b.bind_text(span, is_present);
    b.set_root(root);
    b.build()
}

#[test]
fn helpers_only_island_imports_rsjs_runtime() {
    // The island has no queries and no mutations, but its bindings
    // reference IsPresent (a runtime-helper-bearing variant). The
    // emitter must still import the rsjs namespace, otherwise the
    // emitted `rsjs.__is_present(...)` call has nothing to bind to.
    let island = build_helpers_only_island();
    let js = emit_js::emit(&island, "helpers_only");
    assert!(
        js.contains(r#"from "/rsjs-runtime-query.js""#),
        "expected rsjs runtime import line for an island whose bindings \
         use a projection helper, even without queries/mutations; got:\n{js}"
    );
    assert!(
        js.contains("rsjs.__is_present("),
        "expected rsjs.__is_present call site; got:\n{js}"
    );
}
